//! 桌面便利贴 —— 单文件桌面计划便签（Rust + eframe/egui）
//!
//! 单文件 exe，无运行时依赖，Win10+ 双击即用。
//!
//! 功能：
//!   - 无边框置顶窗口、自绘标题栏（📌 置顶 / ─ 最小化 / □ 最大化 / × 关闭到托盘）
//!   - 计划条目：标题 / 完成时间 / 内容 / 进度 / 置顶 / 勾选完成 / 拖拽排序
//!   - 筛选（全部·置顶·今日·本周·已过期）、实时搜索 + 高亮、已完成开关
//!   - 截止时间状态：过期(红) / 1小时内(红·闪烁) / 24小时内(橙)
//!   - 边缘吸附自动隐藏，鼠标靠近边缘弹出，离开 500ms 再收回
//!   - 关闭/最小化 → 隐藏到托盘；托盘双击恢复，右键菜单 显示/新建/退出
//!   - 数据落在主程序同目录、同名 .json（便携式，中文名无影响）

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Windows FFI：4 个 unsafe 函数体内直接调用 unsafe API，保持 edition2021 的
// `unsafe fn` 语义（Rust 2024 会要求逐处补显式 unsafe 块，届时再迁移）。
#![allow(unsafe_op_in_unsafe_fn)]

mod cli;
mod mcp;
mod tray;
mod win32;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, Timelike};
use eframe::egui::{self, *};
use serde::{Deserialize, Serialize};

const APP_NAME: &str = "桌面便利贴";
/// 软件版本（关于页显示）
const VERSION: &str = "1.0.0";
/// 关于页作者
const AUTHOR: &str = "青山白马少年";
/// 边框缩放命中区宽度（egui 点）
const BORDER: f32 = 6.0;

/// 正在进行的边框缩放：edge 位掩码 1=左 2=右 4=上 8=下
#[derive(Clone, Copy)]
struct ResizeDrag {
    edge: u8,
    start: Pos2,
    rect: (i32, i32, i32, i32),
}
const DATA_DIR: &str = "desktop-sticky-notes";
/// 嵌入的多尺寸 .ico（由 assets/gen_icon.py 生成），用于窗口/任务栏/托盘图标
const APP_ICON: &[u8] = include_bytes!("../assets/app.ico");

// ---- 色阶阶梯：同一色相由浅到深分档，界面层次靠明度差表达（全暖色、无灰）----
// 面（surface）进阶：窗口 → 非置顶卡（普通/已完成）→ 置顶卡（一档比一档深）
// 非置顶卡刻意做淡，与置顶卡的色差要一眼看得出来
const Y_100: Color32 = Color32::from_rgb(0xFE, 0xF9, 0xC9); // 窗口底（最浅的面）
const Y_150: Color32 = Color32::from_rgb(0xFF, 0xF6, 0xBE); // 非置顶卡（淡，比窗口深一档）
const Y_200: Color32 = Color32::from_rgb(0xFF, 0xEC, 0x94); // 置顶卡（最深的面）
const Y_300: Color32 = Color32::from_rgb(0xFC, 0xEF, 0x71); // 标题栏 / 强调底
const Y_400: Color32 = Color32::from_rgb(0xFB, 0xE4, 0x55); // 悬停
const Y_500: Color32 = Color32::from_rgb(0xF6, 0xC3, 0x43); // 主按钮 / 激活标签 / 进度
const Y_600: Color32 = Color32::from_rgb(0xED, 0xAC, 0x45); // 描边 / 边框
const Y_700: Color32 = Color32::from_rgb(0xD9, 0x93, 0x1F); // 按下 / 强调描边
// 暖中性阶（代替灰阶，色相偏暖）
const N_050: Color32 = Color32::from_rgb(0xFF, 0xFD, 0xE9); // 输入框 / 次按钮底
const N_300: Color32 = Color32::from_rgb(0xF2, 0xE6, 0xC4); // 卡片描边（浅）
const N_600: Color32 = Color32::from_rgb(0x88, 0x6F, 0x65); // 次要文字
const N_900: Color32 = Color32::from_rgb(0x59, 0x41, 0x39); // 主文字
// 语义色阶
const R_500: Color32 = Color32::from_rgb(0xD3, 0x48, 0x3E); // 危险 / 置顶徽章
const R_600: Color32 = Color32::from_rgb(0xB2, 0x3A, 0x32); // 危险按下
const O_500: Color32 = Color32::from_rgb(0xD5, 0x5B, 0x25); // 警告
const G_500: Color32 = Color32::from_rgb(0x43, 0xA0, 0x47); // 完成

// ---- 语义别名（调用点沿用旧名，颜色全部来自上面的色阶）----
const C_BG: Color32 = Y_100;
/// 普通卡：不置顶的淡面
const C_CARD: Color32 = Y_150;
/// 置顶卡片：比非置顶卡深一档，色差一眼可见
const C_BG_PIN: Color32 = Y_200;
/// 已完成卡片与普通卡同色（都是不置顶的淡面），靠左侧金条与绿勾区分
const C_BG_DONE: Color32 = Y_150;
const C_LIGHT: Color32 = N_050;
const C_ACCENT: Color32 = Y_300;
const C_BORDER: Color32 = Y_600;
const C_TEXT: Color32 = N_900;
const C_TEXT2: Color32 = N_600;
const C_RED: Color32 = R_500;
const C_ORANGE: Color32 = O_500;
const C_GREEN: Color32 = G_500;
const C_ITEM_BORDER: Color32 = Y_600;
const C_CARD_BORDER: Color32 = N_300;
const C_HL: Color32 = Y_400;
const C_PRIMARY: Color32 = Y_500;

// ---------------------------------------------------------------- 数据模型

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct Item {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    deadline: Option<String>,
    #[serde(default)]
    progress: i32,
    #[serde(default)]
    is_pinned: bool,
    #[serde(default)]
    is_completed: bool,
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    updated_at: i64,
    #[serde(default)]
    color: Option<String>,
    /// 手动排序号（拖拽后写入）；0 = 未手动排过，走截止时间兜底排序
    #[serde(default)]
    order: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct WindowBounds {
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    window_bounds: Option<WindowBounds>,
    always_on_top: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Data {
    #[serde(default)]
    items: Vec<Item>,
    #[serde(default)]
    settings: Settings,
}

/// 目录可写探测（便携版放在 Program Files 之类只读位置时会失败）
fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".write_probe");
    match std::fs::write(&probe, b"ok") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 数据文件：**与主程序同目录、同名 .json**（便携式，中文名无影响）。
/// exe 所在目录不可写时（如 Program Files），退回 %APPDATA%\desktop-sticky-notes\。
fn data_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.with_extension("json");
        if candidate.exists() {
            return candidate;
        }
        if let Some(dir) = candidate.parent() {
            if dir_writable(dir) {
                return candidate;
            }
        }
    }

    // 兜底：系统应用数据目录
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(base).join(DATA_DIR);
    let _ = std::fs::create_dir_all(&dir);
    dir.join("data.json")
}

fn load_data(path: &Path) -> Data {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Data::default(),
    }
}

// ---------------------------------------------------------------- 时间工具

#[derive(PartialEq, Eq, Clone, Copy)]
enum Status {
    None,
    Normal,
    Warning,
    Urgent,
    Expired,
}

fn parse_deadline(s: &str) -> Option<NaiveDateTime> {
    let t = s.trim();
    for fmt in [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(t, fmt) {
            return Some(dt);
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0);
    }
    None
}

fn time_status(deadline: &Option<String>) -> Status {
    let Some(raw) = deadline else {
        return Status::None;
    };
    let Some(dt) = parse_deadline(raw) else {
        return Status::None;
    };
    let now = Local::now().naive_local();
    let diff = dt.signed_duration_since(now).num_milliseconds();
    if diff < 0 {
        Status::Expired
    } else if diff < 60 * 60 * 1000 {
        Status::Urgent
    } else if diff < 24 * 60 * 60 * 1000 {
        Status::Warning
    } else {
        Status::Normal
    }
}

/// 日期格式化：只显示「M月D日」
fn format_deadline_short(deadline: &Option<String>) -> String {
    match deadline.as_deref().and_then(parse_deadline) {
        Some(dt) => format!("{}月{}日", dt.month(), dt.day()),
        None => String::new(),
    }
}

fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

fn uuid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = nanos
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut hex = String::with_capacity(32);
    for _ in 0..16 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        hex.push_str(&format!("{:02x}", ((state >> 33) & 0xFF) as u8));
    }
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

/// 按字符数截断
fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// 拆成 (文本, 是否命中)，用于搜索高亮
fn split_highlight(text: &str, query: &str) -> Vec<(String, bool)> {
    if query.is_empty() {
        return vec![(text.to_string(), false)];
    }
    let lower = text.to_lowercase();
    let q = query.to_lowercase();
    let mut out = Vec::new();
    let mut idx = 0usize;
    while let Some(pos) = lower[idx..].find(&q) {
        let abs = idx + pos;
        if abs > idx {
            out.push((text[idx..abs].to_string(), false));
        }
        let end = abs + q.len();
        out.push((text[abs..end].to_string(), true));
        idx = end;
        if idx >= lower.len() {
            break;
        }
    }
    if idx < text.len() {
        out.push((text[idx..].to_string(), false));
    }
    if out.is_empty() {
        out.push((text.to_string(), false));
    }
    out
}

fn paint_runs(
    p: &Painter,
    x: f32,
    y_center: f32,
    text: &str,
    query: &str,
    font: FontId,
    color: Color32,
) -> f32 {
    let mut cursor = x;
    for (piece, hit) in split_highlight(text, query) {
        if piece.is_empty() {
            continue;
        }
        let galley = p.layout_no_wrap(piece, font.clone(), color);
        let size = galley.size();
        let pos = pos2(cursor, y_center - size.y * 0.5);
        if hit {
            p.rect_filled(
                Rect::from_min_size(pos2(pos.x - 1.0, pos.y), size + vec2(2.0, 0.0)),
                2.0,
                C_HL,
            );
        }
        p.galley(pos, galley, color);
        cursor += size.x;
    }
    cursor - x
}

