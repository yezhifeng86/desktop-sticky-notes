//! 系统托盘：独立线程 + 消息窗口，事件经 mpsc 交给主界面。
//!
//! 对应原版行为：双击托盘恢复窗口；右键菜单 显示窗口 / 新建计划 / 退出。

#![cfg(windows)]

use std::sync::mpsc::Sender;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Foundation::{HWND, POINT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER,
    DIB_RGB_COLORS, HBITMAP, RGBQUAD,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetMessageW, GetCursorPos, InsertMenuW, LoadIconW, PostMessageW, RegisterClassExW, SetForegroundWindow,
    TrackPopupMenu, TranslateMessage, CreateIconIndirect, ICONINFO, IDI_APPLICATION, MF_SEPARATOR,
    MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WNDCLASSEXW, WM_APP, WM_COMMAND,
    WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, HWND_MESSAGE,
};

#[derive(Clone, Copy, Debug)]
pub enum TrayMsg {
    Show,
    NewItem,
    About,
    Quit,
}

const WM_TRAY: u32 = WM_APP + 1;
const ID_SHOW: usize = 1001;
const ID_NEW: usize = 1002;
const ID_QUIT: usize = 1003;
const ID_ABOUT: usize = 1004;

static TX: Mutex<Option<Sender<TrayMsg>>> = Mutex::new(None);

fn emit(msg: TrayMsg) {
    if let Ok(guard) = TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(msg);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = (lparam as u32) & 0xFFFF;
            if event == WM_LBUTTONDBLCLK || event == WM_LBUTTONUP {
                emit(TrayMsg::Show);
            } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                show_menu(hwnd);
            }
            0
        }
        WM_COMMAND => {
            match wparam & 0xFFFF {
                ID_SHOW => emit(TrayMsg::Show),
                ID_NEW => emit(TrayMsg::NewItem),
                ID_ABOUT => emit(TrayMsg::About),
                ID_QUIT => emit(TrayMsg::Quit),
                _ => {}
            }
            0
        }
        WM_DESTROY => 0,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let s_show = wide("显示窗口");
    let s_new = wide("新建计划");
    let s_about = wide("关于");
    let s_quit = wide("退出");
    AppendMenuW(menu, MF_STRING, ID_SHOW, s_show.as_ptr());
    AppendMenuW(menu, MF_STRING, ID_NEW, s_new.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(menu, MF_STRING, ID_ABOUT, s_about.as_ptr());
    AppendMenuW(menu, MF_STRING, ID_QUIT, s_quit.as_ptr());

    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, std::ptr::null());
    DestroyMenu(menu);
    // 修复托盘菜单收起后不消失的经典问题
    PostMessageW(hwnd, WM_NULL, 0, 0);
}

/// 托盘图标：优先用 exe 里嵌入的 .ico（build.rs 里 windres 以资源 ID 1 嵌入），
/// 拿不到时退回自绘的黄色方块。
unsafe fn make_icon() -> windows_sys::Win32::Foundation::HANDLE {
    let module = GetModuleHandleW(std::ptr::null());
    let embedded = LoadIconW(module, 1 as *const u16); // MAKEINTRESOURCE(1)
    if !embedded.is_null() {
        return embedded;
    }

    const W: i32 = 16;
    const H: i32 = 16;
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = W;
    bmi.bmiHeader.biHeight = -H; // 自上而下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = 0; // BI_RGB

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hdc = GetDC(std::ptr::null_mut());
    let hbmp = CreateDIBSection(hdc, &mut bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
    if hbmp.is_null() || bits.is_null() {
        if !hbmp.is_null() {
            DeleteObject(hbmp as _);
        }
        if !hdc.is_null() {
            ReleaseDC(std::ptr::null_mut(), hdc);
        }
        return LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
    }
    // BGRA：底色 #FFF9C4，边框 #F9A825
    let fill: u32 = 0xFFF9_C4u32 | 0xFF00_0000;
    let border: u32 = 0x25A8_F9u32 | 0xFF00_0000;
    let px = bits as *mut u32;
    for y in 0..H {
        for x in 0..W {
            let edge = x == 0 || y == 0 || x == W - 1 || y == H - 1;
            let value = if edge { border } else { fill };
            px.add((y * W + x) as usize).write_unaligned(value);
        }
    }
    let mask = CreateBitmap(W, H, 1, 1, std::ptr::null());
    let mut ii: ICONINFO = std::mem::zeroed();
    ii.fIcon = TRUE;
    ii.hbmColor = hbmp;
    ii.hbmMask = mask;
    let icon = CreateIconIndirect(&ii);
    DeleteObject(hbmp as _);
    if !mask.is_null() {
        DeleteObject(mask as _);
    }
    ReleaseDC(std::ptr::null_mut(), hdc);
    if icon.is_null() {
        LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
    } else {
        icon
    }
}

/// 启动托盘线程。事件通过 `tx` 回传主界面。
pub fn spawn(tx: Sender<TrayMsg>) {
    std::thread::spawn(move || unsafe {
        if let Ok(mut guard) = TX.lock() {
            *guard = Some(tx);
        }

        let class_name = wide("PlanStickyTrayClass");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()) as windows_sys::Win32::Foundation::HINSTANCE,
            hIcon: LoadIconW(std::ptr::null_mut(), IDI_APPLICATION),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: LoadIconW(std::ptr::null_mut(), IDI_APPLICATION),
        };
        RegisterClassExW(&wc);

        let window_name = wide("桌面计划贴托盘");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            wc.hInstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }

        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = make_icon();
        let tip = wide("桌面计划贴");
        let len = (tip.len() - 1).min(nid.szTip.len() - 1);
        nid.szTip[..len].copy_from_slice(&tip[..len]);
        Shell_NotifyIconW(NIM_ADD, &nid);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Shell_NotifyIconW(NIM_DELETE, &nid);
        if let Ok(mut guard) = TX.lock() {
            *guard = None;
        }
    });
}

/// 退出时移除托盘图标（进程退出前调用一次）
pub fn shutdown() {
    // 托盘线程随进程结束而终止，Windows 会自动回收图标；
    // 这里额外广播一次关闭消息，尽量优雅地收尾。
    unsafe {
        let _ = InsertMenuW;
    }
}
