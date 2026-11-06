# Desktop Sticky Notes · 桌面便利贴

> Language: [中文](README.md) ｜ **English**

![Platform](https://img.shields.io/badge/platform-Windows%2010%2B-blue)
![Rust](https://img.shields.io/badge/Rust-1.70+-orange)
![Size](https://img.shields.io/badge/size-5%20MB-brightgreen)
![License](https://img.shields.io/badge/license-MIT-green)

A **single-file, zero-dependency** desktop sticky planner for Windows 10/11.
One `.exe`, double-click to run — no VC++ runtime, no installer, no network access.

![Main window](docs/screenshot-main.png)

## Features

| Area | What you get |
| --- | --- |
| 🪟 Window | Borderless sticky-note style; drag, edge-resize (320×400 up to 70% of the work area), optional always-on-top |
| 📌 Edge snapping | Auto-hides after 300 ms against any of the four screen edges; hover the edge to pop it back out. Startup grace period + requires a released mouse button, so it never "opens and disappears" |
| ✅ Task management | Create / edit / delete / pin / complete / progress bar / drag to reorder |
| 📅 Time picker | Calendar for the date plus hour/minute dropdowns and a "Now" shortcut — no more typing `2026-11-05T09:00` |
| 🔍 Filters & search | All / Pinned / Today / This week / Expired plus keyword search with 300 ms debounce |
| ☑️ Multi-select | A select-all checkbox at the left of the filter row; the delete button appears at its right (with confirmation) |
| 🗂 Tray | Close button minimizes to tray; context menu: Show window / New plan / About / Quit |
| 🚀 Startup & single instance | Optional autostart; launching the exe again focuses the existing window instead of starting a second process |
| 💾 Portable data | The data file lives **next to the executable with the same name** (`.json`) — take it on a USB stick |
| 🤖 AI interface | Built-in CLI and MCP server: an AI handed the exe path can teach itself and drive the app directly, with the UI refreshing live |

## Download

1. Open the [Releases](../../releases) page and download `desktop-sticky-notes-v1.0.0-win64.exe`
2. Put it anywhere (desktop, USB drive, …) — renaming it to `桌面便利贴.exe` is safe; the data file follows the executable's name
3. **Double-click to use** — Windows 10/11, no runtime installation required

Uninstall = delete the exe (its sibling `桌面便利贴.json` holds your data; back it up if needed).

## Shortcuts

| Key | Action |
| --- | --- |
| `Ctrl + N` | New plan |
| `Ctrl + F` | Focus the search box |
| `Ctrl + A` | Select all visible items (multi-select mode) |
| `Esc` | Close the current dialog |
| Double-click a card | Edit it |
| Drag the title bar | Move the window (snap to an edge to hide) |

## Data file

- Location: **same folder, same base name** as the executable, e.g. `桌面便利贴.exe` → `桌面便利贴.json`
- Format: UTF-8 JSON (`items[].title / content / deadline / progress / isPinned / isCompleted / createdAt / updatedAt`)
- Backup: just copy that `.json` file
- External edits (CLI / AI / hand-editing) are detected and reflected in the running UI immediately — no restart

## For AI agents (CLI / MCP)

The program documents itself — **an AI handed the exe path can learn and drive it** without reading source code or clicking the UI.

```bash
桌面便利贴.exe --help            # human-readable manual
桌面便利贴.exe describe --json   # machine-readable: commands, args, examples, data file, exit codes
```

CLI commands: `list` · `add <title>` · `set <selector>` · `done <selector>` · `pin <selector>` · `del <selector>...` — all accept `--json`. A selector is an id or a unique keyword from the title/content. Exit codes: `0` ok, `1` usage, `2` not found / ambiguous, `3` IO. Changes reach the running UI immediately.

MCP: `桌面便利贴.exe mcp` (stdio, newline-delimited JSON-RPC) exposes `app_info`, `list_tasks`, `add_task`, `update_task`, `complete_task`, `pin_task`, `delete_task`, each with a description and JSON Schema, so a registered client surfaces them to the model with zero prompting:

```json
{"mcpServers":{"desktop-sticky-notes":{"command":"C:\\path\\to\\桌面便利贴.exe","args":["mcp"]}}}
```

## Build from source

Requires the [Rust toolchain](https://rustup.rs/) with the Windows GNU target (`x86_64-pc-windows-gnu`; MinGW linker ships with it).

```bash
cargo build --release
```

Output:

```
target/x86_64-pc-windows-gnu/release/desktop-sticky-notes.exe
```

Rename the binary to `桌面便利贴.exe` if you like — the app locates its data file by **its own file name**, so renaming is safe.

> This repository intentionally omits `.cargo/config.toml` (it holds a machine-specific `target-dir`).

### Project layout

```
├── src/
│   ├── main.rs      # UI, task logic, time picker, multi-select, edge-snap state machine
│   ├── tray.rs      # Tray icon and context menu (pure Win32)
│   └── win32.rs     # HWND / work area / window moves / single-instance mutex / autostart
├── build.rs         # Embeds the .ico and version resources (windres)
├── assets/          # Multi-size icon + generator script
├── docs/            # README screenshots
└── Cargo.toml, LICENSE, README.md, CHANGELOG.md
```

## FAQ

**Do I need the VC++ runtime on a clean Windows install?**
No. The import table only references system DLLs (`kernel32 / user32 / gdi32 / shell32 / advapi32 / uxtheme / dwmapi / OPENGL32 / api-ms-win-crt-*`) — no `VCRUNTIME`, no `libgcc`.

**How do I bring a hidden window back?**
Hover the screen edge where it hid, use the tray menu → "Show window", or double-click the exe again.

**How do I disable autostart?**
Delete the `桌面便利贴` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. The program only writes it when the value is missing or points to a missing file.

**Can I run two instances?**
No — deliberately single-instance, so two processes never fight over the same data file.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

[MIT](LICENSE) © 2026 **青衫白马少年**