/// 简易按宽度折行（中文按 1 字符宽估算），最多返回 2 行
fn wrap_by_width(text: &str, per_line: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() && lines.len() < 2 {
        let end = (i + per_line).min(chars.len());
        lines.push(chars[i..end].iter().collect());
        i = end;
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// ---------------------------------------------------------------- 状态枚举

#[derive(PartialEq, Eq, Clone, Copy)]
enum Filter {
    All,
    Pinned,
    Today,
    Week,
    Expired,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Modal {
    None,
    Edit,
    Confirm,
    About,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

// ---------------------------------------------------------------- 应用主体

struct Planner {
    data: Data,
    path: PathBuf,

    show_completed: bool,
    filter: Filter,
    search: String,
    search_query: String,
    search_edited: Option<Instant>,

    modal: Modal,
    editing: Option<String>,
    confirm_ids: Vec<String>,
    // 多选
    select_mode: bool,
    selected: std::collections::HashSet<String>,
    f_title: String,
    f_content: String,
    f_deadline: String,
    f_progress: i32,
    f_pinned: bool,
    form_error: Option<String>,

    // 时间选择器（日历 + 时分下拉，替代纯文本输入）
    show_picker: bool,
    pick_year: i32,
    pick_month: u32,
    pick_day: u32,
    pick_hour: u32,
    pick_min: u32,

    always_on_top: bool,
    hwnd: isize,
    visible: bool,
    quitting: bool,
    focus_search: bool,

    tray_rx: Option<Receiver<tray::TrayMsg>>,
    item_rects: Vec<(String, Rect)>,
    drop_id: Option<String>,
    drop_pt: Option<Pos2>,
    save_pending: bool,
    /// 上次读写数据文件的 mtime：与磁盘不一致说明被命令行/AI 外部改过，需重载
    last_data_mtime: Option<std::time::SystemTime>,

    // 边缘吸附
    edge: Option<Edge>,
    pending_edge: Option<Edge>,
    resize: Option<ResizeDrag>,
    // 边沿吸附状态机：区分"用户拖的"和"我们自己隐藏/恢复移的"，
    // 否则启动时窗口贴边会被自己吸走（实测踩过：打开就没）
    /// 吸附隐藏后，鼠标必须先离开触发区一次才允许弹回（防止"吸走→立刻弹回"闪烁）
    edge_armed: bool,
    last_tick_rect: Option<(i32, i32, i32, i32)>,
    self_move_at: Option<Instant>,
    user_moved_at: Option<Instant>,
    shown_at: Option<Instant>,
    restore_pos: Option<(i32, i32)>,
    near_edge_since: Option<Instant>,
    mouse_out_since: Option<Instant>,
    last_bounds: Option<(i32, i32, i32, i32)>,
    position_checked: bool,
}

impl Planner {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_style(&cc.egui_ctx);

        let path = data_path();
        let data = load_data(&path);
        let last_data_mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        let always_on_top = data.settings.always_on_top.unwrap_or(true);

        let (tx, rx) = std::sync::mpsc::channel();
        tray::spawn(tx);
        win32::ensure_autostart();

        Self {
            data,
            path,
            show_completed: false,
            filter: Filter::All,
            search: String::new(),
            search_query: String::new(),
            search_edited: None,
            modal: Modal::None,
            editing: None,
            confirm_ids: Vec::new(),
            select_mode: false,
            selected: Default::default(),
            f_title: String::new(),
            f_content: String::new(),
            f_deadline: String::new(),
            f_progress: 0,
            f_pinned: false,
            form_error: None,
            show_picker: false,
            pick_year: Local::now().year(),
            pick_month: Local::now().month(),
            pick_day: Local::now().day(),
            pick_hour: 9,
            pick_min: 0,
            always_on_top,
            hwnd: 0,
            visible: true,
            quitting: false,
            focus_search: false,
            tray_rx: Some(rx),
            item_rects: Vec::new(),
            drop_id: None,
            drop_pt: None,
            save_pending: false,
            last_data_mtime,
            edge: None,
            pending_edge: None,
            resize: None,
            edge_armed: true,
            last_tick_rect: None,
            self_move_at: None,
            user_moved_at: None,
            shown_at: Some(Instant::now()),
            restore_pos: None,
            near_edge_since: None,
            mouse_out_since: None,
            last_bounds: None,
            position_checked: false,
        }
    }

    // ---- 持久化 ----

    fn mark_save(&mut self) {
        self.save_pending = true;
    }

    fn flush_save(&mut self) {
        if !self.save_pending {
            return;
        }
        self.save_pending = false;
        self.data.settings.always_on_top = Some(self.always_on_top);
        match serde_json::to_string_pretty(&self.data) {
            Ok(text) => {
                let tmp = self.path.with_extension("json.tmp");
                if std::fs::write(&tmp, &text).is_ok() {
                    let _ = std::fs::rename(&tmp, &self.path);
                    // 记下自己写的 mtime，之后磁盘变化才认得出是"外部改动"
                    self.last_data_mtime =
                        std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
                }
            }
            Err(e) => eprintln!("序列化失败: {e}"),
        }
    }

    /// 命令行 / AI 改了数据文件时自动重载，界面立即跟着变（无需重启）
    fn poll_external_data_change(&mut self) {
        if self.save_pending {
            return; // 自己正要落盘，等写完这一轮再看
        }
        let Ok(md) = std::fs::metadata(&self.path) else { return; };
        let Ok(mtime) = md.modified() else { return; };
        if self.last_data_mtime == Some(mtime) {
            return;
        }
        self.data = load_data(&self.path);
        self.last_data_mtime = std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        // 清掉已不存在的选中项，避免后续批量操作对空气生效
        self.selected.retain(|id| self.data.items.iter().any(|i| &i.id == id));
        // 正在编辑的条目若被外部删掉，关掉编辑框，否则保存时会误建新条目
        if let Some(id) = self.editing.clone() {
            if !self.data.items.iter().any(|i| i.id == id) {
                self.editing = None;
                if matches!(self.modal, Modal::Edit) {
                    self.modal = Modal::None;
                }
            }
        }
    }

    // ---- 窗口显示/隐藏 ----

    fn hide_window(&mut self, ctx: &Context) {
        self.visible = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.flush_save();
    }

    fn show_window(&mut self, ctx: &Context) {
        if self.edge.is_some() {
            self.restore_from_edge();
        }
        self.shown_at = Some(Instant::now());
        self.user_moved_at = None;
        self.visible = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    fn quit(&mut self, ctx: &Context) {
        self.quitting = true;
        self.flush_save();
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn toggle_always_on_top(&mut self, ctx: &Context) {
        self.always_on_top = !self.always_on_top;
        let level = if self.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
        self.mark_save();
    }

    fn toggle_maximize(&mut self, ctx: &Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    // ---- 表单 ----

    fn open_edit(&mut self, id: Option<String>) {
        self.form_error = None;
        self.show_picker = false;
        self.editing = id.clone();
        match id {
            Some(id) => {
                if let Some(item) = self.data.items.iter().find(|i| i.id == id) {
                    self.f_title = item.title.clone();
                    self.f_content = item.content.clone();
                    self.f_deadline = item.deadline.clone().unwrap_or_default();
                    self.f_progress = item.progress;
                    self.f_pinned = item.is_pinned;
                }
            }
            None => {
                // 新建时默认「今天 09:00」
                let now = Local::now();
                self.f_title.clear();
                self.f_content.clear();
                self.f_deadline = format!(
                    "{:04}-{:02}-{:02}T{:02}:00",
                    now.year(),
                    now.month(),
                    now.day(),
                    9
                );
                self.f_progress = 0;
                self.f_pinned = false;
            }
        }
        self.sync_picker();
        self.modal = Modal::Edit;
    }

    /// 把当前 f_deadline 同步进选择器（没有则默认今天 09:00）
    fn sync_picker(&mut self) {
        let now = Local::now();
        let (y, m, d, h, mi) = parse_deadline(&self.f_deadline)
            .map(|dt| (dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute()))
            .unwrap_or((now.year(), now.month(), now.day(), 9, 0));
        self.pick_year = y;
        self.pick_month = m;
        self.pick_day = d;
        self.pick_hour = h;
        self.pick_min = mi;
    }

    fn clamp_pick_day(&mut self) {
        if let Some(first) = NaiveDate::from_ymd_opt(self.pick_year, self.pick_month, 1) {
            let max = first.num_days_in_month() as u32;
            if self.pick_day > max {
                self.pick_day = max;
            }
        }
    }

    /// 完成时间选择器：显示当前值 + 展开日历/时分面板
    fn deadline_picker(&mut self, ui: &mut Ui) {
        let pretty = if self.f_deadline.is_empty() {
            "未设置".to_string()
        } else {
            parse_deadline(&self.f_deadline)
                .map(|dt| {
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}",
                        dt.year(),
                        dt.month(),
                        dt.day(),
                        dt.hour(),
                        dt.minute()
                    )
                })
                .unwrap_or_else(|| self.f_deadline.clone())
        };

        ui.horizontal(|ui| {
            let text = if self.f_deadline.is_empty() {
                "📅 选择完成时间…".to_string()
            } else {
                format!("📅 {pretty}")
            };
            if ui
                .add_sized(
                    vec2((ui.available_width() - 74.0).max(120.0), 28.0),
                    Button::new(text),
                )
                .clicked()
            {
                if !self.show_picker {
                    self.sync_picker();
                }
                self.show_picker = !self.show_picker;
            }
            if !self.f_deadline.is_empty() && ui.button("清除").clicked() {
                self.f_deadline.clear();
                self.show_picker = false;
            }
        });

        if self.show_picker {
            self.picker_panel(ui);
        }
    }

    /// 日历 + 时分下拉面板
    fn picker_panel(&mut self, ui: &mut Ui) {
        Frame::NONE
            .fill(C_LIGHT)
            .stroke(Stroke::new(1.0, C_BORDER))
            .corner_radius(8.0)
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                // 月份导航
                ui.horizontal(|ui| {
                    if ui.button("‹").clicked() {
                        self.pick_month -= 1;
                        if self.pick_month == 0 {
                            self.pick_month = 12;
                            self.pick_year -= 1;
                        }
                        self.clamp_pick_day();
                    }
                    ui.label(
                        RichText::new(format!("{} 年 {} 月", self.pick_year, self.pick_month))
                            .strong()
                            .size(13.0),
                    );
                    if ui.button("›").clicked() {
                        self.pick_month += 1;
                        if self.pick_month > 12 {
                            self.pick_month = 1;
                            self.pick_year += 1;
                        }
                        self.clamp_pick_day();
                    }
                    if ui.button("现在").clicked() {
                        let now = Local::now();
                        self.pick_year = now.year();
                        self.pick_month = now.month();
                        self.pick_day = now.day();
                        self.pick_hour = now.hour();
                        self.pick_min = now.minute();
                    }
                });

                // 星期表头
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for w in ["日", "一", "二", "三", "四", "五", "六"] {
                        ui.add_sized(
                            vec2(30.0, 16.0),
                            Label::new(RichText::new(w).size(11.0).color(C_TEXT2)),
                        );
                    }
                });

                // 日期网格
                if let Some(first) = NaiveDate::from_ymd_opt(self.pick_year, self.pick_month, 1) {
                    let lead = first.weekday().num_days_from_sunday() as usize;
                    let days = first.num_days_in_month() as u32;
                    let mut cells: Vec<Option<u32>> = vec![None; lead];
                    cells.extend((1..=days).map(Some));
                    while cells.len() % 7 != 0 {
                        cells.push(None);
                    }
                    for week in cells.chunks(7) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for cell in week {
                                match cell {
                                    None => {
                                        ui.add_sized(vec2(30.0, 24.0), Label::new(""));
                                    }
                                    Some(d) => {
                                        // cell 是 &Option<u32>，绑定模式下 d 是 &u32，要解引用
                                        let selected = *d == self.pick_day;
                                        if ui
                                            .add_sized(
                                                vec2(30.0, 24.0),
                                                Button::selectable(selected, d.to_string()),
                                            )
                                            .clicked()
                                        {
                                            self.pick_day = *d;
                                        }
                                    }
                                }
                            }
                        });
                    }
                }

                // 时分
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("时间").size(12.0).color(C_TEXT2));
                    ComboBox::from_id_salt("pick_hour")
                        .selected_text(format!("{:02} 时", self.pick_hour))
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            for h in 0..24u32 {
                                ui.selectable_value(&mut self.pick_hour, h, format!("{h:02}"));
                            }
                        });
                    ComboBox::from_id_salt("pick_min")
                        .selected_text(format!("{:02} 分", self.pick_min))
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            for m in 0..60u32 {
                                ui.selectable_value(&mut self.pick_min, m, format!("{m:02}"));
                            }
                        });
                });

                // 确定 / 取消
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("不设时间").clicked() {
                        self.f_deadline.clear();
                        self.show_picker = false;
                    }
                    if ui.add(Button::new("确定").fill(C_PRIMARY)).clicked() {
                        if let Some(date) =
                            NaiveDate::from_ymd_opt(self.pick_year, self.pick_month, self.pick_day)
                        {
                            if let Some(t) = date.and_hms_opt(self.pick_hour, self.pick_min, 0) {
                                self.f_deadline = format!(
                                    "{:04}-{:02}-{:02}T{:02}:{:02}",
                                    t.year(),
                                    t.month(),
                                    t.day(),
                                    t.hour(),
                                    t.minute()
                                );
                            }
                        }
                        self.show_picker = false;
                    }
                });
            });
    }

    fn save_item(&mut self) {
        let title = self.f_title.trim().to_string();
        if title.is_empty() {
            self.form_error = Some("请输入计划标题".into());
            return;
        }
        let deadline = if self.f_deadline.trim().is_empty() {
            None
        } else {
            Some(self.f_deadline.trim().to_string())
        };
        let now = now_ms();
        match self.editing.clone() {
            Some(id) => {
                if let Some(item) = self.data.items.iter_mut().find(|i| i.id == id) {
                    item.title = title;
                    item.content = self.f_content.trim().to_string();
                    item.deadline = deadline;
                    item.progress = self.f_progress.clamp(0, 100);
                    item.is_pinned = self.f_pinned;
                    item.updated_at = now;
                }
            }
            None => {
                let next_order = self.data.items.iter().map(|i| i.order).max().unwrap_or(0) + 1;
                self.data.items.push(Item {
                    id: uuid(),
                    title,
                    content: self.f_content.trim().to_string(),
                    deadline,
                    progress: self.f_progress.clamp(0, 100),
                    is_pinned: self.f_pinned,
                    is_completed: false,
                    created_at: now,
                    updated_at: now,
                    color: None,
                    order: next_order,
                });
            }
        }
        self.mark_save();
        self.modal = Modal::None;
        self.editing = None;
    }

    fn toggle_complete(&mut self, id: &str) {
        if let Some(item) = self.data.items.iter_mut().find(|i| i.id == id) {
            item.is_completed = !item.is_completed;
            if item.is_completed {
                item.progress = 100;
            }
            item.updated_at = now_ms();
            self.mark_save();
        }
    }

    fn toggle_pin_item(&mut self, id: &str) {
        if let Some(item) = self.data.items.iter_mut().find(|i| i.id == id) {
            item.is_pinned = !item.is_pinned;
            item.updated_at = now_ms();
            self.mark_save();
        }
    }

    fn delete_item(&mut self, id: &str) {
        self.data.items.retain(|i| i.id != id);
        self.selected.remove(id);
        self.mark_save();
    }

    // ---- 可见条目（过滤 + 搜索 + 排序）----

    fn visible_items(&self) -> Vec<Item> {
        let now = Local::now().naive_local();
        let today_end = now.date().and_hms_opt(23, 59, 59).unwrap_or(now);
        let week_end = now + chrono::Duration::days(7);
        let q = self.search_query.clone();

        let mut list: Vec<Item> = self
            .data
            .items
            .iter()
            .filter(|it| {
                if !self.show_completed && it.is_completed {
                    return false;
                }
                match self.filter {
                    Filter::All => true,
                    Filter::Pinned => it.is_pinned && !it.is_completed,
                    Filter::Today => !it.is_completed
                        && it.deadline
                            .as_deref()
                            .and_then(parse_deadline)
                            .map(|d| d <= today_end)
                            .unwrap_or(false),
                    Filter::Week => !it.is_completed
                        && it.deadline
                            .as_deref()
                            .and_then(parse_deadline)
                            .map(|d| d <= week_end)
                            .unwrap_or(false),
                    Filter::Expired => !it.is_completed
                        && it.deadline
                            .as_deref()
                            .and_then(parse_deadline)
                            .map(|d| d < now)
                            .unwrap_or(false),
                }
            })
            .filter(|it| {
                if q.is_empty() {
                    true
                } else {
                    it.title.to_lowercase().contains(&q)
                        || it.content.to_lowercase().contains(&q)
                }
            })
            .cloned()
            .collect();

        // 排序：置顶优先 → 手动顺序（拖拽写入）→ 截止时间兜底（没手动排过的仍按时间）
        list.sort_by(|a, b| {
            b.is_pinned
                .cmp(&a.is_pinned)
                .then_with(|| a.order.cmp(&b.order))
                .then_with(|| match (&a.deadline, &b.deadline) {
                    (Some(x), Some(y)) => parse_deadline(x).cmp(&parse_deadline(y)),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                })
        });
        list
    }

    // ---- 托盘 / 快捷键 / 关闭拦截 ----

    fn poll_tray(&mut self, ctx: &Context) {
        let mut actions = Vec::new();
        if let Some(rx) = &self.tray_rx {
            while let Ok(msg) = rx.try_recv() {
                actions.push(msg);
            }
        }
        for msg in actions {
            match msg {
                tray::TrayMsg::Show => self.show_window(ctx),
                tray::TrayMsg::NewItem => {
                    self.show_window(ctx);
                    self.open_edit(None);
                }
                tray::TrayMsg::About => {
                    self.show_window(ctx);
                    self.modal = Modal::About;
                }
                tray::TrayMsg::Quit => self.quit(ctx),
            }
        }
    }

    fn handle_close_request(&mut self, ctx: &Context) {
        let requested = ctx.input(|i| i.viewport().close_requested());
        if !requested || self.quitting {
            return;
        }
        // 点关闭只是隐藏到托盘
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        self.hide_window(ctx);
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let mut new_item = false;
        let mut focus_search = false;
        let mut escape = false;
        let mut select_all = false;
        ctx.input(|i| {
            if i.key_pressed(Key::Escape) {
                escape = true;
            }
            if i.modifiers.command && i.key_pressed(Key::N) {
                new_item = true;
            }
            if i.modifiers.command && i.key_pressed(Key::F) {
                focus_search = true;
            }
            if i.modifiers.command && i.key_pressed(Key::A) {
                select_all = true;
            }
        });
        if select_all && self.select_mode {
            let ids: Vec<String> = self
                .visible_items()
                .iter()
                .map(|i| i.id.clone())
                .collect();
            for id in ids {
                self.selected.insert(id);
            }
        }
        if escape {
            self.modal = Modal::None;
            self.form_error = None;
        }
        if new_item {
            self.open_edit(None);
        }
        if focus_search {
            self.focus_search = true;
        }
    }

    fn apply_search_debounce(&mut self) {
        if let Some(t) = self.search_edited {
            if t.elapsed() >= Duration::from_millis(300) {
                self.search_query = self.search.trim().to_lowercase();
                self.search_edited = None;
            }
        }
    }

    fn apply_pending_drop(&mut self) {
        // 两个字段必须都在才能取：否则 drop_pt 未到时 drop_id 会被白白取走，拖拽永远不生效
        if self.drop_id.is_none() || self.drop_pt.is_none() {
            return;
        }
        let id = self.drop_id.take().unwrap();
        let pt = self.drop_pt.take().unwrap();
        {
            let target = self
                .item_rects
                .iter()
                .find(|(_, r)| r.contains(pt))
                .map(|(tid, _)| tid.clone());
            let Some(target_id) = target else { return };
            if target_id == id {
                return;
            }
            // 必须在"可见顺序"里移动：列表每帧按 置顶→顺序→时间 重排，
            // 只动 data 顺序会被这次重排覆盖，表现为"拖了没反应"
            let mut vis: Vec<String> = self.visible_items().iter().map(|i| i.id.clone()).collect();
            let (Some(from), Some(to)) = (
                vis.iter().position(|x| *x == id),
                vis.iter().position(|x| *x == target_id),
            ) else {
                return;
            };
            if from == to {
                return;
            }
            vis.remove(from);
            vis.insert(to, id); // to=目标原索引：向下→插到目标后；向上→插到目标前
            // 写回：可见项按新顺序占据原槽位，不可见项原地不动；同时落 order 号
            let vis_set: std::collections::HashSet<String> = vis.iter().cloned().collect();
            let mut ordered = Vec::with_capacity(vis.len());
            for (i, v) in vis.iter().enumerate() {
                if let Some(mut it) = self.data.items.iter().find(|x| x.id == *v).cloned() {
                    it.order = i as i64;
                    ordered.push(it);
                }
            }
            let mut it = ordered.into_iter();
            for slot in self.data.items.iter_mut() {
                if vis_set.contains(&slot.id) {
                    if let Some(new_item) = it.next() {
                        *slot = new_item;
                    }
                }
            }
            self.mark_save();
        }
    }

    // ---- 边缘吸附（贴边隐藏 / 鼠标靠近弹出）----

    fn hide_to_edge(&mut self, edge: Edge) {
        if self.hwnd == 0 {
            return;
        }
        // 最大化的窗口位置由系统托管，直接 SetWindowPos 移不动（实测弹不回来），
        // 先退到普通状态再吸。
        if win32::is_zoomed(self.hwnd) {
            win32::unmaximize(self.hwnd);
        }
        let (wx, wy, ww, wh) = win32::window_rect(self.hwnd);
        let (ax, ay, aw, ah) = win32::work_area(self.hwnd);
        self.restore_pos = Some((wx, wy));
        let (nx, ny) = match edge {
            Edge::Top => (wx, ay - wh + 5),
            Edge::Bottom => (wx, ay + ah - 5),
            Edge::Left => (ax - ww + 5, wy),
            Edge::Right => (ax + aw - 5, wy),
        };
        win32::move_window(self.hwnd, nx, ny);
        self.self_move_at = Some(Instant::now());
        self.edge = Some(edge);
        self.edge_armed = false; // 鼠标多半还停在边上，先禁止弹回
        self.near_edge_since = None;
        self.mouse_out_since = None;
    }

    fn restore_from_edge(&mut self) {
        if let Some((x, y)) = self.restore_pos.take() {
            win32::move_window(self.hwnd, x, y);
            self.self_move_at = Some(Instant::now());
        }
        self.edge = None;
        self.near_edge_since = None;
        self.shown_at = Some(Instant::now()); // 弹出后重新给一个自吸宽限期
        self.user_moved_at = None; // 弹出不是用户拖的，别马上又吸回去
    }

    fn tick_edge(&mut self, ctx: &Context) {
        if !self.visible || self.quitting || self.hwnd == 0 {
            return;
        }
        // 最小化/已在屏幕外时不做贴边判断：最小化时窗口在 -32000，
        // 会被误判成"贴着左上边缘"而拖走（实测踩过）。
        if win32::is_minimized(self.hwnd) {
            return;
        }
        let (wx, wy, ww, wh) = win32::window_rect(self.hwnd);
        if ww <= 0 || wh <= 0 {
            return;
        }
        let (ax, ay, aw, ah) = win32::work_area(self.hwnd);
        if self.edge.is_none() && (wx + ww <= ax || wx >= ax + aw || wy + wh <= ay || wy >= ay + ah) {
            return;
        }
        // 位置变化归因：600ms 内的位移是我们自己（隐藏/恢复）造成的，
        // 否则就是用户在拖/缩放 —— 只有用户动过窗口才允许自动吸附隐藏。
        if let Some(prev) = self.last_tick_rect {
            if prev != (wx, wy, ww, wh) {
                let ours = self
                    .self_move_at
                    .map(|t| t.elapsed() < Duration::from_millis(600))
                    .unwrap_or(false);
                if !ours {
                    self.user_moved_at = Some(Instant::now());
                    // 用户动过窗口 → "弹出后待吸回"作废（否则会拿旧边乱吸：
                    // 实测：从顶边弹出后把窗口拖到底部，鼠标一离开就被吸回顶边）
                    self.pending_edge = None;
                }
            }
        }
        self.last_tick_rect = Some((wx, wy, ww, wh));
        let (cx, cy) = win32::cursor_pos();
        let in_win = cx >= wx && cx <= wx + ww && cy >= wy && cy <= wy + wh;
        // 阈值按 DPI 缩放：App 看到的是真实像素，写死60在200%屏上只有30逻辑点
        let ppp = ctx.pixels_per_point().max(1.0);
        let band = |v: f32| (v * ppp) as i32;
        let pointer_down = win32::left_button_down();

        // 吸附隐藏中：鼠标进入边缘触发区（窗口横向范围 ±20、距边 60px）→ 弹出
        if let Some(edge) = self.edge {
            // 被外部挪回屏幕内（典型：窗口吸附隐藏后用户又双击了 exe）→ 状态复位
            let fully_in =
                wx >= ax && wx + ww <= ax + aw && wy >= ay && wy + wh <= ay + ah;
            if fully_in {
                self.edge = None;
                self.restore_pos = None;
                self.pending_edge = None;
                self.near_edge_since = None;
                self.shown_at = Some(Instant::now());
                // 是被外部挪回来的（第二次双击 exe），不是用户拖的 → 不允许立刻再吸走
                self.user_moved_at = None;
            } else {
            let trigger = match edge {
                Edge::Top => cy < ay + band(60.0) && cx >= wx - band(20.0) && cx <= wx + ww + band(20.0),
                Edge::Bottom => {
                    cy > ay + ah - band(60.0)
                        && cx >= wx - band(20.0)
                        && cx <= wx + ww + band(20.0)
                }
                Edge::Left => {
                    cx < ax + band(60.0) && cy >= wy - band(20.0) && cy <= wy + wh + band(20.0)
                }
                Edge::Right => {
                    cx > ax + aw - band(60.0)
                        && cy >= wy - band(20.0)
                        && cy <= wy + wh + band(20.0)
                }
            };
            if !trigger {
                self.edge_armed = true; // 鼠标已离开触发区 → 允许下次弹回
            }
            if trigger && !pointer_down && self.edge_armed {
                self.restore_from_edge();
                self.pending_edge = Some(edge);
                self.mouse_out_since = None;
            }
            return;
            }
        }

        // 弹出后：鼠标离开窗口 500ms → 重新吸回（常量 LEAVE_DELAY）
        if let Some(edge) = self.pending_edge {
            // 窗口已经不在那条边上（用户拖走了）→ 作废，别按旧边吸
            let b2 = band(5.0);
            let still_at_edge = match edge {
                Edge::Top => wy <= ay + b2,
                Edge::Bottom => wy + wh >= ay + ah - b2,
                Edge::Left => wx <= ax + b2,
                Edge::Right => wx + ww >= ax + aw - b2,
            };
            if !still_at_edge {
                self.pending_edge = None;
                self.mouse_out_since = None;
            } else if in_win {
                self.mouse_out_since = None;
            } else {
                let started = *self.mouse_out_since.get_or_insert_with(Instant::now);
                if started.elapsed() >= Duration::from_millis(500) {
                    self.hide_to_edge(edge);
                    self.pending_edge = None;
                    self.mouse_out_since = None;
                }
            }
            return;
        }

        // 正常状态：窗口贴边（5px 内）持续 300ms → 吸附隐藏（常量 HIDE_DELAY）
        // 单侧判定：越过屏幕边也算贴住。
        // 前提是窗口尺寸已限制在工作区70%以内 —— 否则"底边永远成立"，
        // 窗口随便一放就被吸走（实测踩过，靠上面的尺寸上限解决）。
        // 单侧判定（越过屏幕边也算贴住），但在多条边同时满足时取"离屏幕边最近"的那条。
        // 只按固定顺序（上→下→左→右）会抢边：拖到左边时若底边也挂着，就被吸到底边（实测踩过）。
        let b = band(5.0);
        let mut cands: Vec<(Edge, i32)> = Vec::new();
        if wy <= ay + b {
            cands.push((Edge::Top, wy - ay));
        }
        if wy + wh >= ay + ah - b {
            cands.push((Edge::Bottom, (ay + ah) - (wy + wh)));
        }
        if wx <= ax + b {
            cands.push((Edge::Left, wx - ax));
        }
        if wx + ww >= ax + aw - b {
            cands.push((Edge::Right, (ax + aw) - (wx + ww)));
        }
        let near = cands
            .into_iter()
            .min_by_key(|(_, m)| m.abs())
            .map(|(e, _)| e);
        // 自吸前置条件：用户主动动过窗口 + 距显示已过1.5s 宽限期。
        // 少了这条，启动时存档位置正好贴边 → 打开300ms后窗口自己吸走（"打开就没了"）。
        // 必须已松手：拖拽进行中吸会和系统的窗口拖拽循环打架（实测窗口被拉回来乱跳）
        let fresh_move = self
            .user_moved_at
            .map(|t| t.elapsed() <= Duration::from_secs(2))
            .unwrap_or(false);
        let grace_ok = self
            .shown_at
            .map(|t| t.elapsed() >= Duration::from_millis(1500))
            .unwrap_or(true);
        match near {
            Some(edge) if !pointer_down && fresh_move && grace_ok => {
                let started = *self.near_edge_since.get_or_insert_with(Instant::now);
                if started.elapsed() >= Duration::from_millis(300) {
                    self.hide_to_edge(edge);
                }
            }
            _ => self.near_edge_since = None,
        }
    }

    /// 窗口位置/尺寸落盘
    fn persist_bounds(&mut self, ctx: &Context) {
        if !self.visible || self.edge.is_some() || self.hwnd == 0 {
            return;
        }
        // 最小化/屏幕外的坐标是垃圾值（-32000），写进去会让下次启动开在屏幕外
        if win32::is_minimized(self.hwnd) || win32::is_offscreen(self.hwnd) {
            return;
        }
        // 最大化时存的是放大的矩形，下次会直接开成最大化 → 跳过，保留上次普通尺寸
        if win32::is_zoomed(self.hwnd) {
            return;
        }
        let rect = win32::window_rect(self.hwnd);
        if rect.2 <= 0 || rect.3 <= 0 {
            return;
        }
        if self.last_bounds == Some(rect) {
            return;
        }
        self.last_bounds = Some(rect);
        let ppf = ctx.pixels_per_point().max(0.1);
        let size = ctx.viewport_rect().size();
        self.data.settings.window_bounds = Some(WindowBounds {
            x: Some(rect.0 as f64 / ppf as f64),
            y: Some(rect.1 as f64 / ppf as f64),
            width: Some(size.x as f64),
            height: Some(size.y as f64),
        });
        self.mark_save();
    }
}

