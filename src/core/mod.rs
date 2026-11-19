//! 纯逻辑层：不依赖 gpui，也不依赖 Win32。
//!
//! 这里的所有函数都应该是可单测的；界面只负责把数据画出来、把用户操作转成对
//! [`store::Store`] 的调用。

pub mod model;
pub mod ordering;
pub mod reminder;
pub mod store;
pub mod timeparse;
