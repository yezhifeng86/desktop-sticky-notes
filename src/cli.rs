//! 命令行接口 —— 让 AI / 脚本**不看界面、不读源码**就能操作本程序。
//!
//! 设计目标：拿到 exe 路径后执行一次 `--help`（人读）或 `describe --json`（机器读），
//! 立刻拿到完整操作手册：有哪些命令、每个命令的参数、示例、数据文件在哪、退出码含义。
//! 之后按需调用子命令即可；写入数据文件后，正在运行的界面会自动刷新（见 main.rs 的文件监听）。

use super::{data_path, load_data, now_ms, parse_deadline, uuid, Data, Item, APP_NAME};
use std::path::Path;

const HELP: &str = r#"桌面便利贴 · 命令行接口
================================
用法: 桌面便利贴.exe <命令> [参数...] [--json]

命令:
  help                       显示本帮助
  describe                   输出机器可读的接口说明(JSON) —— AI 请先执行这个
  list                       列出全部计划                    [--json]
  add <标题>                 新建计划                        [--content 内容] [--deadline 2026-12-31T18:00] [--progress 0-100] [--pin]
  set <选择器>               修改计划                        [--title 新标题] [--content 新内容] [--deadline 时间 | --clear-deadline] [--progress N] [--pin | --unpin]
  done <选择器>              标记为已完成                    [--off] 取消完成标记
  pin <选择器>               置顶切换                        [--off] 取消置顶
  del <选择器> [选择器...]   删除计划（可一次传多个）

选择器: 完整 id，或标题/内容里唯一的关键词（不区分大小写）；命中多条会列出候选让你收紧条件。
输出:   默认人读；任何命令加 --json 都得到机器可读结果。
退出码: 0=成功  1=用法错误  2=未找到或选择器多义  3=读写失败

示例:
  桌面便利贴.exe list --json
  桌面便利贴.exe add "周报" --content "汇总本周进度" --deadline 2026-11-10T17:00 --pin
  桌面便利贴.exe done 周报
  桌面便利贴.exe set 周报 --progress 60
  桌面便利贴.exe del 周报

给 AI: 先跑 `describe --json` 拿接口描述，再调命令；结果统一用 --json 解析。
"#;

/// 不占值的开关
const SWITCHES: &[&str] = &["--json", "--pin", "--unpin", "--off", "--clear-deadline"];
/// 需要一个值的选项
const VALUE_FLAGS: &[&str] = &["--content", "--deadline", "--progress", "--title"];

#[derive(Default)]
struct Opts {
    pos: Vec<String>,
    json: bool,
    pin: bool,
    unpin: bool,
    off: bool,
    clear_deadline: bool,
    content: Option<String>,
    deadline: Option<String>,
    progress: Option<i32>,
    title: Option<String>,
}

fn parse_opts(rest: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        if SWITCHES.contains(&a) {
            match a {
                "--json" => o.json = true,
                "--pin" => o.pin = true,
                "--unpin" => o.unpin = true,
                "--off" => o.off = true,
                "--clear-deadline" => o.clear_deadline = true,
                _ => {}
            }
        } else if VALUE_FLAGS.contains(&a) {
            i += 1;
            let v = rest
                .get(i)
                .ok_or_else(|| format!("选项 {a} 需要一个值"))?
                .clone();
            match a {
                "--content" => o.content = Some(v),
                "--deadline" => o.deadline = Some(v),
                "--title" => o.title = Some(v),
                "--progress" => {
                    let n: i32 = v
                        .parse()
                        .map_err(|_| format!("--progress 需要 0-100 的整数，收到 {v}"))?;
                    o.progress = Some(n.clamp(0, 100));
                }
                _ => {}
            }
        } else if a.starts_with("--") {
            return Err(format!("未知选项 {a}（可用选项见 help）"));
        } else {
            o.pos.push(a.to_string());
        }
        i += 1;
    }
    Ok(o)
}

/// 选择器 → 条目下标：完整 id 优先，其次标题/内容唯一关键词
fn resolve(data: &Data, sel: &str) -> Result<usize, String> {
    if let Some(i) = data.items.iter().position(|it| it.id == sel) {
        return Ok(i);
    }
    let l = sel.to_lowercase();
    let hits: Vec<usize> = data
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| {
            it.title.to_lowercase().contains(&l) || it.content.to_lowercase().contains(&l)
        })
        .map(|(i, _)| i)
        .collect();
    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err(format!("没有找到匹配“{sel}”的计划")),
        _ => {
            let list = hits
                .iter()
                .map(|&i| format!("[{}] {}", short(&data.items[i].id), data.items[i].title))
                .collect::<Vec<_>>()
                .join("  ");
            Err(format!("“{sel}”匹配到多条，请收紧关键词：{list}"))
        }
    }
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn item_json(it: &Item) -> serde_json::Value {
    serde_json::to_value(it).unwrap_or(serde_json::Value::Null)
}