// ---------------------------------------------------------------- 字体与样式

fn setup_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let load = |fonts: &mut FontDefinitions, path: &str, name: &str| -> bool {
        match std::fs::read(path) {
            Ok(bytes) => {
                fonts.font_data.insert(
                    name.to_string(),
                    std::sync::Arc::new(FontData::from_owned(bytes)),
                );
                true
            }
            Err(_) => false,
        }
    };
    let has_yahei = load(&mut fonts, "C:/Windows/Fonts/msyh.ttc", "yahei");
    let has_bold = load(&mut fonts, "C:/Windows/Fonts/msyhbd.ttc", "yahei-bold");
    // ☐☑✓ 等符号与彩色 emoji 由 Windows 自带字体补位（雅黑没有这些字形）
    load(&mut fonts, "C:/Windows/Fonts/seguisym.ttf", "segoe-symbol");
    load(&mut fonts, "C:/Windows/Fonts/seguiemj.ttf", "segoe-emoji");

    // 显示顺序：微软雅黑 → 彩色 emoji → 单色符号 → egui 自带字体兜底。
    // 顺序不能乱：以前靠往前面插，结果黑体/粗体抢在了雅黑前面。
    // emoji 必须排在符号字体前面：seguisym.ttf 里同样有 📌✏️🗑️🔍 这些码位，
    // 但它是单色字形，排前面会把彩色 emoji 全染成白色剪影（实测踩过）。
    let mut chain: Vec<String> = Vec::new();
    if has_yahei {
        chain.push("yahei".into());
    }
    chain.push("segoe-emoji".into());
    chain.push("segoe-symbol".into());
    if let Some(defaults) = fonts.families.get(&FontFamily::Proportional) {
        for f in defaults.clone() {
            if !chain.contains(&f) {
                chain.push(f);
            }
        }
    }
    fonts.families.insert(FontFamily::Proportional, chain.clone());
    fonts.families.insert(FontFamily::Monospace, chain);

    // 粗体族：egui 的 .strong() 只加深颜色、不换字体，粗体必须换字体族
    let mut bold: Vec<String> = Vec::new();
    if has_bold {
        bold.push("yahei-bold".into());
    }
    if has_yahei {
        bold.push("yahei".into());
    }
    bold.push("segoe-emoji".into());
    bold.push("segoe-symbol".into());
    fonts.families.insert(FontFamily::Name("yahei-bold".into()), bold);
    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &Context) {
    let mut style = (*ctx.style_of(Theme::Light)).clone();

    // 对话框标题栏（Window 标题走 TextStyle::Heading）→ 微软雅黑粗体
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(18.0, FontFamily::Name("yahei-bold".into())),
    );
    // 按钮/标签文字（新建、已完成、全部…）→ 微软雅黑粗体
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, FontFamily::Name("yahei-bold".into())),
    );

    // 所有文字强制暖棕色（覆盖 egui 默认的灰字）
    style.visuals.override_text_color = Some(C_TEXT);
    style.visuals.hyperlink_color = C_ORANGE;
    style.visuals.warn_fg_color = C_ORANGE;
    style.visuals.error_fg_color = C_RED;
    style.visuals.faint_bg_color = C_BG;
    style.visuals.extreme_bg_color = C_LIGHT;
    style.visuals.text_edit_bg_color = Some(C_LIGHT);
    style.visuals.code_bg_color = C_LIGHT;

    style.visuals.selection.bg_fill = C_PRIMARY;
    style.visuals.selection.stroke = Stroke::new(1.0, C_TEXT);
    style.visuals.window_corner_radius = CornerRadius::same(12);
    style.visuals.menu_corner_radius = CornerRadius::same(8);
    style.visuals.window_fill = C_BG;
    style.visuals.panel_fill = C_BG;
    style.visuals.window_stroke = Stroke::new(1.0, C_BORDER);

    // 控件三态全部换成暖色，egui 默认的灰底/灰字一律不要
    let v = &mut style.visuals.widgets;
    v.inactive.bg_fill = C_LIGHT;
    v.inactive.weak_bg_fill = C_LIGHT;
    v.inactive.bg_stroke = Stroke::new(1.0, C_BORDER);
    v.inactive.fg_stroke = Stroke::new(1.0, C_TEXT);
    v.hovered.bg_fill = C_ACCENT;
    v.hovered.weak_bg_fill = C_ACCENT;
    v.hovered.bg_stroke = Stroke::new(1.0, C_BORDER);
    v.hovered.fg_stroke = Stroke::new(1.0, C_TEXT);
    v.active.bg_fill = C_PRIMARY;
    v.active.weak_bg_fill = C_PRIMARY;
    v.active.bg_stroke = Stroke::new(1.0, C_BORDER);
    v.active.fg_stroke = Stroke::new(1.0, C_TEXT);
    v.noninteractive.bg_fill = C_BG;
    v.noninteractive.weak_bg_fill = C_BG;
    v.noninteractive.bg_stroke = Stroke::new(1.0, C_BORDER);
    v.noninteractive.fg_stroke = Stroke::new(1.0, C_TEXT2);
    // 对话框标题栏走 widgets.open.weak_bg_fill（egui 默认 #DCDCDC 灰，实测踩过）
    v.open.bg_fill = C_ACCENT;
    v.open.weak_bg_fill = C_ACCENT;
    v.open.bg_stroke = Stroke::new(1.0, C_BORDER);
    v.open.fg_stroke = Stroke::new(1.5, C_TEXT);

    style.spacing.item_spacing = vec2(8.0, 6.0);
    // 明暗两套主题都用便签黄，避免跟随系统深色模式
    ctx.set_style_of(Theme::Light, style.clone());
    ctx.set_style_of(Theme::Dark, style);
}

