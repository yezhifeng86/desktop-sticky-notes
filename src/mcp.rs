//! MCP (stdio) 服务 —— 让 AI **不用任何提示**就能操作本程序。
//!
//! 启动：`桌面便利贴.exe mcp`
//! 传输：stdin/stdout 上的换行分隔 JSON-RPC（MCP stdio transport）。
//! 客户端（Hermes / OpenCode / Claude 等）注册本进程后，下面的工具会直接
//! 出现在 AI 的工具列表里，AI 打开就知道"有什么能力、参数是什么"，
//! 既不需要读源码，也不需要有人告诉它先敲 `--help`。
//!
//! 实现要点：工具调用统一转成命令行参数、再以子进程方式执行本 exe，
//! 捕获其 stdout/stderr 回传 —— 与 CLI 共用一套逻辑，避免两处实现走偏。

use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL_FALLBACK: &str = "2025-06-18";

/// 工具表：name / description / inputSchema 是 AI "自学" 的全部材料
fn tools() -> Vec<Value> {
    let selector = json!({
        "type": "string",
        "description": "计划选择器：完整 id，或标题/内容里唯一的关键词（不区分大小写）；命中多条会返回候选列表"
    });
    vec![
        json!({
            "name": "app_info",
            "description": "返回本程序的版本、数据文件位置、命令行用法与字段说明。第一次调用建议先用它了解全貌。",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        }),
        json!({
            "name": "list_tasks",
            "description": "列出全部计划（含 id、标题、内容、截止时间、进度、置顶、完成状态、排序号）。界面正在运行时数据始终是最新的。",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        }),
        json!({
            "name": "add_task",
            "description": "新建一条计划，返回新条目的完整字段（含 id）。deadline 格式 YYYY-MM-DDTHH:MM，也接受 YYYY-MM-DD（按当天 00:00）。新计划排在列表末尾。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "计划标题（必填）"},
                    "content": {"type": "string", "description": "计划内容/备注"},
                    "deadline": {"type": "string", "description": "截止时间 YYYY-MM-DDTHH:MM"},
                    "progress": {"type": "integer", "minimum": 0, "maximum": 100, "description": "进度 0-100，默认0"},
                    "pinned": {"type": "boolean", "description": "是否置顶，默认 false"}
                },
                "required": ["title"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "update_task",
            "description": "修改已有计划的任意字段；只传要改的字段即可，未传的字段保持不变。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": selector.clone(),
                    "title": {"type": "string", "description": "新标题"},
                    "content": {"type": "string", "description": "新内容"},
                    "deadline": {"type": "string", "description": "新截止时间 YYYY-MM-DDTHH:MM"},
                    "clear_deadline": {"type": "boolean", "description": "true 时清除截止时间"},
                    "progress": {"type": "integer", "minimum": 0, "maximum": 100},
                    "pinned": {"type": "boolean", "description": "true=置顶 false=取消置顶"}
                },
                "required": ["selector"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "complete_task",
            "description": "把计划标记为已完成（进度自动设为100）；off=true 取消完成标记。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": selector.clone(),
                    "off": {"type": "boolean", "description": "true=取消完成"}
                },
                "required": ["selector"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "pin_task",
            "description": "置顶/取消置顶一条计划；置顶的会排在列表最前。off=true 取消置顶。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": selector.clone(),
                    "off": {"type": "boolean", "description": "true=取消置顶"}
                },
                "required": ["selector"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "delete_task",
            "description": "删除一条或多条计划（不可恢复，调用前请确认用户意图）。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selectors": {
                        "type": "array",
                        "items": {"type": "string"},
                        "minItems": 1,
                        "description": "要删除的计划选择器列表"
                    }
                },
                "required": ["selectors"],
                "additionalProperties": false
            }
        }),
    ]
}

