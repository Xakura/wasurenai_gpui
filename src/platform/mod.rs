//! 跨平台系统集成抽象层（HAL）。
//!
//! 职责边界：只做操作系统级集成（窗口置底/置顶、托盘与全局热键、开机自启、单实例、
//! 文件管理器定位、图标生成），不知道 `TodoItem` 等领域数据结构。
//!
//! 依赖方向：`ui` / `main` → `platform`；`ui` 层不接触 `HWND` / `NSWindow` 等 OS 原生类型，
//! 仅通过 [`NativeWindow`]（基于 `raw_window_handle`）向本层传递窗口句柄。

use std::path::Path;

use async_channel::Sender;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::core::hotkey::HotkeySpec;

pub mod icon;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use self::windows as imp;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use self::macos as imp;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod unsupported;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use self::unsupported as imp;

/// 平台无关的原生窗口句柄包装，避免 `ui` 层直接引用 `HWND` 或 `NSWindow`。
#[derive(Debug, Clone, Copy)]
pub struct NativeWindow(RawWindowHandle);

impl NativeWindow {
    /// 从任何实现了 `HasWindowHandle` 的窗口（如 GPUI `Window`）提取原生句柄。
    pub fn from_window<W: HasWindowHandle>(window: &W) -> Option<Self> {
        window.window_handle().ok().map(|h| Self(h.as_raw()))
    }

    pub(crate) fn raw(self) -> RawWindowHandle {
        self.0
    }
}

/// 桌面常驻面板当前所处的层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowLayer {
    /// 桌面层：常驻于普通窗口之下，显示桌面时仍可见。
    Bottom = 0,
    /// 置顶层：浮在所有窗口之上（全局快捷键 / 到期强提醒）。
    Top = 1,
}

/// 平台服务发给 GPUI 主线程的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEvent {
    /// 托盘「编辑待办」被点击。
    OpenEditor,
    /// 托盘「设置」被点击。
    OpenSettings,
    /// 托盘「收起 / 展开」被点击。
    ToggleWidget,
    /// 全局快捷键：桌面层窗口上浮 / 回落。
    ToggleFloat,
    /// 全局快捷键注册结果（换绑后回报给 UI）。
    HotkeyChanged {
        spec: String,
        error: Option<String>,
    },
    /// 托盘「退出」被点击。
    Quit,
}

/// GPUI 主线程发给平台服务的命令。
#[derive(Debug, Clone)]
pub enum PlatformCommand {
    /// 重新注册全局快捷键。
    SetHotkey(HotkeySpec),
    /// 录制快捷键期间临时注销旧热键，避免按下原组合键时误触上浮。
    PauseHotkey,
    /// 取消录制后恢复原先的全局快捷键。
    ResumeHotkey,
    /// 同步托盘提示、图标状态与「收起 / 展开」菜单文案。
    UpdateTray {
        pending: usize,
        overdue: usize,
        widget_visible: bool,
    },
}

/// 单实例守卫；进程退出时 `Drop` 释放系统锁。
pub struct SingleInstanceGuard(
    /// 持有平台单实例锁句柄，仅依赖 `Drop` 在进程退出时释放。
    #[allow(dead_code)]
    imp::SingleInstanceGuard,
);

impl SingleInstanceGuard {
    /// 尝试获取单实例锁。若已有实例在运行则返回 `None`。
    pub fn acquire() -> Option<Self> {
        imp::SingleInstanceGuard::acquire().map(Self)
    }
}

/// 桌面面板窗口层级、可见性与窗口图标控制。
pub mod window {
    use super::{NativeWindow, WindowLayer, imp};

    /// 接管桌面常驻面板窗口（去除任务栏图标、去除系统默认圆角/边框、挂接桌面层）。
    pub fn attach_desktop_panel(window: NativeWindow) -> bool {
        imp::attach_desktop_panel(window)
    }

    /// 查询面板当前所处层级。
    pub fn current_layer() -> WindowLayer {
        imp::current_layer()
    }

    /// 切换桌面面板层级（置底 / 置顶）。
    pub fn set_layer(window: NativeWindow, layer: WindowLayer) -> bool {
        imp::set_layer(window, layer)
    }

    /// 显示 / 隐藏桌面面板窗口（不销毁句柄）。
    pub fn set_visible(window: NativeWindow, visible: bool) -> bool {
        imp::set_visible(window, visible)
    }

    /// 为普通编辑窗口应用内嵌应用图标（Windows 上通过 `WM_SETICON` 设置，macOS 由 `.app` Bundle 负责）。
    pub fn apply_window_icon(window: NativeWindow) {
        imp::apply_window_icon(window);
    }
}

/// 系统托盘与全局热键后台服务。
pub mod service {
    use super::{HotkeySpec, PlatformCommand, PlatformEvent, Sender, imp};

    /// 启动平台服务（托盘图标/菜单 + 全局快捷键）。
    pub fn spawn(
        tx: Sender<PlatformEvent>,
        initial_hotkey: HotkeySpec,
        initial_pending: usize,
        initial_overdue: usize,
    ) {
        imp::spawn_service(tx, initial_hotkey, initial_pending, initial_overdue);
    }

    /// 向平台服务投递命令。返回是否成功唤醒并投递。
    pub fn send_command(cmd: PlatformCommand) -> bool {
        imp::send_command(cmd)
    }
}

/// 开机自启动配置。
pub mod autostart {
    use super::imp;

    pub fn is_enabled() -> bool {
        imp::autostart_is_enabled()
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        imp::autostart_set_enabled(enabled)
    }
}

/// 系统文件关联打开与文件管理器定位。
pub mod shell {
    use super::{Path, imp};

    /// 使用系统默认关联程序打开指定文件。
    pub fn open_file(path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("文件不存在：{}", path.display()));
        }
        imp::open_file(path)
    }

    /// 在系统文件管理器中定位并选中指定文件（Windows: Explorer，macOS: Finder）。
    pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
        imp::reveal_in_file_manager(path)
    }
}
