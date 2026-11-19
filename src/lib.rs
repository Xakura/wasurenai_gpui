//! wasurenai —— 常驻桌面的待办清单。
//!
//! 分层（依赖方向严格单向：ui → core，platform 不依赖 core）：
//!
//! - [`core`]：数据模型、存储、排序筛选、到期判定。纯逻辑，零 UI/Win32 依赖，全部可单测。
//! - `platform`：Win32 相关（置底、托盘、全局热键、专用线程）。
//! - `ui`：gpui 视图与状态。

pub mod core;

#[cfg(windows)]
pub mod platform;

pub mod ui;
