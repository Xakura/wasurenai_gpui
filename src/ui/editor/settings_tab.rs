//! 编辑窗口「设置」页签：工作标签管理、全局快捷键、开机自启与本地数据路径。

use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::ui::theme::editor as palette;

use super::components::secondary_button;
use super::{EditorWindow, PAD_X};

impl EditorWindow {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_settings(
        &mut self,
        cx: &mut Context<Self>,
        predefined_tags: &[String],
        hotkey: &str,
        recording: bool,
        note: Option<String>,
        data_path: &str,
        log_path: &str,
    ) -> AnyElement {
        let hotkey_controls = if recording {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .px(px(10.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .bg(rgba(palette::ACCENT_SOFT))
                        .border_1()
                        .border_color(rgb(palette::ACCENT))
                        .text_size(px(12.0))
                        .text_color(rgb(palette::ACCENT_TEXT))
                        .child("请按下新组合键… (Esc 取消)"),
                )
                .child(secondary_button(
                    "hotkey-cancel",
                    "取消",
                    cx.listener(|this, _, _window, cx| this.cancel_recording(cx)),
                ))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(render_keycaps(hotkey))
                .child(secondary_button(
                    "hotkey-record",
                    "修改",
                    cx.listener(|this, _, window, cx| this.start_recording(window, cx)),
                ))
                .child(
                    div()
                        .id("hotkey-reset")
                        .px(px(8.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .text_size(px(12.0))
                        .text_color(rgb(palette::MUTED))
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(rgba(palette::HOVER))
                                .text_color(rgb(palette::TEXT))
                        })
                        .child("恢复默认")
                        .on_click(cx.listener(|this, _, _window, cx| this.reset_hotkey(cx))),
                )
                .into_any_element()
        };

        let autostart_enabled = self.autostart_enabled;
        let autostart_error = self.autostart_error.clone();
        let tag_note = self.tag_note.clone();

        div()
            .id("settings-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(PAD_X))
            .py(px(22.0))
            .items_center()
            .child(
                div()
                    .flex_shrink_0()
                    .w_full()
                    .max_w(px(680.0))
                    .flex()
                    .flex_col()
                    .gap(px(18.0))
                    // 卡片 1：工作标签管理
                    .child(settings_section(
                        "工作标签管理",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(setting_label(
                                "预定义工作标签",
                                "在此预设常用标签；新增或编辑待办时可直接多选勾选（删除预设不影响历史日志中的标签）",
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .px(px(10.0))
                                            .py(px(3.0))
                                            .rounded(px(7.0))
                                            .bg(rgb(palette::SURFACE_SUBTLE))
                                            .border_1()
                                            .border_color(rgba(palette::BORDER))
                                            .text_color(rgb(palette::TEXT))
                                            .child(
                                                Input::new(&self.new_tag_input)
                                                    .appearance(false)
                                                    .px(px(2.0))
                                                    .py(px(0.0))
                                                    .h(px(26.0))
                                                    .text_size(px(12.5)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("add-predefined-tag-btn")
                                            .flex_none()
                                            .px(px(12.0))
                                            .h(px(32.0))
                                            .rounded(px(7.0))
                                            .bg(rgb(palette::ACCENT))
                                            .hover(|s| s.bg(rgb(palette::ACCENT_HOVER)))
                                            .text_size(px(12.0))
                                            .font_medium()
                                            .text_color(rgb(palette::SURFACE))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .child("添加标签")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.submit_new_predefined_tag(window, cx);
                                            })),
                                    ),
                            )
                            .when_some(tag_note, |card, message| {
                                card.child(
                                    div()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .rounded(px(7.0))
                                        .bg(rgba(palette::DANGER_SOFT))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::DANGER))
                                        .child(message),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap(px(8.0))
                                    .when(predefined_tags.is_empty(), |box_| {
                                        box_.child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(rgb(palette::SUBTLE))
                                                .child("暂无预定义标签，在上方输入名称后按 Enter 添加"),
                                        )
                                    })
                                    .children(predefined_tags.iter().enumerate().map(|(idx, tag)| {
                                        let tag_owned = tag.clone();
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.0))
                                            .pl(px(9.0))
                                            .pr(px(5.0))
                                            .py(px(3.0))
                                            .rounded(px(6.0))
                                            .bg(rgba(palette::TAG_BG))
                                            .border_1()
                                            .border_color(rgba(palette::BORDER))
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .font_medium()
                                                    .text_color(rgb(palette::TAG_TEXT))
                                                    .child(format!("#{tag}")),
                                            )
                                            .child(
                                                div()
                                                    .id(("del-predefined-tag", idx))
                                                    .size(px(18.0))
                                                    .rounded(px(4.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(12.0))
                                                    .text_color(rgb(palette::MUTED))
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(rgba(palette::DANGER_SOFT))
                                                            .text_color(rgb(palette::DANGER))
                                                    })
                                                    .child("×")
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.remove_predefined_tag(
                                                                &tag_owned, cx,
                                                            );
                                                        },
                                                    )),
                                            )
                                    })),
                            )
                            .into_any_element(),
                    ))
                    // 卡片 2：快捷键与桌面面板
                    .child(settings_section(
                        "快捷键与桌面面板",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "唤起 / 收回桌面面板",
                                        "全局快捷键可随时将桌面待办置顶显示或落回底层",
                                    ))
                                    .child(hotkey_controls),
                            )
                            .when_some(note, |card, message| {
                                card.child(
                                    div()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .rounded(px(7.0))
                                        .bg(rgba(palette::DANGER_SOFT))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::DANGER))
                                        .child(message),
                                )
                            })
                            .into_any_element(),
                    ))
                    // 卡片 3：系统集成
                    .child(settings_section(
                        "系统集成",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "开机自动启动",
                                        "登录系统时自动在后台运行并展开桌面面板",
                                    ))
                                    .child(
                                        Switch::new("autostart-switch")
                                            .checked(autostart_enabled)
                                            .on_change(cx.listener(
                                                |this, next: &bool, _window, cx| {
                                                    this.toggle_autostart(*next, cx);
                                                },
                                            )),
                                    ),
                            )
                            .when_some(autostart_error, |card, message| {
                                card.child(
                                    div()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .rounded(px(7.0))
                                        .bg(rgba(palette::DANGER_SOFT))
                                        .text_size(px(11.5))
                                        .text_color(rgb(palette::DANGER))
                                        .child(message),
                                )
                            })
                            .into_any_element(),
                    ))
                    // 卡片 4：数据与日志路径
                    .child(settings_section(
                        "数据与日志",
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .pb(px(12.0))
                                    .border_b_1()
                                    .border_color(rgba(palette::BORDER))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "本地存储路径",
                                        "所有待办与配置均以原子写入保存在本地 JSON 中",
                                    ))
                                    .child(path_badge(data_path)),
                            )
                            .child(
                                div()
                                    .py(px(12.0))
                                    .border_b_1()
                                    .border_color(rgba(palette::BORDER))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "自动日志路径",
                                        "已完成工作自动按时间顺序同步至 Markdown 文件",
                                    ))
                                    .child(path_badge(log_path)),
                            )
                            .child(
                                div()
                                    .pt(px(12.0))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(setting_label(
                                        "截止时间选择",
                                        "点击日历并搭配快捷预设或下方时间段微调，点击 × 一键清除",
                                    ))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(format_chip("日历选日"))
                                            .child(format_chip("时分微调"))
                                            .child(format_chip("快捷预设")),
                                    ),
                            )
                            .into_any_element(),
                    )),
            )
            .into_any_element()
    }
}

