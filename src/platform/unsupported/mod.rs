//! 非 Windows / macOS 平台的兜底空实现，保证跨平台类型检查可通过。

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use async_channel::Sender;

use crate::core::hotkey::HotkeySpec;
use crate::platform::{NativeWindow, PlatformCommand, PlatformEvent, WindowLayer};

static LAYER: AtomicU8 = AtomicU8::new(WindowLayer::Bottom as u8);

pub struct SingleInstanceGuard;

impl SingleInstanceGuard {
    pub fn acquire() -> Option<Self> {
        Some(Self)
    }
}

pub fn attach_desktop_panel(_window: NativeWindow) -> bool {
    false
}

pub fn current_layer() -> WindowLayer {
    match LAYER.load(Ordering::Relaxed) {
        x if x == WindowLayer::Top as u8 => WindowLayer::Top,
        _ => WindowLayer::Bottom,
    }
}

pub fn set_layer(_window: NativeWindow, layer: WindowLayer) -> bool {
    LAYER.store(layer as u8, Ordering::Relaxed);
    false
}

pub fn set_visible(_window: NativeWindow, _visible: bool) -> bool {
    false
}

pub fn apply_window_icon(_window: NativeWindow) {}

pub fn spawn_service(
    _tx: Sender<PlatformEvent>,
    _initial_hotkey: HotkeySpec,
    _initial_pending: usize,
    _initial_overdue: usize,
) {
}

pub fn send_command(_cmd: PlatformCommand) -> bool {
    false
}

pub fn autostart_is_enabled() -> bool {
    false
}

pub fn autostart_set_enabled(_enabled: bool) -> Result<(), String> {
    Err("当前平台暂不支持开机自启".to_string())
}

pub fn open_file(_path: &Path) -> Result<(), String> {
    Err("当前平台暂不支持打开文件".to_string())
}

pub fn reveal_in_file_manager(_path: &Path) -> Result<(), String> {
    Err("当前平台暂不支持打开文件管理器".to_string())
}
