//! wasurenai —— 常驻桌面的待办清单。
//!
//! 分层（依赖方向严格单向：`ui` → `core` / `platform`，`platform` 不依赖 `ui` 与待办模型）：
//!
//! - [`core`]：数据模型、存储、排序筛选、到期判定、自动日志。纯逻辑，零 UI/OS 依赖，全部可单测。
//! - [`platform`]：跨平台系统集成抽象层（窗口置底/置顶、托盘、全局热键、开机自启、单实例、文件打开）。
//! - [`ui`]：GPUI 视图与状态（零 Win32/Cocoa 直接依赖）。

pub mod core;
pub mod platform;
pub mod ui;