fn path_badge(path: &str) -> AnyElement {
    div()
        .max_w(px(340.0))
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(rgb(palette::SURFACE_SUBTLE))
        .border_1()
        .border_color(rgba(palette::BORDER))
        .text_size(px(11.5))
        .text_color(rgb(palette::MUTED))
        .child(path.to_string())
        .into_any_element()
}

fn render_keycaps(hotkey: &str) -> AnyElement {
    let mut row = div().flex().items_center().gap(px(4.0));
    for (idx, part) in hotkey.split('+').enumerate() {
        if idx > 0 {
            row = row.child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(palette::SUBTLE))
                    .child("+"),
            );
        }
        row = row.child(
            div()
                .px(px(8.0))
                .py(px(3.0))
                .rounded(px(6.0))
                .bg(rgb(palette::SURFACE_SUBTLE))
                .border_1()
                .border_b_2()
                .border_color(rgba(palette::BORDER_STRONG))
                .text_size(px(11.5))
                .font_medium()
                .text_color(rgb(palette::TEXT))
                .child(part.trim().to_string()),
        );
    }
    row.into_any_element()
}

fn settings_section(title: &'static str, body: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .px(px(4.0))
                .text_size(px(12.0))
                .font_semibold()
                .text_color(rgb(palette::MUTED))
                .child(title),
        )
        .child(
            div()
                .bg(rgb(palette::SURFACE))
                .rounded(px(12.0))
                .border_1()
                .border_color(rgba(palette::BORDER))
                .p(px(16.0))
                .child(body),
        )
        .into_any_element()
}

fn setting_label(title: &'static str, subtitle: &'static str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(13.5))
                .font_medium()
                .text_color(rgb(palette::TEXT))
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(rgb(palette::SUBTLE))
                .child(subtitle),
        )
        .into_any_element()
}

fn format_chip(label: &'static str) -> AnyElement {
    div()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(6.0))
        .bg(rgb(palette::SURFACE_SUBTLE))
        .border_1()
        .border_color(rgba(palette::BORDER))
        .text_size(px(11.5))
        .text_color(rgb(palette::MUTED))
        .child(label)
        .into_any_element()
}
