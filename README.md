# 桌面便利贴 · Desktop Sticky Notes

> 语言 / Language：**中文** ｜ [English](README.en.md)

![平台](https://img.shields.io/badge/platform-Windows%2010%2B-blue)
![语言](https://img.shields.io/badge/Rust-1.70+-orange)
![体积](https://img.shields.io/badge/size-5%20MB-brightgreen)
![许可证](https://img.shields.io/badge/license-MIT-green)

**单文件、免安装、零依赖**的 Windows 桌面计划便签工具。
一个 `.exe` 双击即用 —— 不需要 VC++ 运行库，不需要安装，不需要联网。

![主界面](docs/screenshot-main.png)

## 功能特性

| 分类 | 说明 |
| --- | --- |
| 🪟 窗口 | 无边框便签样式，可拖动、可边框缩放（320×400 ~ 工作区70%），可置顶 |
| 📌 贴边吸附 | 贴屏幕四边 300ms 自动隐藏，鼠标靠近对应边缘即弹出；启动宽限 + 松手才吸，不会"打开就没了" |
| ✅ 计划管理 | 新建 / 编辑 / 删除 / 置顶 / 完成勾选 / 进度条 / 拖拽排序 |
| 📅 时间选择 | 日历选日期 + 时/分下拉 + 「现在」快捷键，不再手敲 `2026-11-05T09:00` |
| 🔍 筛选搜索 | 全部 / 置顶 / 今日 / 本周 / 已过期 + 关键字搜索（300ms 防抖） |
| ☑️ 多选批量 | 筛选行进入多选：全选 / 反选 / 批量删除（带二次确认） |
| 🗂 托盘 | 关闭按钮最小化到托盘；右键菜单：显示窗口 / 新建计划 / 关于 / 退出 |
| 🚀 自启与单实例 | 可选开机自启；重复双击 exe 只会唤出已开窗口，不会开第二个进程 |
| 💾 便携数据 | 数据文件与主程序**同目录、同名** `.json`，跟着 U 盘走 |
| 🔄 原版兼容 | 字段结构与 Electron 原版一致，首次运行自动迁移 `%APPDATA%\desktop-sticky-planner\data.json` |

## 下载安装

1. 打开 [Releases](../../releases) 页面，下载 `桌面便利贴.exe`
2. 放到任意目录（推荐桌面或 `Desktop\AGENT\`）
3. **双击即用** —— Win10 / Win11 无需安装任何运行库

> 绿色软件：卸载 = 删掉 exe（数据文件 `桌面便利贴.json` 与它同目录，可一并备份）。

## 快捷键

| 按键 | 作用 |
| --- | --- |
| `Ctrl + N` | 新建计划 |
| `Ctrl + F` | 聚焦搜索框 |
| `Ctrl + A` | 多选模式下全选当前列表 |
| `Esc` | 关闭当前弹窗 |
| 双击卡片 | 编辑该计划 |
| 拖动标题栏 | 移动窗口（贴边后可吸附隐藏） |

## 数据文件

- 位置：**与主程序同目录、同名**，例如 `桌面便利贴.exe` → `桌面便利贴.json`
- 格式：UTF-8 JSON，字段与原版一致（`items[].title / content / deadline / progress / isPinned / isCompleted / createdAt / updatedAt`）
- 迁移：首次运行若新位置没有文件，会自动复制原版 `%APPDATA%\desktop-sticky-planner\data.json`
- 备份：直接复制该 `.json` 即可

```json
{
  "items": [
    {
      "id": "...",
      "title": "提交季度培训统计报表",
      "content": "汇总各单位培训人次、学时与完成率",
      "deadline": "2026-11-05T09:00",
      "progress": 100,
      "isPinned": false,
      "isCompleted": true,
      "createdAt": 1793800000000,
      "updatedAt": 1793800000000
    }
  ],
  "settings": { "windowBounds": { "x": 400, "y": 100, "width": 380, "height": 600 }, "alwaysOnTop": false }
}
```

## 从源码构建

依赖：[Rust 工具链](https://rustup.rs/)（Windows GNU 目标，`x86_64-pc-windows-gnu`，自带 MinGW 链接器即可）。

```bash
cargo build --release
```

产物路径：

```
target/x86_64-pc-windows-gnu/release/desktop-sticky-planner.exe
```

复制后改名为 `桌面便利贴.exe` 即可（程序按**自身文件名**定位数据文件，改名不影响运行）。

> 提示：本仓库不含 `.cargo/config.toml`（那是本机 target-dir 配置）。如需自定义产物目录，自行创建 `.cargo/config.toml`。

### 项目结构

```
桌面便利贴Rust/
├── src/
│   ├── main.rs      # 界面、计划逻辑、时间选择器、多选、贴边吸附状态机
│   ├── tray.rs      # 托盘图标与右键菜单（纯 Win32）
│   └── win32.rs     # HWND / 工作区 / 窗口移动缩放 / 单实例互斥量 / 开机自启
├── build.rs         # 嵌入 .ico 图标与版本资源（windres）
├── assets/
│   ├── app.ico          # 多尺寸图标（16~256）
│   ├── app_preview.png  # 图标预览
│   └── gen_icon.py      # 图标生成脚本（Pillow）
├── docs/            # README 截图
├── Cargo.toml
└── LICENSE / README.md / CHANGELOG.md
```

## 常见问题

**Q：纯净系统要装 VC++ 运行库吗？**
不用。导入表只引用 Windows 自带 DLL（`kernel32 / user32 / gdi32 / shell32 / advapi32 / uxtheme / dwmapi / OPENGL32 / api-ms-win-crt-*`），无 `VCRUNTIME`、无 `libgcc`。

**Q：窗口吸附隐藏后怎么叫回来？**
把鼠标移到屏幕对应边缘（隐藏处附近）停一下即可弹出；或托盘右键 →「显示窗口」；或再双击一次 exe。

**Q：怎么关闭开机自启？**
删除注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 中名为 `桌面便利贴` 的值即可（程序只在值缺失或指向失效文件时写入）。

**Q：能同时开两个吗？**
不能，这是刻意设计——单实例互斥，第二次双击会唤出已开窗口，避免两个进程抢写同一个数据文件。

## 更新日志

见 [CHANGELOG.md](CHANGELOG.md)。

## 许可证

[MIT](LICENSE) © 2026 **青山白马少年**

---

### English

A **single-file, zero-dependency** desktop sticky planner for Windows 10/11.
Download `桌面便利贴.exe` from [Releases](../../releases) and double-click — no VC++ runtime, no installer, no network required.

Highlights: borderless draggable/resizable window, edge-snapping auto-hide (4 edges, DPI-aware), calendar time picker, pin/progress/search/filters, multi-select batch delete, tray menu, optional autostart, single-instance lock, and a **portable JSON data file stored next to the executable** (auto-migrates from the original Electron version's `%APPDATA%\desktop-sticky-planner\data.json`).

Build: `cargo build --release` (Rust, `x86_64-pc-windows-gnu`). Full documentation in [中文](README.md). License: MIT © 青山白马少年.
