//! 配色与尺寸 token。界面里不写裸色值，一律从这里取。
//!
//! **约定**：色值末两位是 alpha 的 token 必须用 `rgba()` 消费；用 `rgb()` 会丢掉 alpha
//! 变成一个不透明色（曾经把 hover 底色 `0x00000008` 渲染成纯黑，就是踩了这个坑）。
//!
//! 对比度要求（WCAG，小字更严格）：正文 ≥7、次级文字 ≥4.5 且实际按 ≥5.4 取值、
//! 交互色（强调/危险）≥4.5。`editor` 那套值都是按这个标准算过的。
//!
//! 两套皮肤：
//! - `panel`：桌面层面板（深色半透明，贴在壁纸上要压得住）
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
    pub const MUTED: u32 = 0x9aa0a8;
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
    pub const BG: u32 = 0xf4f4f5;
    /// 表面（表头 / kbd 芯片）。
    pub const SURFACE: u32 = 0xffffff;
    /// 正文：对 `BG` 对比度 15.7。
    pub const TEXT: u32 = 0x1c1c1f;
    /// 次级文字：对 `BG` 对比度 7.16（11px 小字也够）。
    pub const MUTED: u32 = 0x52525b;
    /// 分隔线（8% 黑，出来约 `#e2e2e3`）。
    pub const BORDER: u32 = 0x00000014;
    /// 行 hover 底色（6% 黑，出来约 `#f0f0f0`）。
    pub const HOVER: u32 = 0x0000000f;
    /// 强调色：对 `BG` 对比度 5.39。
    pub const ACCENT: u32 = 0x1f5fd0;
    /// 危险色：对 `BG` 对比度 6.09。
    pub const DANGER: u32 = 0xb42318;
}