// ---------------------------------------------------------------- 主入口

/// release 构建是 windows 子系统、天生没有控制台；
/// 挂到父进程控制台后终端才能看到输出（stdout 被重定向/管道时本来就通，不受影响）
fn attach_parent_console() {
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(0xFFFF_FFFF); // ATTACH_PARENT_PROCESS
    }
}

fn main() -> eframe::Result {
    // 命令行 / AI 接口必须排在单实例锁之前：GUI 正在运行时，
    // `exe list` 也要能读写数据，而不是把窗口唤出来就退出
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(first) = args.first() {
        if first == "mcp" {
            // MCP stdio 服务：stdout 只走 JSON-RPC，不附着控制台、不打印别的
            mcp::serve();
            return Ok(());
        }
        attach_parent_console();
        let code = cli::run(&args);
        std::process::exit(code);
    }

    // 单实例锁：窗口可能正吸附隐藏着，第二次双击应该把它唤出来，
    // 而不是再开一个进程（两个进程会抢同一个数据文件）。
    let mutex_name: Vec<u16> = "Local\\桌面便利贴-单实例"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let handle = windows_sys::Win32::System::Threading::CreateMutexW(
            std::ptr::null_mut(),
            0,
            mutex_name.as_ptr(),
        );
        let already =
            windows_sys::Win32::Foundation::GetLastError() == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS;
        if already {
            win32::reveal_window(APP_NAME);
            if !handle.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
            return Ok(());
        }
        // 句柄保持到进程结束（进程退出内核自动释放），这里故意不 Close
        let _ = handle;
    }

    let data = load_data(&data_path());
    let mut viewport = ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_decorations(false)
        .with_inner_size([380.0, 600.0])
        .with_min_inner_size([320.0, 400.0])
        // 关掉系统 resize 样式：否则拖到顶边会先被 Windows 最大化，
        // 我们自己的贴边吸附就失效了（实测：窗口被撑到1933x1165还弹不回来）
        .with_resizable(false)
        .with_has_shadow(true);
    // 窗口/任务栏图标：不设置的话 winit 会用通用的 IDI_APPLICATION（实测就是"不好看"的来源）
    if let Ok(img) = image::load_from_memory(APP_ICON) {
        let rgba = img.to_rgba8();
        viewport = viewport.with_icon(egui::IconData {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        });
    }
    if data.settings.always_on_top.unwrap_or(true) {
        viewport = viewport.with_always_on_top();
    }
    if let Some(b) = &data.settings.window_bounds {
        if let (Some(x), Some(y)) = (b.x, b.y) {
            viewport = viewport.with_position([x as f32, y as f32]);
        }
        if let (Some(w), Some(h)) = (b.width, b.height) {
            if w >= 320.0 && h >= 400.0 {
                viewport = viewport.with_inner_size([w as f32, h as f32]);
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(Planner::new(cc)))),
    )
}

