//! Windows 平台后端实现。

mod autostart;
mod service;
mod shell;
mod single_instance;
mod window_icon;
mod window_layer;

use std::path::Path;

use async_channel::Sender;
use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::core::hotkey::HotkeySpec;
use crate::platform::{NativeWindow, PlatformCommand, PlatformEvent, WindowLayer};

pub use single_instance::SingleInstanceGuard;

pub(crate) fn hwnd_of(window: NativeWindow) -> Option<HWND> {
    match window.raw() {
        RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut core::ffi::c_void)),
        _ => None,
    }
}

pub fn attach_desktop_panel(window: NativeWindow) -> bool {
    let Some(hwnd) = hwnd_of(window) else {
        eprintln!("[wasurenai] 拿不到窗口 HWND，桌面层接管失败");
        return false;
    };
    let ok = window_layer::attach(hwnd);
    println!(
        "[wasurenai] 桌面层接管={ok} 子类化={} hwnd={hwnd:?}",
        window_layer::subclassed()
    );
    ok
}

pub fn current_layer() -> WindowLayer {
    window_layer::current_layer()
}

pub fn set_layer(window: NativeWindow, layer: WindowLayer) -> bool {
    let Some(hwnd) = hwnd_of(window) else {
        return false;
    };
    window_layer::set_layer(hwnd, layer)
}

pub fn set_visible(window: NativeWindow, visible: bool) -> bool {
    let Some(hwnd) = hwnd_of(window) else {
        return false;
    };
    window_layer::set_visible(hwnd, visible)
}

pub fn apply_window_icon(window: NativeWindow) {
    if let Some(hwnd) = hwnd_of(window) {
        window_icon::apply_window_icon(hwnd);
    }
}

pub fn spawn_service(
    tx: Sender<PlatformEvent>,
    initial_hotkey: HotkeySpec,
    initial_pending: usize,
    initial_overdue: usize,
) {
    service::spawn(tx, initial_hotkey, initial_pending, initial_overdue);
}

pub fn send_command(cmd: PlatformCommand) -> bool {
    service::send_command(cmd)
}

pub fn autostart_is_enabled() -> bool {
    autostart::is_enabled()
}

pub fn autostart_set_enabled(enabled: bool) -> Result<(), String> {
    autostart::set_enabled(enabled)
}

pub fn open_file(path: &Path) -> Result<(), String> {
    shell::open_file(path)
}

pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    shell::reveal_in_file_manager(path)
}
