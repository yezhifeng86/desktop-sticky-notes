//! Win32 辅助：取 HWND、窗口矩形、工作区、光标位置、移动窗口、开机自启。

#![cfg(windows)]

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, REG_SZ,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, VK_LBUTTON,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetCursorPos, GetWindowRect, IsIconic, IsZoomed, SetForegroundWindow, SetWindowPos,
    ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_RESTORE,
    SW_SHOW,
};

pub fn hwnd_from_frame(frame: &eframe::Frame) -> isize {
    match frame.window_handle() {
        Ok(handle) => match handle.as_raw() {
            RawWindowHandle::Win32(w) => w.hwnd.get() as isize,
            _ => 0,
        },
        Err(_) => 0,
    }
}

/// 返回 (x, y, w, h) 屏幕物理像素坐标
pub fn window_rect(hwnd: isize) -> (i32, i32, i32, i32) {
    if hwnd == 0 {
        return (0, 0, 0, 0);
    }
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd as HWND, &mut rc) != 0 {
            (rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top)
        } else {
            (0, 0, 0, 0)
        }
    }
}

/// 窗口所在显示器的工作区（排除任务栏），返回 (x, y, w, h)
pub fn work_area(hwnd: isize) -> (i32, i32, i32, i32) {
    unsafe {
        let hmon = MonitorFromWindow(hwnd as HWND, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut mi) != 0 {
            let rc = mi.rcWork;
            (rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top)
        } else {
            let rc = mi.rcMonitor;
            (rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top)
        }
    }
}

/// 小圆角窗口：Windows11 交给 DWM 画圆角（DWMWCP_ROUNDSMALL）。
/// Win10 不支持该属性，返回失败直接忽略，仍是直角。
pub fn round_window_corners(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUNDSMALL: u32 = 3;
    unsafe {
        let pref: u32 = DWMWCP_ROUNDSMALL;
        let _ = windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd as *mut std::ffi::c_void,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

pub fn cursor_pos() -> (i32, i32) {
    unsafe {
        let mut pt: POINT = std::mem::zeroed();
        if GetCursorPos(&mut pt) != 0 {
            (pt.x, pt.y)
        } else {
            (0, 0)
        }
    }
}

/// 窗口是否处于最小化状态（最小化时 Windows 会把窗口挪到屏幕外的 -32000 处）
pub fn is_zoomed(hwnd: isize) -> bool {
    unsafe { IsZoomed(hwnd as HWND) != 0 }
}

pub fn is_minimized(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe { IsIconic(hwnd as HWND) != 0 }
}

/// 窗口是否完全在显示器工作区之外（最小化、被挪到屏幕外都算）
pub fn is_offscreen(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    let (x, y, w, h) = window_rect(hwnd);
    if w <= 0 || h <= 0 {
        return true;
    }
    let (ax, ay, aw, ah) = work_area(hwnd);
    x + w <= ax || x >= ax + aw || y + h <= ay || y >= ay + ah
}

/// 移动窗口（SWP_NOZORDER：不改变 z-order，置顶状态由 egui 的 WindowLevel 管）
/// 退出最大化（吸附隐藏前调用）
pub fn unmaximize(hwnd: isize) {
    unsafe {
        ShowWindow(hwnd as HWND, SW_RESTORE);
    }
}

/// 找到已在运行的实例窗口并把它唤出来（单实例第二次启动时用）。
/// 吸附隐藏中的窗口大部分在屏幕外 → 先夹回可见区，再前置。
pub fn reveal_window(title: &str) -> bool {
    let t = wide(title);
    unsafe {
        let h = FindWindowW(std::ptr::null_mut(), t.as_ptr());
        if h.is_null() {
            return false;
        }
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        } else {
            ShowWindow(h, SW_SHOW);
        }
        let hi = h as isize;
        let (x, y, w, hh) = window_rect(hi);
        if w > 0 && hh > 0 {
            let (nx, ny) = clamp_to_visible(x, y, w, hh);
            if (nx, ny) != (x, y) {
                move_window(hi, nx, ny);
            }
        }
        SetForegroundWindow(h);
        true
    }
}

/// 同时改位置和大小（边框缩放用）
pub fn set_window_rect(hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(), // HWND_TOP
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// 把鼠标事件锁定到本窗口（拖拽/缩放时指针移出窗口也继续收到 move/up）
pub fn capture_mouse(hwnd: isize) {
    unsafe {
        SetCapture(hwnd as HWND);
    }
}

pub fn release_mouse() {
    unsafe {
        ReleaseCapture();
    }
}

pub fn move_window(hwnd: isize, x: i32, y: i32) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            HWND_TOPMOST,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

/// 把窗口位置夹到可见范围内（防止恢复到已拔掉的显示器上）
pub fn clamp_to_visible(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let origin = POINT {
            x: x + w / 2,
            y: y + h / 2,
        };
        let hmon = MonitorFromPoint(origin, MONITOR_DEFAULTTOPRIMARY);
        if GetMonitorInfoW(hmon, &mut mi) == 0 {
            return (x, y);
        }
        let rc = mi.rcWork;
        let nx = x.clamp(rc.left, (rc.right - w).max(rc.left));
        let ny = y.clamp(rc.top, (rc.bottom - h).max(rc.top));
        (nx, ny)
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 鼠标左键的真实状态。
/// 不用 egui 的 pointer.primary_down()：系统级拖拽/最大化会吞掉 UP 事件，
/// egui 会一直以为左键还按着，导致贴边弹出条件 `!pointer_down` 永远不成立（实测踩过）。
pub fn left_button_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000 != 0 }
}

/// 读一个 REG_SZ 值
unsafe fn reg_read(hkey: HKEY, name: *const u16) -> Option<String> { unsafe {
    let mut ty: u32 = 0;
    let mut size: u32 = 0;
    if RegQueryValueExW(
        hkey,
        name,
        std::ptr::null_mut(),
        &mut ty,
        std::ptr::null_mut(),
        &mut size,
    ) != 0
        || size == 0
    {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    if RegQueryValueExW(
        hkey,
        name,
        std::ptr::null_mut(),
        &mut ty,
        buf.as_mut_ptr(),
        &mut size,
    ) != 0
    {
        return None;
    }
    let units: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
}}

/// 开机自启：HKCU\...\Run
/// - 值不存在 → 写入
/// - 存在但指向的 exe 已不在（改名/移动过）→ 更新
/// - 用户手动删掉的 → 不加回
pub fn ensure_autostart() {
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => return,
    };
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide("桌面便利贴");
    let value = wide(&exe);
    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        if RegCreateKeyW(HKEY_CURRENT_USER, key.as_ptr(), &mut hkey) != 0 {
            return;
        }
        // 新项缺失或指向的 exe 已不在（移动过）→ 写入/更新
        let need_write = match reg_read(hkey, name.as_ptr()) {
            None => true,
            Some(v) => !std::path::Path::new(&v).exists(),
        };
        if need_write {
            let bytes = (value.len() * 2) as u32;
            RegSetValueExW(
                hkey,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr() as *const u8,
                bytes,
            );
        }
        RegCloseKey(hkey);
    }
}
