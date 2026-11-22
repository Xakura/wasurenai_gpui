//! 配色与尺寸 token。界面里不写裸色值，一律从这里取。
//!
//! **约定**：
//! - `0xRRGGBB`（6 位十六进制）必须用 `rgb()` 消费；误用 `rgba()` 会把高位当成 `0x00` 并导致颜色严重偏移。
//! - `0xRRGGBBAA`（8 位十六进制，末两位是 alpha）必须用 `rgba()` 消费；误用 `rgb()` 会丢掉 alpha。
//!
//! 对比度要求（WCAG，小字更严格）：正文 ≥7、次级文字 ≥4.5、交互色（强调/危险）≥4.5。
//!
//! 两套皮肤：
//! - `panel`：桌面层面板（深色半透明，贴在壁纸上要压得住）
//! - `editor`：编辑窗口（浅灰底 + 纯白圆角卡片风，跟组件库的浅色主题一致）

pub mod panel {
    /// 面板底色（半透明深色）。
    pub const BG: u32 = 0x141414d0;
    /// 行 hover 底色。
    pub const ROW_HOVER: u32 = 0xffffff0f;
    /// 圆角。
    pub const RADIUS: f32 = 14.0;
    /// 内边距。
    pub const PADDING: f32 = 14.0;

    /// 正文。
    pub const TEXT: u32 = 0xeaeaea;
    /// 次要文字（已完成、元信息）。
    pub const MUTED: u32 = 0x9aa0a8;
    /// 半透明分隔线 / 描边（深色底上不要用实心深灰）。
    pub const BORDER: u32 = 0xffffff1f;
    /// 上浮置顶状态下的柔和强调细边（区分桌面常驻与置顶浮层）。
    pub const BORDER_TOP: u32 = 0x4a9eff52;
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
    /// 窗口画布底色（柔和浅灰，烘托纯白卡片）。
    pub const BG: u32 = 0xf3f4f6;
    /// 纯白卡片与顶栏表面。
    pub const SURFACE: u32 = 0xffffff;
    /// 次级表面（微底 / 空标签胶囊）。
    pub const SURFACE_SUBTLE: u32 = 0xf9fafb;
    /// 正文（对白底对比度 >17）。
    pub const TEXT: u32 = 0x18181b;
    /// 次级文字 / 已完成文字（对白底对比度 >7.5）。
    pub const MUTED: u32 = 0x52525b;
    /// 辅助提示 / 表头说明（对白底对比度 4.83）。
    pub const SUBTLE: u32 = 0x71717a;
    /// 卡片与行间分隔线（8% 黑）。
    pub const BORDER: u32 = 0x00000014;
    /// 按钮 / 聚焦态边框（14% 黑）。
    pub const BORDER_STRONG: u32 = 0x00000024;
    /// 行悬停底色（4% 黑）。
    pub const HOVER: u32 = 0x0000000a;
    /// 主强调蓝（对白底对比度 5.17）。
    pub const ACCENT: u32 = 0x2563eb;
    /// 主强调蓝悬停态。
    pub const ACCENT_HOVER: u32 = 0x1d4ed8;
    /// 浅蓝激活底色（~9% 蓝）。
    pub const ACCENT_SOFT: u32 = 0x2563eb18;
    /// 浅蓝胶囊上的深蓝文字（对比度 >8）。
    pub const ACCENT_TEXT: u32 = 0x1e40af;
    /// 危险 / 逾期红（对白底对比度 6.47）。
    pub const DANGER: u32 = 0xb91c1c;
    /// 逾期红胶囊浅底（~8% 红）。
    pub const DANGER_SOFT: u32 = 0xdc262614;
    /// 正常截止时间胶囊浅底（~3.5% 黑）。
    pub const DUE_BG: u32 = 0x00000009;
    /// 标签胶囊浅蓝底（~7% 蓝）。
    pub const TAG_BG: u32 = 0x2563eb12;
    /// 标签胶囊文字。
    pub const TAG_TEXT: u32 = 0x1d4ed8;
}