// ---------------------------------------------------------------- 界面

impl eframe::App for Planner {
    /// 非绘制逻辑：窗口句柄、托盘事件、快捷键、边缘吸附、落盘。
    /// 窗口隐藏时 eframe 仍会调用 `logic`（不跑绘制），托盘「显示」正是靠它唤醒。
    fn logic(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        if self.hwnd == 0 {
            self.hwnd = win32::hwnd_from_frame(frame);
        }
        // 首帧：如果恢复出来的位置在屏幕外（上次被最小化时写坏的坐标），
        // 夹回可见区域，否则窗口会开在看不见的地方。
        if !self.position_checked && self.hwnd != 0 {
            self.position_checked = true;
            // 先退出最大化（存档里可能存了最大化矩形）
            if win32::is_zoomed(self.hwnd) {
                win32::unmaximize(self.hwnd);
            }
            let (x, y, w, h) = win32::window_rect(self.hwnd);
            // 尺寸夹进工作区：窗口一旦比工作区还大，"贴边"判断会永远成立（底边/顶边），
            // 表现为窗口自己乱跳（实测：尺寸撑到1920x1152 后随机吸附）
            let (_, _, aw, ah) = win32::work_area(self.hwnd);
            // 便签不该占满全屏：上限取工作区70%，否则四边距全是0，贴边判断会失真
            let w2 = w.min((aw * 7 / 10).max(320)).max(320);
            let h2 = h.min((ah * 7 / 10).max(400)).max(400);
            if (w2, h2) != (w, h) {
                win32::set_window_rect(self.hwnd, x, y, w2, h2);
            }
            let (nx, ny) = win32::clamp_to_visible(x, y, w2, h2);
            if (nx, ny) != (x, y) {
                win32::move_window(self.hwnd, nx, ny);
            }
            // 启动时的归位不是用户拖的，清掉标记，避免一开就在贴边位置被吸走
            self.user_moved_at = None;
        }

        self.handle_resize(ctx);
        self.poll_tray(ctx);
        self.handle_close_request(ctx);
        self.handle_shortcuts(ctx);
        self.apply_search_debounce();
        self.apply_pending_drop();
        if self.resize.is_none() {
            self.tick_edge(ctx);
        }
        self.persist_bounds(ctx);
        self.flush_save();
        self.poll_external_data_change();

        // 100ms 轮询：托盘事件、边缘吸附、紧急项闪烁（100ms interval）
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // eframe 给的 Ui 没有背景色，不自己铺一层就是黑的（实测踩过）
        ui.painter()
            .rect_filled(ui.max_rect(), CornerRadius::ZERO, C_BG);
        self.titlebar(ui, &ctx);
        self.toolbar(ui, &ctx);
        self.tabs(ui, &ctx);
        self.list(ui, &ctx);
        self.edit_modal(&ctx);
        self.confirm_modal(&ctx);
        self.about_modal(&ctx);
    }
}