/// 把工具调用翻译成本 exe 的命令行参数，再以子进程执行、捕获输出
fn call_tool(name: &str, args: &Value) -> Result<Vec<String>, String> {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let flag = |k: &str| args.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let mut argv: Vec<String> = Vec::new();
    match name {
        "app_info" => argv.push("describe".into()),
        "list_tasks" => argv.push("list".into()),
        "add_task" => {
            let title = s("title").ok_or("add_task 缺少必填参数 title")?;
            argv.push("add".into());
            argv.push(title);
            if let Some(v) = s("content") {
                argv.push("--content".into());
                argv.push(v);
            }
            if let Some(v) = s("deadline") {
                argv.push("--deadline".into());
                argv.push(v);
            }
            if let Some(v) = args.get("progress").and_then(|p| p.as_i64()) {
                argv.push("--progress".into());
                argv.push(v.to_string());
            }
            if flag("pinned") {
                argv.push("--pin".into());
            }
        }
        "update_task" => {
            let sel = s("selector").ok_or("update_task 缺少必填参数 selector")?;
            argv.push("set".into());
            argv.push(sel);
            if let Some(v) = s("title") {
                argv.push("--title".into());
                argv.push(v);
            }
            if let Some(v) = s("content") {
                argv.push("--content".into());
                argv.push(v);
            }
            if flag("clear_deadline") {
                argv.push("--clear-deadline".into());
            } else if let Some(v) = s("deadline") {
                argv.push("--deadline".into());
                argv.push(v);
            }
            if let Some(v) = args.get("progress").and_then(|p| p.as_i64()) {
                argv.push("--progress".into());
                argv.push(v.to_string());
            }
            if let Some(b) = args.get("pinned").and_then(|p| p.as_bool()) {
                argv.push(if b { "--pin" } else { "--unpin" }.into());
            }
        }
        "complete_task" => {
            let sel = s("selector").ok_or("complete_task 缺少必填参数 selector")?;
            argv.push("done".into());
            argv.push(sel);
            if flag("off") {
                argv.push("--off".into());
            }
        }
        "pin_task" => {
            let sel = s("selector").ok_or("pin_task 缺少必填参数 selector")?;
            argv.push("pin".into());
            argv.push(sel);
            if flag("off") {
                argv.push("--off".into());
            }
        }
        "delete_task" => {
            let sels = args
                .get("selectors")
                .and_then(|v| v.as_array())
                .ok_or("delete_task 缺少必填参数 selectors（数组）")?;
            if sels.is_empty() {
                return Err("selectors 不能为空".into());
            }
            argv.push("del".into());
            for x in sels {
                let t = x
                    .as_str()
                    .ok_or("selectors 里的元素必须是字符串")?;
                argv.push(t.to_string());
            }
        }
        other => return Err(format!("未知工具: {other}")),
    }
    argv.push("--json".into());
    Ok(argv)
}

fn run_cli(argv: &[String]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let out = std::process::Command::new(exe)
        .args(argv)
        .output()
        .map_err(|e| format!("执行失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&stdout_of(&out)).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        Err(if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        })
    }
}

fn stdout_of(out: &std::process::Output) -> Vec<u8> {
    out.stdout.clone()
}

fn send(msg: Value) {
    let mut so = std::io::stdout();
    let _ = writeln!(so, "{}", msg);
    let _ = so.flush();
}

fn reply(id: Option<Value>, result: Value) {
    send(json!({"jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result}));
}

fn reply_err(id: Option<Value>, code: i64, message: &str) {
    send(json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {"code": code, "message": message}
    }));
}

/// 主循环：读一行处理一行，stdin 关闭即退出
pub fn serve() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                reply_err(None, -32700, &format!("解析失败: {e}"));
                continue;
            }
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            // 客户端要什么协议版本就回什么版本，保证握手不卡
            "initialize" => {
                let ver = msg
                    .get("params")
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(PROTOCOL_FALLBACK)
                    .to_string();
                reply(
                    id,
                    json!({
                        "protocolVersion": ver,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {
                            "name": "desktop-sticky-notes",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": concat!(
                            "本服务管理 Windows 桌面计划便签。先用 list_tasks 查看现状，",
                            "增删改后界面会自动刷新；计划用 selector（id 或标题唯一关键词）定位。"
                        )
                    }),
                );
            }
            "notifications/initialized" | "notifications/cancelled" | "notifications/roots/list_changed" => {}
            "ping" => reply(id, json!({})),
            "tools/list" => reply(id, json!({"tools": tools()})),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match call_tool(name, &args) {
                    Err(e) => reply(
                        id,
                        json!({"content": [{"type": "text", "text": e}], "isError": true}),
                    ),
                    Ok(argv) => match run_cli(&argv) {
                        Ok(text) => reply(
                            id,
                            json!({"content": [{"type": "text", "text": text}], "isError": false}),
                        ),
                        Err(e) => reply(
                            id,
                            json!({"content": [{"type": "text", "text": e}], "isError": true}),
                        ),
                    },
                }
            }
            other => {
                if id.is_some() {
                    reply_err(id, -32601, &format!("未知方法: {other}"));
                }
                // 未知通知按 MCP 规范忽略
            }
        }
    }
}
