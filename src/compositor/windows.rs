//! Win32 window adapter. The Windows analogue of the X11/EWMH adapter: it
//! lists alt-tab-worthy top-level windows via `EnumWindows`, tracks the
//! foreground window, and drives activate / close / maximise through the
//! ordinary Win32 messaging APIs. No compositor protocol is involved — the
//! DWM always composites, so the transparent overlay just works.
//!
//! `WindowId` is the window's `HWND` rendered as a decimal string.
//! `app_id` is the owning process's executable stem, lowercased ("firefox"),
//! so it matches desktop-app entries the same way an X11 class does.

use super::{Compositor, CompositorEvent, WindowInfo};
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HANDLE, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetSystemMetrics, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, PostMessageW, SetForegroundWindow, ShowWindow,
};

// Win32 constants kept local so we don't depend on which windows-sys module
// re-exports each one (they move between releases).
const GWL_EXSTYLE: i32 = -20;
const GW_OWNER: u32 = 4;
const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
const WS_EX_APPWINDOW: isize = 0x0004_0000;
const SW_RESTORE: i32 = 9;
const SW_MAXIMIZE: i32 = 3;
const WM_CLOSE: u32 = 0x0010;
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const DWMWA_CLOAKED: u32 = 14;
const TRUE: BOOL = 1;
const FALSE: BOOL = 0;

pub struct WindowsCompositor;

impl WindowsCompositor {
    /// Win32 is always available on Windows; this never fails.
    pub fn connect() -> Result<WindowsCompositor, String> {
        Ok(WindowsCompositor)
    }
}

impl Compositor for WindowsCompositor {
    fn backend(&self) -> &'static str {
        "windows"
    }

    fn output_size(&mut self) -> Option<(u32, u32)> {
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        (w > 0 && h > 0).then_some((w as u32, h as u32))
    }

    fn windows(&mut self) -> Vec<WindowInfo> {
        list_windows()
    }

    fn active_window(&mut self) -> Option<WindowInfo> {
        let fg = unsafe { GetForegroundWindow() };
        if fg.is_null() {
            return None;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(fg, &mut pid) };
        if pid == unsafe { GetCurrentProcessId() } || !unsafe { is_app_window(fg) } {
            return None;
        }
        Some(WindowInfo {
            id: hwnd_id(fg),
            app_id: process_stem(pid).unwrap_or_default(),
            title: window_title(fg),
            focused: true,
        })
    }

    /// Poll the window set + foreground every 400 ms and push events on change.
    /// Win32 has no free "window list changed" signal short of a WinEvent hook
    /// with its own message pump; polling is simpler and cheap for a launcher.
    fn watch(&mut self, tx: Sender<CompositorEvent>) {
        std::thread::spawn(move || {
            let mut last_ids: Vec<String> = Vec::new();
            let mut last_active: Option<String> = None;
            loop {
                let ws = list_windows();
                let ids: Vec<String> = ws.iter().map(|w| w.id.clone()).collect();
                if ids != last_ids {
                    last_ids = ids;
                    if tx.send(CompositorEvent::Windows(ws.clone())).is_err() {
                        return;
                    }
                }
                let active = ws.into_iter().find(|w| w.focused);
                let active_id = active.as_ref().map(|w| w.id.clone());
                if active_id != last_active {
                    last_active = active_id;
                    if tx.send(CompositorEvent::Active(active)).is_err() {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        });
    }

    fn activate(&mut self, id: &super::WindowId) {
        if let Some(hwnd) = parse_hwnd(id) {
            unsafe {
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                force_foreground(hwnd);
            }
        }
    }

    fn close_window(&mut self, id: &super::WindowId) {
        if let Some(hwnd) = parse_hwnd(id) {
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        }
    }

    /// Windows has no per-window "fullscreen"; maximise is the closest analogue.
    fn fullscreen(&mut self, id: &super::WindowId) {
        if let Some(hwnd) = parse_hwnd(id) {
            unsafe {
                force_foreground(hwnd);
                ShowWindow(hwnd, SW_MAXIMIZE);
            }
        }
    }
}

// ------------------------------------------------------------- enumeration

struct EnumCtx {
    out: Vec<WindowInfo>,
    self_pid: u32,
}

fn list_windows() -> Vec<WindowInfo> {
    let mut ctx = EnumCtx {
        out: Vec::new(),
        self_pid: unsafe { GetCurrentProcessId() },
    };
    unsafe { EnumWindows(Some(enum_proc), &mut ctx as *mut EnumCtx as LPARAM) };
    let fg = hwnd_id(unsafe { GetForegroundWindow() });
    for w in &mut ctx.out {
        if w.id == fg {
            w.focused = true;
        }
    }
    ctx.out
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam as *mut EnumCtx);
    if !is_app_window(hwnd) {
        return TRUE; // keep enumerating
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == ctx.self_pid {
        return TRUE; // our own overlay never lists itself
    }
    let title = window_title(hwnd);
    if title.is_empty() {
        return TRUE;
    }
    ctx.out.push(WindowInfo {
        id: hwnd_id(hwnd),
        app_id: process_stem(pid).unwrap_or_default(),
        title,
        focused: false,
    });
    TRUE
}

/// The alt-tab heuristic: visible, titled, not a tool window, either
/// unowned or explicitly an app window, and not a cloaked (ghost UWP) window.
unsafe fn is_app_window(hwnd: HWND) -> bool {
    if IsWindowVisible(hwnd) == 0 || GetWindowTextLengthW(hwnd) == 0 {
        return false;
    }
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    if ex & WS_EX_TOOLWINDOW != 0 {
        return false;
    }
    if !GetWindow(hwnd, GW_OWNER).is_null() && ex & WS_EX_APPWINDOW == 0 {
        return false;
    }
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut c_void,
        std::mem::size_of::<u32>() as u32,
    );
    cloaked == 0
}

// ------------------------------------------------------------- helpers

fn hwnd_id(hwnd: HWND) -> String {
    (hwnd as isize).to_string()
}

fn parse_hwnd(id: &str) -> Option<HWND> {
    id.parse::<isize>().ok().map(|v| v as HWND)
}

fn window_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

/// Owning process's executable stem, lowercased, `.exe` stripped ("firefox").
fn process_stem(pid: u32) -> Option<String> {
    unsafe {
        let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let base = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        let stem = base
            .strip_suffix(".exe")
            .or_else(|| base.strip_suffix(".EXE"))
            .unwrap_or(base);
        Some(stem.to_lowercase())
    }
}

/// Give `hwnd` the foreground. Windows blocks background focus-steals, so we
/// briefly attach to the current foreground thread's input queue — the trick
/// that lets a foreground app hand focus to another window reliably.
unsafe fn force_foreground(hwnd: HWND) {
    let target_tid = GetCurrentThreadId();
    let fg = GetForegroundWindow();
    let fg_tid = if fg.is_null() {
        0
    } else {
        GetWindowThreadProcessId(fg, std::ptr::null_mut())
    };
    if fg_tid != 0 && fg_tid != target_tid {
        AttachThreadInput(target_tid, fg_tid, TRUE);
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        AttachThreadInput(target_tid, fg_tid, FALSE);
    } else {
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
    }
}