impl Planner {
    /// 命中边框？返回位掩码 1=左 2=右 4=上 8=下（无边框窗口默认没有缩放区，要自己做）
    fn hit_edge(&self, ctx: &Context) -> Option<u8> {
        let rect = ctx.content_rect();
        let p = ctx.input(|i| i.pointer.hover_pos())?;
        let mut e = 0u8;
        // 左右边：避开标题栏按钮区（顶起 40pt 以内不命中）
        if p.x - rect.left() <= BORDER && p.y >= rect.top() + 40.0 {
            e |= 1;
        }
        if rect.right() - p.x <= BORDER && p.y >= rect.top() + 40.0 {
            e |= 2;
        }
        // 底边（含两个下角）
        if rect.bottom() - p.y <= BORDER {
            e |= 8;
        }
        // 顶边：只在标题栏按钮之间生效，别和关闭/最小化抢点击
        if p.y - rect.top() <= BORDER
            && p.x >= rect.left() + 90.0
            && p.x <= rect.right() - 140.0
        {
            e |= 4;
        }
        if e == 0 {
            None
        } else {
            Some(e)
        }
    }

    /// 边框缩放：按下开始，拖动时 SetWindowPos 实时改大小（内容会跟着重绘），松手结束
    fn handle_resize(&mut self, ctx: &Context) {
        if self.hwnd == 0 {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let pos = ctx.input(|i| i.pointer.latest_pos());
        let down = ctx.input(|i| i.pointer.any_down());
        let pressed = ctx.input(|i| i.pointer.any_pressed());

        if let Some(rs) = self.resize.take() {
            match (pos, down) {
                (Some(p), true) => {
                    let dx = ((p.x - rs.start.x) * ppp).round() as i32;
                    let dy = ((p.y - rs.start.y) * ppp).round() as i32;
                    let (sx, sy, sw, sh) = rs.rect;
                    let min_w = (320.0 * ppp).round() as i32;
                    let min_h = (400.0 * ppp).round() as i32;
                    let (_, _, aw, ah) = win32::work_area(self.hwnd);
                    let max_w = (aw * 7 / 10).max(min_w);
                    let max_h = (ah * 7 / 10).max(min_h);
                    let (mut x, mut y, mut w, mut h) = (sx, sy, sw, sh);
                    if rs.edge & 1 != 0 {
                        w = (sw - dx).clamp(min_w, max_w);
                        x = sx + (sw - w); // 左边缩放要同步挪 x，否则窗口会跳
                    }
                    if rs.edge & 2 != 0 {
                        w = (sw + dx).clamp(min_w, max_w);
                    }
                    if rs.edge & 4 != 0 {
                        h = (sh - dy).clamp(min_h, max_h);
                        y = sy + (sh - h);
                    }
                    if rs.edge & 8 != 0 {
                        h = (sh + dy).clamp(min_h, max_h);
                    }
                    win32::set_window_rect(self.hwnd, x, y, w, h);
                    self.user_moved_at = Some(Instant::now());
                    self.resize = Some(rs);
                    // 拖拽中也给正确光标（指针可能已经离开边框）
                    let icon = Self::resize_cursor(rs.edge);
                    ctx.set_cursor_icon(icon);
                }
                _ => {
                    win32::release_mouse(); // 松手 → 结束
                    self.user_moved_at = Some(Instant::now());
                }
            }
            return;
        }

        if let Some(e) = self.hit_edge(ctx) {
            ctx.set_cursor_icon(Self::resize_cursor(e));
            if pressed {
                if let Some(p) = pos {
                    self.resize = Some(ResizeDrag {
                        edge: e,
                        start: p,
                        rect: win32::window_rect(self.hwnd),
                    });
                    win32::capture_mouse(self.hwnd);
                }
            }
        }
    }

    /// 位掩码 → 光标方向
    fn resize_cursor(edge: u8) -> CursorIcon {
        match edge {
            1 | 2 => CursorIcon::ResizeHorizontal,
            4 | 8 => CursorIcon::ResizeVertical,
            5 => CursorIcon::ResizeNwSe,  // 左上
            6 => CursorIcon::ResizeNeSw,  // 右上
            9 => CursorIcon::ResizeNeSw,  // 左下
            10 => CursorIcon::ResizeNwSe, // 右下
            _ => CursorIcon::Default,
        }
    }

    fn titlebar(&mut self, ui: &mut Ui, ctx: &Context) {
        let width = ui.available_width();
        // 必须 click_and_drag：只有 Sense::click 时 drag_started 永远不触发，
        // 窗口就拖不动（实测踩过）
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click_and_drag());
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::ZERO, C_ACCENT);
        p.line_segment(
            [
                pos2(rect.left(), rect.bottom()),
                pos2(rect.right(), rect.bottom()),
            ],
            Stroke::new(1.0, C_BORDER),
        );

        // 图钉用白色作顶点色 = 彩色 emoji 不被着色，显示原生红色；再放大一档
        p.text(
            pos2(rect.left() + 7.0, rect.center().y),
            Align2::LEFT_CENTER,
            "📌",
            FontId::proportional(18.0),
            Color32::WHITE,
        );
        p.text(
            pos2(rect.left() + 34.0, rect.center().y),
            Align2::LEFT_CENTER,
            APP_NAME,
            FontId::new(15.0, FontFamily::Name("yahei-bold".into())),
            C_TEXT,
        );

