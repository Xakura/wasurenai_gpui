//! Win32 平台相关能力。只做"系统集成"，不知道待办数据长什么样。
//!
//! 依赖方向：ui → platform，platform 不依赖 core / ui。

pub mod win_layer;
pub mod win_thread;

/// 平台线程发给 gpui 主线程的事件。
///
/// 只放"发生了什么"，具体语义（打开编辑器、切换浮层…）由 ui 层解释。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEvent {
    /// 托盘「编辑」被点击。
    OpenEditor,
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
