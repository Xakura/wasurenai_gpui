//! 界面配色与尺寸。全部集中在这里，后面换皮肤只改这个文件。

/// 桌面层窗口背景（半透明深色，末位是 alpha）。
pub const SURFACE: u32 = 0x141414d0;
/// 面板圆角。
pub const RADIUS: f32 = 12.0;
/// 面板内边距。
pub const PADDING: f32 = 14.0;

/// 正文颜色。
pub const TEXT: u32 = 0xeaeaea;
/// 次要文字（已完成条目、无截止时间）。
pub const TEXT_MUTED: u32 = 0x8b8f96;
/// 截止时间颜色（未到期）。
pub const DUE: u32 = 0x8ecdf7;
/// 逾期颜色。
pub const DUE_OVERDUE: u32 = 0xff7a7a;

/// 筛选条未选中底色。
pub const CHIP: u32 = 0xffffff14;
/// 筛选条选中底色。
pub const CHIP_ACTIVE: u32 = 0x1e6fd9cc;
/// 筛选条文字。
pub const CHIP_TEXT: u32 = 0xd6dae0;
