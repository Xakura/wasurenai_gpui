//! 编辑窗口跨页签复用的小组件与日期时间选择器预设。

use chrono::{Datelike as _, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use gpui_kit::component::date_picker::{DatePicker, DatePickerState, DateRangePreset, DateTime};
use gpui_kit::component::time_field::{HourCycle, TimePrecision};
use gpui_kit::component::{Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::ui::theme::editor as palette;

use super::EditorWindow;

const DUE_DISPLAY_FORMAT: &str = "%Y-%m-%d %H:%M";

pub(super) fn checkable_tag_pill(
    id: (&'static str, usize),
    tag: &str,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(6.0))
        .border_1()
        .flex()
        .items_center()
        .gap(px(4.0))
        .text_size(px(11.5))
        .cursor_pointer()
        .when(checked, |el| {
            el.bg(rgba(palette::ACCENT_SOFT))
                .border_color(rgb(palette::ACCENT))
                .font_medium()
                .text_color(rgb(palette::ACCENT_TEXT))
                .child("✓")
        })
        .when(!checked, |el| {
            el.bg(rgb(palette::SURFACE))
                .border_color(rgba(palette::BORDER_STRONG))
                .text_color(rgb(palette::MUTED))
                .hover(|s| {
                    s.border_color(rgb(palette::ACCENT))
                        .text_color(rgb(palette::TEXT))
                })
                .child("#")
        })
        .child(tag.to_string())
        .on_click(on_click)
        .into_any_element()
}

pub(super) fn count_badge(count: usize, accent: bool) -> AnyElement {
    div()
        .px(px(7.0))
        .py(px(1.0))
        .rounded(px(10.0))
        .bg(if accent {
            rgba(palette::ACCENT_SOFT)
        } else {
            rgba(palette::DUE_BG)
        })
        .text_size(px(11.0))
        .font_medium()
        .text_color(if accent {
            rgb(palette::ACCENT_TEXT)
        } else {
            rgb(palette::MUTED)
        })
        .child(count.to_string())
        .into_any_element()
}

pub(super) fn secondary_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px(px(12.0))
        .py(px(5.0))
        .rounded(px(7.0))
        .bg(rgb(palette::SURFACE))
        .border_1()
        .border_color(rgba(palette::BORDER_STRONG))
        .text_size(px(12.0))
        .font_medium()
        .text_color(rgb(palette::TEXT))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(palette::SURFACE_SUBTLE)))
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

pub(super) fn build_due_picker(
    initial: Option<NaiveDateTime>,
    window: &mut Window,
    cx: &mut Context<EditorWindow>,
) -> Entity<DatePickerState> {
    cx.new(|cx| {
        let mut picker = DatePickerState::new(window, cx)
            .date_format(DUE_DISPLAY_FORMAT)
            .time_precision(TimePrecision::Minute)
            .hour_cycle(HourCycle::H23)
            .default_time(NaiveTime::from_hms_opt(18, 0, 0).expect("valid default time"))
            .first_day_of_week(Weekday::Mon);
        if let Some(dt) = initial {
            picker.set_date_time(DateTime::Single(Some(dt)), window, cx);
        }
        picker
    })
}

pub(super) fn render_due_picker(
    state: &Entity<DatePickerState>,
    presets: &[DateRangePreset],
) -> DatePicker {
    DatePicker::new(state)
        .appearance(false)
        .small()
        .cleanable(true)
        .placeholder("截止时间")
        .presets(presets.to_vec())
}

pub(super) fn due_presets(today: NaiveDate) -> Vec<DateRangePreset> {
    due_preset_specs(today)
        .into_iter()
        .map(|(label, dt)| DateRangePreset::date_time(label, dt))
        .collect()
}

fn due_preset_specs(today: NaiveDate) -> Vec<(&'static str, NaiveDateTime)> {
    let at = |date: NaiveDate, h: u32, m: u32| {
        date.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid preset time"))
    };
    let tomorrow = today + Duration::days(1);
    let days_until_next_monday = 7 - i64::from(today.weekday().num_days_from_monday());
    let next_monday = today + Duration::days(days_until_next_monday);
    vec![
        ("今天 18:00", at(today, 18, 0)),
        ("今天 21:00", at(today, 21, 0)),
        ("明天 09:00", at(tomorrow, 9, 0)),
        ("明天 18:00", at(tomorrow, 18, 0)),
        ("下周一 09:00", at(next_monday, 9, 0)),
    ]
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime};

    use super::due_preset_specs;

    #[test]
    fn due_presets_cover_today_tomorrow_and_next_monday() {
        // 2026-11-22 是周日，下周一应为 2026-11-23
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let specs = due_preset_specs(sunday);
        assert_eq!(specs.len(), 5);
        assert_eq!(
            specs[0],
            (
                "今天 18:00",
                sunday.and_time(NaiveTime::from_hms_opt(18, 0, 0).unwrap())
            )
        );
        assert_eq!(
            specs[4],
            (
                "下周一 09:00",
                NaiveDate::from_ymd_opt(2026, 11, 23)
                    .unwrap()
                    .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
            )
        );

        // 2026-11-23 是周一，下周一应为 2026-11-30（+7 天）
        let monday = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        let specs = due_preset_specs(monday);
        assert_eq!(
            specs[4].1.date(),
            NaiveDate::from_ymd_opt(2026, 11, 30).unwrap()
        );
    }
}
