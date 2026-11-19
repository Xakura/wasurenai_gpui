//! 配色与尺寸 token。界面里不写裸色值，一律从这里取。
//!
//! 两套皮肤：
//! - `panel`：桌面层面板 + 置顶浮层（深色半透明，贴在壁纸上要压得住）
//! - `editor`：编辑窗口（浅色，跟组件库的浅色主题一致）

pub mod panel {
    /// 面板底色（半透明深色）。
    pub const BG: u32 = 0x141414d0;
    /// 行 hover 底色。
    pub const ROW_HOVER: u32 = 0xffffff0f;
    /// 圆角。
    pub const RADIUS: f32 = 12.0;
    /// 内边距。
    pub const PADDING: f32 = 14.0;

    /// 正文。
    pub const TEXT: u32 = 0xeaeaea;
    /// 次要文字（已完成、元信息）。
    pub const MUTED: u32 = 0x8b8f96;
    /// 半透明分隔线 / 描边（深色底上不要用实心深灰）。
    pub const BORDER: u32 = 0xffffff1f;
    /// 唯一的强调色（只用两处：勾选态 + 选中标签）。
    pub const ACCENT: u32 = 0x4a9eff;
    /// 逾期时间（语义色，不算强调色）。
    pub const DANGER: u32 = 0xff7a7a;
    /// 未选中的标签底色。
    pub const CHIP: u32 = 0xffffff14;
    /// 标签文字。
    pub const CHIP_TEXT: u32 = 0xd6dae0;
}

pub mod editor {
    /// 窗口底色（浅色，但不用纯白）。
    pub const BG: u32 = 0xf6f6f7;
    /// 卡片 / 行底色。
    pub const SURFACE: u32 = 0xffffff;
    /// 正文。
    pub const TEXT: u32 = 0x18181b;
    /// 次要文字。
    pub const MUTED: u32 = 0x71717a;
    /// 半透明分隔线。
    pub const BORDER: u32 = 0x00000014;
    /// 行 hover 底色。
    pub const HOVER: u32 = 0x00000008;
    /// 强调色（与面板同一个蓝）。
    pub const ACCENT: u32 = 0x2f6feb;
    /// 校验错误。
    pub const DANGER: u32 = 0xd92d20;
}
