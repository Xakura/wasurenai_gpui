//! macOS 平台后端占位桩（为后续移植预留统一接口）。
//!
//! 未来 macOS 完整实现映射参考：
//! - `window`：从 `RawWindowHandle::AppKit` 取得 `NSView` / `NSWindow`：
//!   - `WindowLayer::Bottom` → `CGWindowLevelForKey(kCGDesktopWindowLevelKey)` +
//!     `NSWindowCollectionBehaviorCanJoinAllSpaces | Stationary | IgnoresCycle`
//!   - `WindowLayer::Top` → `NSFloatingWindowLevel` / `NSStatusWindowLevel`
//!   - `set_visible` → `orderFrontRegardless` / `orderOut`
//! - `service`：`tray-icon` + `global-hotkey`（注意 macOS 上需在主线程 NSRunLoop 创建）
//! - `autostart`：`SMAppService` 或 `~/Library/LaunchAgents/com.wasurenai.plist`
//! - `single_instance`：`flock` 锁文件（位于 `~/Library/Caches/wasurenai_gpui/instance.lock`）
//! - `shell`：已直接通过 `open` / `open -R` 实现

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};

use async_channel::Sender;

use crate::core::hotkey::HotkeySpec;
use crate::platform::{NativeWindow, PlatformCommand, PlatformEvent, WindowLayer};

static LAYER: AtomicU8 = AtomicU8::new(WindowLayer::Bottom as u8);

pub struct SingleInstanceGuard;

impl SingleInstanceGuard {
    pub fn acquire() -> Option<Self> {
        // TODO(macos): 使用 flock 锁文件实现单实例守卫
        Some(Self)
    }
}

pub fn attach_desktop_panel(_window: NativeWindow) -> bool {
    // TODO(macos): 设置 NSWindow level 为 desktop layer 并配置 collectionBehavior
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
    // TODO(macos): 切换 NSWindow level
    false
}

pub fn set_visible(_window: NativeWindow, _visible: bool) -> bool {
    // TODO(macos): orderFrontRegardless / orderOut
    false
}

pub fn apply_window_icon(_window: NativeWindow) {
    // macOS 应用图标由 .app Bundle 的 Info.plist / .icns 管理，无需逐窗设置
}

pub fn spawn_service(
    _tx: Sender<PlatformEvent>,
    _initial_hotkey: HotkeySpec,
    _initial_pending: usize,
    _initial_overdue: usize,
) {
    // TODO(macos): 在主线程初始化 NSStatusItem 与全局热键
    eprintln!("[wasurenai] macOS 托盘与全局热键服务尚未实现");
}

pub fn send_command(_cmd: PlatformCommand) -> bool {
    false
}

pub fn autostart_is_enabled() -> bool {
    // TODO(macos): 查询 LaunchAgents plist 或 SMAppService 状态
    false
}

pub fn autostart_set_enabled(_enabled: bool) -> Result<(), String> {
    Err("macOS 开机自启尚未实现".to_string())
}

pub fn open_file(path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("打开文件失败：{err}"))
}

pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let mut cmd = Command::new("open");
    if path.exists() {
        cmd.arg("-R").arg(path);
    } else if let Some(parent) = path.parent() {
        cmd.arg(parent);
    } else {
        return Err(format!("无法定位目录：{}", path.display()));
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|err| format!("打开 Finder 失败：{err}"))
}