        if resp.drag_started() && self.resize.is_none() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }

        let buttons: [(&str, &str); 4] = [("pin", "📌"), ("min", "─"), ("max", "□"), ("close", "×")];
        let mut x = rect.right() - 4.0 - 32.0 * buttons.len() as f32;
        for (kind, label) in buttons {
            let br = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(32.0, 28.0));
            let r = ui.interact(br, Id::unique(("tb", kind)), Sense::click());
            // 彩色 emoji 不接受顶点着色，置顶开关的"开启"状态改用淡红底表达
            let active = kind == "pin" && self.always_on_top;
            if r.hovered() || active {
                let fill = if kind == "close" {
                    C_RED
                } else if active {
                    Color32::from_rgba_unmultiplied(0xD3, 0x48, 0x3E, 70)
                } else {
                    C_ACCENT.gamma_multiply(0.85)
                };
                p.rect_filled(br, CornerRadius::same(4), fill);
            }
            let color = if kind == "close" && r.hovered() {
                Color32::WHITE
            } else if kind == "pin" {
                if self.always_on_top {
                    C_RED
                } else if r.hovered() {
                    C_TEXT
                } else {
                    C_TEXT2
                }
            } else {
                C_TEXT
            };
            p.text(
                br.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(if kind == "pin" { 14.0 } else { 16.0 }),
                color,
            );
            if r.clicked() {
                match kind {
                    "pin" => self.toggle_always_on_top(ctx),
                    "min" => {
                        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                    }
                    "max" => self.toggle_maximize(ctx),
                    _ => self.hide_window(ctx),
                }
            }
            x += 32.0;
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, _ctx: &Context) {
        Frame::NONE
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let new_btn = Button::new(RichText::new("＋ 新建").strong())
                        .fill(C_PRIMARY)
                        .min_size(vec2(0.0, 30.0));
                    if ui.add(new_btn).clicked() {
                        self.open_edit(None);
                    }

                    let mut done_btn = Button::new("✓ 已完成").min_size(vec2(0.0, 30.0));
                    if self.show_completed {
                        done_btn = done_btn.fill(C_PRIMARY);
                    }
                    if ui.add(done_btn).clicked() {
                        self.show_completed = !self.show_completed;
                    }

                    // 图标单独画、与文字一起垂直居中（emoji 混进 hint 会把行盒撑歪）
                    let sw = ui.available_width().max(60.0);
                    let (srect, _) = ui.allocate_exact_size(vec2(sw, 30.0), Sense::hover());
                    let sp = ui.painter();
                    sp.rect_filled(srect, CornerRadius::same(6), C_LIGHT);
                    sp.rect_stroke(srect, CornerRadius::same(6), Stroke::new(1.0, C_BORDER), StrokeKind::Middle);
                    sp.text(
                        pos2(srect.left() + 8.0, srect.center().y),
                        Align2::LEFT_CENTER,
                        "🔍",
                        FontId::proportional(16.0),
                        Color32::WHITE,
                    );
                    let row_h =
                        ui.fonts_mut(|f| f.row_height(&TextStyle::Button.resolve(ui.style())));
                    let trect = Rect::from_min_size(
                        pos2(srect.left() + 28.0, srect.center().y - row_h * 0.5),
                        vec2((srect.width() - 36.0).max(30.0), row_h),
                    );
                    let resp = ui.put(
                        trect,
                        TextEdit::singleline(&mut self.search)
                            .hint_text("搜索计划...")
                            .frame(Frame::NONE),
                    );
                    if resp.changed() {
                        self.search_edited = Some(Instant::now());
                    }
                    if self.focus_search {
                        resp.request_focus();
                        self.focus_search = false;
                    }
                });
            });
    }

    fn tabs(&mut self, ui: &mut Ui, _ctx: &Context) {
        Frame::NONE
            .inner_margin(Margin::symmetric(0, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // 全选框：勾选 = 自动全选当前列表；再点一次 = 取消全选并退出多选
                    ui.add_space(9.0);
                    let visible_ids: Vec<String> = self
                        .visible_items()
                        .iter()
                        .map(|i| i.id.clone())
                        .collect();
                    let all_selected = !visible_ids.is_empty()
                        && visible_ids.iter().all(|id| self.selected.contains(id));
                    let on = self.select_mode && all_selected;
                    let (chk, chk_resp) =
                        ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(
                        chk,
                        CornerRadius::same(4),
                        if on {
                            C_PRIMARY
                        } else if chk_resp.hovered() {
                            Color32::from_rgba_unmultiplied(0xFF, 0xC1, 0x07, 40)
                        } else {
                            C_LIGHT
                        },
                    );
                    draw_rect_border(
                        p,
                        chk,
                        CornerRadius::same(4),
                        if on {
                            C_PRIMARY
                        } else if chk_resp.hovered() {
                            Y_700
                        } else {
                            C_BORDER
                        },
                    );
                    if on {
                        p.text(
                            chk.center(),
                            Align2::CENTER_CENTER,
                            "✓",
                            FontId::proportional(13.0),
                            Color32::WHITE,
                        );
                    }
                    if chk_resp.clicked() {
                        if on {
                            self.selected.clear();
                            self.select_mode = false;
                        } else {
                            self.select_mode = true;
                            self.selected.extend(visible_ids.iter().cloned());
                        }
                    }
                    ui.add_space(7.0);
                    let items: [(Filter, &str); 5] = [
                        (Filter::All, "全部"),
                        (Filter::Pinned, "置顶"),
                        (Filter::Today, "今日"),
                        (Filter::Week, "本周"),
                        (Filter::Expired, "已过期"),
                    ];
                    for (f, label) in items {
                        let selected = self.filter == f;
                        if ui.selectable_label(selected, label).clicked() {
                            self.filter = f;
                        }
                    }
                    // 同排最右：有选中项时出现删除按钮
                    if !self.selected.is_empty() {
                        let bw = 56.0;
                        let rest = ui.available_width();
                        if rest > bw + 8.0 {
                            ui.add_space(rest - bw - 4.0);
                        }
                        let (del_rect, del_resp) =
                            ui.allocate_exact_size(vec2(bw, 20.0), Sense::click());
                        let p = ui.painter();
                        p.rect_filled(
                            del_rect,
                            CornerRadius::same(4),
                            if del_resp.hovered() { R_600 } else { C_RED },
                        );
                        p.text(
                            del_rect.center(),
                            Align2::CENTER_CENTER,
                            "删除",
                            FontId::new(13.0, FontFamily::Name("yahei-bold".into())),
                            Color32::WHITE,
                        );
                        if del_resp.clicked() {
                            self.confirm_ids = self.selected.iter().cloned().collect();
                            self.modal = Modal::Confirm;
                        }
                    }
                });
            });
    }

    fn list(&mut self, ui: &mut Ui, ctx: &Context) {
        let visible = self.visible_items();
        self.item_rects.clear();

        if visible.is_empty() {
            let text = if !self.search_query.is_empty() {
                "没有找到匹配的计划"
            } else if !self.show_completed {
                "所有计划都已完成"
            } else if self.filter != Filter::All {
                "没有符合条件的计划"
            } else {
                "暂无计划"
            };
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("📝").size(44.0).color(C_TEXT2));
                ui.add_space(8.0);
                ui.label(RichText::new(text).size(14.0).color(C_TEXT2));
            });
            return;
        }

        ui.spacing_mut().item_spacing.y = 6.0;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // 顶部留白：标签行与首张卡片之间的呼吸间距
                ui.add_space(12.0);
                for item in &visible {
                    self.item_card(ui, ctx, item);
                }
            });
        // 拖拽反馈：指针下的目标卡片描红边，让"拖到哪"看得见
        if self.drop_id.is_some() {
            if let Some(pt) = ctx.input(|i| i.pointer.latest_pos()) {
                if let Some((_, r)) = self.item_rects.iter().find(|(_, r)| r.contains(pt)) {
                    ui.painter().rect_stroke(
                        *r,
                        CornerRadius::same(8),
                        Stroke::new(2.5, C_RED),
                        StrokeKind::Middle,
                    );
                }
            }
        }
    }

    fn toggle_select(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_string());
        }
    }

    fn item_card(&mut self, ui: &mut Ui, ctx: &Context, item: &Item) {
        let status = time_status(&item.deadline);
        let mut border = C_ITEM_BORDER;
        match status {
            Status::Expired | Status::Urgent => border = C_RED,
            Status::Warning => border = C_ORANGE,
            _ => {}
        }

        let width = ui.available_width();
        let has_deadline = item.deadline.is_some();
        let has_content = !item.content.is_empty();
        let mut height = 7.0 + 18.0 + 3.0;
        if has_deadline {
            height += 14.0;
        }
        if has_content {
            height += 32.0;
        }
        height += 7.0;

        let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        self.item_rects.push((item.id.clone(), rect));

        if resp.double_clicked() && !self.select_mode {
            self.open_edit(Some(item.id.clone()));
        }

        let p = ui.painter();
        // 紧急项闪烁（urgentPulse）
        let pinned_bg = item.is_pinned && !item.is_completed;
        let base = if pinned_bg { C_BG_PIN } else { C_CARD };
        let fill = if item.is_completed {
            C_BG_DONE
        } else if status == Status::Urgent {
            let phase = (ctx.input(|i| i.time) * 2.0) as i64 % 2;
            if phase == 0 {
                base
            } else {
                Color32::from_rgb(0xFF, 0xE0, 0x82)
            }
        } else {
            base
        };
        p.rect_filled(rect, CornerRadius::same(8), fill);
        // 卡片描边：与窗口底拉开一档，列表才有层次
        p.rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.0, if pinned_bg { C_BORDER } else { C_CARD_BORDER }),
            StrokeKind::Middle,
        );
        // 左侧状态条（置顶区用更深的橙，和深色底配套）
        let bar = if pinned_bg && matches!(status, Status::Normal | Status::Warning) {
            C_ORANGE
        } else {
            border
        };
        p.rect_filled(
            Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
            CornerRadius::same(3),
            bar,
        );

        let x = rect.left() + 9.0;
        let y_top = rect.top() + 7.0;

        // 勾选框
        let chk = Rect::from_min_size(pos2(x, y_top), vec2(18.0, 18.0));
        let chk_resp = ui.interact(chk, Id::unique(("chk", item.id.clone())), Sense::click());
        let sel_on = self.select_mode && self.selected.contains(&item.id);
        let show_check = if self.select_mode {
            sel_on
        } else {
            item.is_completed
        };
        p.rect_filled(
            chk,
            CornerRadius::same(4),
            if show_check {
                if self.select_mode {
                    C_PRIMARY
                } else {
                    C_GREEN
                }
            } else if chk_resp.hovered() {
                Color32::from_rgba_unmultiplied(0xFF, 0xC1, 0x07, 40)
            } else {
                C_LIGHT
            },
        );
        draw_rect_border(
            p,
            chk,
            CornerRadius::same(4),
            if sel_on {
                C_PRIMARY
            } else if chk_resp.hovered() {
                Y_700
            } else {
                border
            },
        );
        if show_check {
            p.text(
                chk.center(),
                Align2::CENTER_CENTER,
                "✓",
                FontId::proportional(13.0),
                Color32::WHITE,
            );
        }
        if chk_resp.clicked() {
            let id = item.id.clone();
            if self.select_mode {
                self.toggle_select(&id);
            } else {
                self.toggle_complete(&id);
            }
            return;
        }
        // 多选模式：点卡片任意位置 = 切换选中
        if self.select_mode {
            if resp.clicked() {
                let id = item.id.clone();
                self.toggle_select(&id);
            }
        }

        // 右侧操作区（从右往左排）
        let mut rx = rect.right() - 7.0;
        let step = |rx: &mut f32, w: f32| -> f32 {
            *rx -= w;
            *rx
        };

        if !self.select_mode {
        let del_rect = Rect::from_min_size(pos2(step(&mut rx, 24.0), y_top), vec2(24.0, 18.0));
        let del_resp = ui.interact(del_rect, Id::unique(("del", item.id.clone())), Sense::click());
        if del_resp.hovered() {
            p.rect_filled(del_rect, CornerRadius::same(4), R_600);
        }
        p.text(
            del_resp.rect.center(),
            Align2::CENTER_CENTER,
            "🗑️",
            FontId::new(17.0, FontFamily::Name("yahei-bold".into())),
            Color32::WHITE,
        );
        if del_resp.clicked() {
            self.confirm_ids = vec![item.id.clone()];
            self.modal = Modal::Confirm;
            return;
        }

        let edit_rect = Rect::from_min_size(pos2(step(&mut rx, 24.0), y_top), vec2(24.0, 18.0));
        let edit_resp = ui.interact(edit_rect, Id::unique(("edit", item.id.clone())), Sense::click());
        p.text(
            edit_resp.rect.center(),
            Align2::CENTER_CENTER,
            "✏️",
            FontId::new(17.0, FontFamily::Name("yahei-bold".into())),
            Color32::WHITE,
        );
        if edit_resp.clicked() {
            self.open_edit(Some(item.id.clone()));
            return;
        }

        let pin_rect = Rect::from_min_size(pos2(step(&mut rx, 24.0), y_top), vec2(24.0, 18.0));
        let pin_resp = ui.interact(pin_rect, Id::unique(("pin", item.id.clone())), Sense::click());
        p.text(
            pin_resp.rect.center(),
            Align2::CENTER_CENTER,
            if item.is_pinned { "📌" } else { "📍" },
            FontId::new(17.0, FontFamily::Name("yahei-bold".into())),
            Color32::WHITE,
        );
        if pin_resp.clicked() {
            let id = item.id.clone();
            self.toggle_pin_item(&id);
            return;
        }
        } // if !select_mode

        // 进度百分比：13px 粗体，与标题行同一条中线（y_top+9）垂直居中
        let prog = format!("{}%", item.progress);
        let prog_galley =
            p.layout_no_wrap(prog, FontId::new(13.0, FontFamily::Name("yahei-bold".into())), C_TEXT2);
        let prog_w = prog_galley.size().x + 8.0;
        let prog_x = step(&mut rx, prog_w);
        p.galley(
            pos2(prog_x + 4.0, y_top + 9.0 - prog_galley.size().y * 0.5),
            prog_galley,
            C_TEXT2,
        );

        // 拖拽手柄
        if !self.select_mode {
            let handle_rect =
                Rect::from_min_size(pos2(step(&mut rx, 20.0), y_top), vec2(20.0, 18.0));
            let handle_resp = ui.interact(
                handle_rect,
                Id::unique(("drag", item.id.clone())),
                Sense::click_and_drag(),
            );
            // 通用拖拽把手：三道圆角横杠（自绘，比字符/emoji 符号更通用耐看）
            {
                let hc = handle_resp.rect.center();
                let col = if handle_resp.dragged() || handle_resp.hovered() {
                    C_RED
                } else {
                    C_TEXT2
                };
                let (bw, bh) = (13.0f32, 2.2f32);
                for i in -1..=1i32 {
                    let top = hc.y + i as f32 * 5.0 - bh * 0.5;
                    p.rect_filled(
                        Rect::from_min_size(pos2(hc.x - bw * 0.5, top), vec2(bw, bh)),
                        CornerRadius::same(1),
                        col,
                    );
                }
            }
            if handle_resp.drag_started() {
                self.drop_id = Some(item.id.clone());
            }
            if handle_resp.drag_stopped() {
                self.drop_pt = ctx.input(|i| i.pointer.latest_pos());
            }
        }

        let text_left = x + 26.0;
        let text_right = rx - 6.0;
        let available = (text_right - text_left).max(40.0);

        // 标题
        let title_budget = (available / 14.0).max(4.0) as usize;
        let title = truncate_chars(&item.title, title_budget);
        let title_color = if matches!(status, Status::Expired | Status::Urgent) {
            C_RED
        } else {
            C_TEXT
        };
        let mut y = y_top + 9.0;
        paint_runs(
            p,
            text_left,
            y,
            &title,
            &self.search_query,
            FontId::new(14.0, FontFamily::Name("yahei-bold".into())),
            title_color,
        );

        // 截止时间
        if has_deadline {
            y += 13.0;
            let color = match status {
                Status::Expired | Status::Urgent => C_RED,
                Status::Warning => C_ORANGE,
                _ => C_TEXT2,
            };
            let label = format!("完成日期：{}", format_deadline_short(&item.deadline));
            p.text(
                pos2(text_left, y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                color,
            );
        }

        // 内容（最多两行）
        if has_content {
            y += 15.0;
            let per_line = (available / 12.0).max(4.0) as usize;
            let shown = truncate_chars(&item.content, per_line * 2);
            let lines = wrap_by_width(&shown, per_line);
            for (i, line) in lines.iter().take(2).enumerate() {
                paint_runs(
                    p,
                    text_left,
                    y + i as f32 * 15.0,
                    line,
                    &self.search_query,
                    FontId::proportional(12.0),
                    C_TEXT2,
                );
            }
        }

    }

    fn edit_modal(&mut self, ctx: &Context) {
        if self.modal != Modal::Edit {
            return;
        }
        let mut open = true;
        let title = if self.editing.is_some() {
            "编辑计划"
        } else {
            "新建计划"
        };
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .default_width(340.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(RichText::new("标题 *").size(12.0).color(C_TEXT2));
                ui.add(
                    TextEdit::singleline(&mut self.f_title)
                        .hint_text("请输入计划标题")
                        .desired_width(f32::INFINITY),
                );
                if let Some(err) = &self.form_error {
                    ui.colored_label(C_RED, err.clone());
                }

                ui.add_space(8.0);
                ui.label(RichText::new("内容").size(12.0).color(C_TEXT2));
                ui.add(
                    TextEdit::multiline(&mut self.f_content)
                        .hint_text("请输入计划内容")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(8.0);
                ui.label(RichText::new("进度").size(12.0).color(C_TEXT2));
                ui.add(Slider::new(&mut self.f_progress, 0..=100).suffix("%"));

                ui.add_space(8.0);
                ui.label(RichText::new("完成时间").size(12.0).color(C_TEXT2));
                // 时间用日历+时分选择，不再让用户敲文本
                self.deadline_picker(ui);

                ui.add_space(6.0);
                ui.checkbox(&mut self.f_pinned, "置顶此计划");

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("取消").clicked() {
                        cancel_clicked = true;
                    }
                    if ui.add(Button::new("保存").fill(C_PRIMARY)).clicked() {
                        save_clicked = true;
                    }
                });
            });

        if save_clicked {
            self.save_item();
        } else if cancel_clicked || !open {
            self.modal = Modal::None;
            self.form_error = None;
        }
    }

    /// 关于：版本 / 开发语言 / 作者
    fn about_modal(&mut self, ctx: &Context) {
        if self.modal != Modal::About {
            return;
        }
        let mut open = true;
        Window::new("关于")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .default_width(300.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new(APP_NAME).size(20.0).strong().color(C_TEXT));
                    ui.add_space(10.0);

                    let exe = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
                        .unwrap_or_default();
                    let rows: [(&str, String); 4] = [
                        ("版本", VERSION.to_string()),
                        ("开发语言", "Rust（界面 eframe/egui）".to_string()),
                        ("作者", AUTHOR.to_string()),
                        ("主程序", exe),
                    ];
                    for (k, v) in rows {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{k}：")).size(13.0).color(C_TEXT2));
                            ui.label(RichText::new(v).size(13.0).color(C_TEXT));
                        });
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("数据文件与主程序同目录、同名 .json")
                            .size(11.0)
                            .color(C_TEXT2),
                    );
                    ui.add_space(10.0);
                    // 这里不能读 `open`：它正被 .open(&mut open) 借用着（编译器会报 E0502），
                    // 关闭判断放在 .show() 之后
                    if ui.add(Button::new("关闭").fill(C_PRIMARY)).clicked() {
                        self.modal = Modal::None;
                    }
                    ui.add_space(4.0);
                });
            });
        if !open {
            self.modal = Modal::None;
        }
    }

    fn confirm_modal(&mut self, ctx: &Context) {
        if self.modal != Modal::Confirm {
            return;
        }
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;

        Window::new("确认删除")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .default_width(280.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    let n = self.confirm_ids.len();
                    if n <= 1 {
                        ui.label("确定要删除这个计划吗？");
                    } else {
                        ui.label(format!("确定要删除选中的 {n} 个计划吗？"));
                    }
                    ui.label("此操作无法撤销。");
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                        if ui.add(Button::new("删除").fill(C_RED)).clicked() {
                            confirm = true;
                        }
                    });
                });
            });

        if confirm {
            // 先把待删 id 取出来，否则 drain 会一直占着 &mut self
            let ids: Vec<String> = std::mem::take(&mut self.confirm_ids);
            for id in &ids {
                self.delete_item(id);
            }
            self.selected.clear();
            self.modal = Modal::None;
        } else if cancel || !open {
            self.confirm_ids.clear();
            self.modal = Modal::None;
        }
    }
}

// ---------------------------------------------------------------- 绘制小工具

fn draw_rect_border(p: &Painter, rect: Rect, _radius: CornerRadius, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    p.line_segment([pos2(l, t), pos2(r, t)], stroke);
    p.line_segment([pos2(r, t), pos2(r, b)], stroke);
    p.line_segment([pos2(r, b), pos2(l, b)], stroke);
    p.line_segment([pos2(l, b), pos2(l, t)], stroke);
}