/// 统一出口：--json 走机器格式，否则人读
fn emit(json: bool, human: String, value: serde_json::Value) {
    if json {
        println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
    } else {
        println!("{human}");
    }
}

fn load() -> (std::path::PathBuf, Data) {
    let p = data_path();
    let d = load_data(&p);
    (p, d)
}

fn save(path: &Path, data: &Data) -> Result<(), String> {
    let text = serde_json::to_string_pretty(data).map_err(|e| format!("序列化失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text).map_err(|e| format!("写入失败: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("替换失败: {e}"))?;
    Ok(())
}

fn normalize_deadline(raw: &str) -> Result<String, String> {
    let cand = if raw.len() == 10 {
        format!("{raw}T00:00")
    } else {
        raw.to_string()
    };
    if parse_deadline(&cand).is_some() {
        Ok(cand)
    } else {
        Err(format!(
            "时间格式无法识别: {raw}（示例 2026-12-31T18:00 或 2026-12-31）"
        ))
    }
}

/// 机器可读的接口说明：AI 拿到它就知道怎么操作本程序
fn describe(exe: &str) -> serde_json::Value {
    let data_file = data_path();
    serde_json::json!({
        "name": APP_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "kind": "desktop-sticky-notes-cli",
        "exe": exe,
        "dataFile": data_file.to_string_lossy(),
        "summary": "Windows 桌面计划便签。所有命令直接读写数据文件；界面正在运行时会自动刷新，无需重启。",
        "howToUse": "先执行本命令拿到接口描述，再按 commands 调用子命令；结果建议加 --json 解析。",
        "mcpMode": format!("`{exe} mcp` 以 MCP stdio 服务运行：AI 客户端注册后，list_tasks/add_task/complete_task/pin_task/update_task/delete_task/app_info 会自动出现在工具列表里，AI 无需任何提示即可操作本程序。"),
        "selector": "计划可用完整 id 指定，也可用标题/内容里唯一的关键词（不区分大小写）；多义时命令会返回候选列表。",
        "timeFormat": "YYYY-MM-DDTHH:MM（也接受 YYYY-MM-DD，按当天 00:00 处理）",
        "exitCodes": {"0":"成功","1":"用法错误","2":"未找到或选择器多义","3":"读写失败"},
        "output": {"default":"人读文本","--json":"机器可读 JSON（字段与数据文件一致）"},
        "commands": [
            {"name":"help",      "args":[],                                "example":"help",                     "desc":"显示人读帮助"},
            {"name":"describe",  "args":[],                                "example":"describe --json",         "desc":"输出本接口说明(JSON)"},
            {"name":"list",      "args":["--json"],                        "example":"list --json",             "desc":"列出全部计划（含 id/标题/进度/截止/置顶/完成）"},
            {"name":"add",       "args":["<标题>","--content","--deadline","--progress","--pin"], "example":"add \"周报\" --deadline 2026-11-10T17:00 --pin", "desc":"新建计划，返回新条目"},
            {"name":"set",       "args":["<选择器>","--title","--content","--deadline|--clear-deadline","--progress","--pin|--unpin"], "example":"set 周报 --progress 60", "desc":"修改计划的任意字段"},
            {"name":"done",      "args":["<选择器>","--off"],              "example":"done 周报",               "desc":"标记完成；--off 取消"},
            {"name":"pin",       "args":["<选择器>","--off"],              "example":"pin 周报",                "desc":"置顶切换；--off 取消置顶"},
            {"name":"del",       "args":["<选择器>..."],                   "example":"del 周报 旧任务",         "desc":"删除一个或多个计划"},
        ],
        "itemFields": {
            "id":"唯一标识","title":"标题","content":"内容",
            "deadline":"YYYY-MM-DDTHH:MM 或 null","progress":"0-100",
            "isPinned":"是否置顶","isCompleted":"是否完成",
            "order":"手动排序号（拖拽后写入）","createdAt":"毫秒时间戳","updatedAt":"毫秒时间戳","color":null
        },
        "examples": [
            "桌面便利贴.exe list --json",
            "桌面便利贴.exe add \"周报\" --content \"汇总本周进度\" --deadline 2026-11-10T17:00",
            "桌面便利贴.exe done 周报",
            "桌面便利贴.exe del 周报"
        ]
    })
}

fn human_list(d: &Data) -> String {
    if d.items.is_empty() {
        return "暂无计划".to_string();
    }
    let pinned = d.items.iter().filter(|i| i.is_pinned).count();
    let done = d.items.iter().filter(|i| i.is_completed).count();
    let mut out = format!(
        "{} 条计划（置顶 {} / 已完成 {}）\n",
        d.items.len(),
        pinned,
        done
    );
    for it in &d.items {
        let tags = format!(
            "{}{}",
            if it.is_pinned { "[置顶] " } else { "" },
            if it.is_completed { "[已完成] " } else { "" }
        );
        let dl = it
            .deadline
            .as_deref()
            .map(|s| format!(" 截止 {s}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  {}{}  进度{}%{}  id={}\n",
            tags,
            it.title,
            it.progress,
            dl,
            it.id
        ));
    }
    out
}

/// 入口：返回进程退出码
pub fn run(args: &[String]) -> i32 {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| APP_NAME.to_string());

    let (first, rest) = match args.split_first() {
        Some((f, r)) => (f.as_str(), r),
        None => {
            print!("{HELP}");
            return 0;
        }
    };

    match first {
        "help" | "--help" | "-h" | "-?" => {
            print!("{HELP}");
            0
        }
        "describe" | "--describe" => {
            let json = rest.iter().any(|a| a == "--json") || first.starts_with("--");
            emit(
                json,
                format!("接口说明用 --json 查看：{exe} describe --json"),
                describe(&exe),
            );
            0
        }
        "list" | "ls" => {
            let o = match parse_opts(rest) {
                Ok(o) => o,
                Err(e) => return fail(&e),
            };
            let (_, d) = load();
            let value = serde_json::json!({
                "count": d.items.len(),
                "items": d.items.iter().map(item_json).collect::<Vec<_>>(),
            });
            emit(o.json, human_list(&d), value);
            0
        }
        "add" => {
            let o = match parse_opts(rest) {
                Ok(o) => o,
                Err(e) => return fail(&e),
            };
            let title = match o.pos.first() {
                Some(t) if !t.trim().is_empty() => t.trim().to_string(),
                _ => return fail("add 需要标题：add \"标题\" [--content ...] [--deadline ...]"),
            };
            let deadline = match &o.deadline {
                Some(s) => match normalize_deadline(s) {
                    Ok(v) => Some(v),
                    Err(e) => return fail(&e),
                },
                None => None,
            };
            let (path, mut d) = load();
            let now = now_ms();
            let next_order = d.items.iter().map(|i| i.order).max().unwrap_or(0) + 1;
            let item = Item {
                id: uuid(),
                title,
                content: o.content.unwrap_or_default(),
                deadline,
                progress: o.progress.unwrap_or(0).clamp(0, 100),
                is_pinned: o.pin,
                is_completed: false,
                created_at: now,
                updated_at: now,
                color: None,
                order: next_order,
            };
            let human = format!("已新建：{}  id={}", item.title, item.id);
            let value = serde_json::json!({"ok": true, "item": item_json(&item)});
            d.items.push(item);
            if let Err(e) = save(&path, &d) {
                return fail(&e);
            }
            emit(o.json, human, value);
            0
        }
        "set" => {
            let o = match parse_opts(rest) {
                Ok(o) => o,
                Err(e) => return fail(&e),
            };
            let sel = match o.pos.first() {
                Some(s) => s.clone(),
                None => return fail("set 需要选择器：set <id或标题关键词> [--title ...]"),
            };
            let (path, mut d) = load();
            let idx = match resolve(&d, &sel) {
                Ok(i) => i,
                Err(e) => return fail(&e),
            };
            if o.title.is_none()
                && o.content.is_none()
                && o.deadline.is_none()
                && !o.clear_deadline
                && o.progress.is_none()
                && !o.pin
                && !o.unpin
            {
                return fail("set 没有任何要改的字段（见 help）");
            }
            let it = &mut d.items[idx];
            if let Some(t) = o.title {
                it.title = t;
            }
            if let Some(c) = o.content {
                it.content = c;
            }
            if o.clear_deadline {
                it.deadline = None;
            } else if let Some(s) = o.deadline {
                match normalize_deadline(&s) {
                    Ok(v) => it.deadline = Some(v),
                    Err(e) => return fail(&e),
                }
            }
            if let Some(p) = o.progress {
                it.progress = p.clamp(0, 100);
            }
            if o.pin {
                it.is_pinned = true;
            }
            if o.unpin {
                it.is_pinned = false;
            }
            it.updated_at = now_ms();
            let human = format!("已更新：{}", it.title);
            let value = serde_json::json!({"ok": true, "item": item_json(&d.items[idx])});
            if let Err(e) = save(&path, &d) {
                return fail(&e);
            }
            emit(o.json, human, value);
            0
        }
        "done" => {
            let o = match parse_opts(rest) {
                Ok(o) => o,
                Err(e) => return fail(&e),
            };
            let sel = match o.pos.first() {
                Some(s) => s.clone(),
                None => return fail("done 需要选择器：done <id或标题关键词> [--off]"),
            };
            let (path, mut d) = load();
            let idx = match resolve(&d, &sel) {
                Ok(i) => i,
                Err(e) => return fail(&e),
            };
            let it = &mut d.items[idx];
            it.is_completed = !o.off;
            if it.is_completed {
                it.progress = 100;
            }
            it.updated_at = now_ms();
            let human = format!(
                "{}：{}",
                if it.is_completed { "已完成" } else { "已取消完成" },
                it.title
            );
            let value = serde_json::json!({"ok": true, "item": item_json(&d.items[idx])});
            if let Err(e) = save(&path, &d) {
                return fail(&e);
            }
            emit(o.json, human, value);
            0
        }
        "pin" => {
            let o = match parse_opts(rest) {
                Ok(o) => o,
                Err(e) => return fail(&e),
            };
            let sel = match o.pos.first() {
                Some(s) => s.clone(),
                None => return fail("pin 需要选择器：pin <id或标题关键词> [--off]"),
            };
            let (path, mut d) = load();
            let idx = match resolve(&d, &sel) {
                Ok(i) => i,
                Err(e) => return fail(&e),
            };
            let it = &mut d.items[idx];
            it.is_pinned = !o.off;
            it.updated_at = now_ms();
            let human = format!(
                "{}：{}",
                if it.is_pinned { "已置顶" } else { "已取消置顶" },
                it.title
            );
            let value = serde_json::json!({"ok": true, "item": item_json(&d.items[idx])});
            if let Err(e) = save(&path, &d) {
                return fail(&e);
            }
            emit(o.json, human, value);
            0
        }
        "del" | "delete" | "rm" => {
            let o = match parse_opts(rest) {
                Ok(o) => o,
                Err(e) => return fail(&e),
            };
            if o.pos.is_empty() {
                return fail("del 需要至少一个选择器：del <id或标题关键词> [...]");
            }
            let (path, mut d) = load();
            // 先全部解析成功再删，避免删一半失败
            let mut idxs = Vec::new();
            for sel in &o.pos {
                match resolve(&d, sel) {
                    Ok(i) => {
                        if !idxs.contains(&i) {
                            idxs.push(i);
                        }
                    }
                    Err(e) => return fail(&e),
                }
            }
            idxs.sort_unstable_by(|a, b| b.cmp(a));
            let mut removed = Vec::new();
            for i in idxs {
                removed.push(d.items.remove(i));
            }
            if let Err(e) = save(&path, &d) {
                return fail(&e);
            }
            let human = format!(
                "已删除 {} 条：{}",
                removed.len(),
                removed
                    .iter()
                    .map(|i| i.title.as_str())
                    .collect::<Vec<_>>()
                    .join("、")
            );
            let value = serde_json::json!({
                "ok": true,
                "deleted": removed.iter().map(item_json).collect::<Vec<_>>(),
                "remaining": d.items.len(),
            });
            emit(o.json, human, value);
            0
        }
        other => {
            eprintln!("未知命令：{other}\n");
            print!("{HELP}");
            1
        }
    }
}

fn fail(msg: &str) -> i32 {
    eprintln!("错误: {msg}");
    // 用法类错误给一个机器可读的尾巴，方便 AI 解析
    eprintln!("{}", serde_json::json!({"ok": false, "error": msg}));
    if msg.contains("用法") || msg.contains("需要") || msg.contains("未知") || msg.contains("格式") {
        1
    } else if msg.contains("没有找到") || msg.contains("多条") {
        2
    } else {
        3
    }
}
