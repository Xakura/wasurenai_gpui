//! 截止时间的文本解析与格式化。
//!
//! 编辑器里用手输时间（而不是日期选择器）：能少一堆组件依赖，输入也更快。
//! 为了不让人记格式，这里接受若干常见写法；解析不出来就让 UI 标红提示。
//!
//! 纯函数（`today` 作为参数传进来），因此可单测。

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

/// 输入框里显示的时间格式。
pub const DISPLAY_FORMAT: &str = "%Y-%m-%d %H:%M";

/// 把用户输入解析成截止时间。空输入 = 清除（`Ok(None)`）。
///
/// 接受：
/// - `2026-11-20 18:00` / `2026-11-20 18:00:00` / `2026-11-20T18:00`
/// - `2026-11-20`（当天 00:00）
/// - `11-20 18:00`（当年）
/// - `18:00`（今天）
pub fn parse_due(input: &str, today: NaiveDate) -> Result<Option<NaiveDateTime>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }

    // 折叠多余空白；再试一次去掉全部空白的写法（容忍「19: 20」这类手滑）
    let collapsed = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let no_spaces: String = collapsed.chars().filter(|c| !c.is_whitespace()).collect();

    if let Some(value) = parse_candidates(&collapsed, today).or_else(|| parse_candidates(&no_spaces, today)) {
        return Ok(Some(value));
    }

    Err(format!(
        "看不懂「{collapsed}」，试试 2026-11-20 18:00 / 11-20 18:00 / 18:00"
    ))
}

fn parse_candidates(input: &str, today: NaiveDate) -> Option<NaiveDateTime> {
    for format in [
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y/%m/%d %H:%M",
    ] {
        if let Ok(value) = NaiveDateTime::parse_from_str(input, format) {
            return Some(value);
        }
    }

    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Some(date.and_time(NaiveTime::MIN));
    }

    if let Ok(value) = NaiveDateTime::parse_from_str(
        &format!("{}-{}", today.year_str(), input),
        "%Y-%m-%d %H:%M",
    ) {
        return Some(value);
    }

    if let Ok(time) = NaiveTime::parse_from_str(input, "%H:%M") {
        return Some(today.and_time(time));
    }

    None
}

/// 把截止时间格式化成输入框里的文本。
pub fn format_due(due: Option<NaiveDateTime>) -> String {
    match due {
        Some(due) => due.format(DISPLAY_FORMAT).to_string(),
        None => String::new(),
    }
}

/// `NaiveDate` 的年份字符串（避免为了一行格式化引入额外依赖）。
trait YearStr {
    fn year_str(&self) -> String;
}

impl YearStr for NaiveDate {
    fn year_str(&self) -> String {
        use chrono::Datelike as _;
        format!("{:04}", self.year())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
    }

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn empty_input_clears_due() {
        assert_eq!(parse_due("   ", today()), Ok(None));
    }

    #[test]
    fn full_date_time_formats() {
        assert_eq!(parse_due("2026-11-20 18:00", today()), Ok(Some(at(20, 18, 0))));
        assert_eq!(parse_due("2026-11-20 18:00:00", today()), Ok(Some(at(20, 18, 0))));
        assert_eq!(parse_due("2026-11-20T18:00", today()), Ok(Some(at(20, 18, 0))));
        assert_eq!(parse_due("2026/11/20 18:00", today()), Ok(Some(at(20, 18, 0))));
        assert_eq!(parse_due("  2026-11-20 18:00  ", today()), Ok(Some(at(20, 18, 0))));
    }

    #[test]
    fn date_only_means_midnight() {
        assert_eq!(parse_due("2026-11-20", today()), Ok(Some(at(20, 0, 0))));
    }

    #[test]
    fn month_day_uses_current_year() {
        assert_eq!(parse_due("11-20 18:00", today()), Ok(Some(at(20, 18, 0))));
    }

    #[test]
    fn time_only_uses_today() {
        assert_eq!(parse_due("18:00", today()), Ok(Some(at(19, 18, 0))));
    }

    #[test]
    fn garbage_is_rejected_with_hint() {
        let err = parse_due("明天下午", today()).expect_err("应解析失败");
        assert!(err.contains("看不懂"), "错误信息应引导用户：{err}");
    }

    #[test]
    fn extra_spaces_are_tolerated() {
        assert_eq!(parse_due(" 19: 20 ", today()), Ok(Some(at(19, 19, 20))));
        assert_eq!(
            parse_due("2026-11-20  18:  00", today()),
            Ok(Some(at(20, 18, 0)))
        );
    }

    #[test]
    fn format_round_trips() {
        let text = format_due(Some(at(20, 18, 30)));
        assert_eq!(text, "2026-11-20 18:30");
        assert_eq!(parse_due(&text, today()), Ok(Some(at(20, 18, 30))));
        assert_eq!(format_due(None), "");
    }
}
