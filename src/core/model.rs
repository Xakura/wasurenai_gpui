//! 数据结构。这里只放"数据长什么样"，行为放在 [`crate::core::store`]。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

pub type TodoId = u64;

/// 一条待办。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: TodoId,
    pub text: String,
    #[serde(default)]
    pub done: bool,
    /// 截止时间（本地时间）。JSON 里写作 `"2026-11-20T18:00:00"`，没有就是 `null`。
    #[serde(default)]
    pub due: Option<NaiveDateTime>,
    /// 单个标签的裸词（不带 `#`）。没有就是 `null`。
    #[serde(default)]
    pub tag: Option<String>,
}

impl TodoItem {
    /// 创建一条新待办。`text` 会被 trim。
    pub fn new(id: TodoId, text: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into().trim().to_string(),
            done: false,
            due: None,
            tag: None,
        }
    }

    /// 是否已逾期（未完成 且 有截止时间 且 截止时间 <= now）。
    pub fn is_overdue(&self, now: NaiveDateTime) -> bool {
        !self.done && self.due.is_some_and(|due| due <= now)
    }
}

/// 落盘的整个应用状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default = "default_next_id")]
    pub next_id: TodoId,
    #[serde(default)]
    pub items: Vec<TodoItem>,
    /// 桌面层窗口是否展开。
    #[serde(default = "default_true")]
    pub widget_visible: bool,
    /// 标签筛选，`None` = 全部。
    #[serde(default)]
    pub tag_filter: Option<String>,
}

fn default_next_id() -> TodoId {
    1
}

fn default_true() -> bool {
    true
}

impl Default for Store {
    fn default() -> Self {
        Self {
            next_id: default_next_id(),
            items: Vec::new(),
            widget_visible: true,
            tag_filter: None,
        }
    }
}

/// 标签规范化：trim、去掉前导 `#`、空白标签视为没有标签。
pub fn normalize_tag(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches('#').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn is_overdue_requires_due_and_not_done() {
        let now = dt(2026, 11, 19, 12, 0);
        let mut item = TodoItem::new(1, "x");

        assert!(!item.is_overdue(now), "没有截止时间不算逾期");

        item.due = Some(dt(2026, 11, 19, 11, 0));
        assert!(item.is_overdue(now), "已过截止时间算逾期");

        item.due = Some(dt(2026, 11, 19, 13, 0));
        assert!(!item.is_overdue(now), "未到截止时间不算逾期");

        item.due = Some(dt(2026, 11, 19, 11, 0));
        item.done = true;
        assert!(!item.is_overdue(now), "已完成不算逾期");
    }

    #[test]
    fn new_item_trims_text() {
        let item = TodoItem::new(7, "  交周报  ");
        assert_eq!(item.text, "交周报");
        assert_eq!(item.id, 7);
    }

    #[test]
    fn normalize_tag_strips_hash_and_blank() {
        assert_eq!(normalize_tag("#工作"), Some("工作".into()));
        assert_eq!(normalize_tag("  生活 "), Some("生活".into()));
        assert_eq!(normalize_tag("  #  "), None);
        assert_eq!(normalize_tag(""), None);
    }

    #[test]
    fn store_default_is_visible_with_next_id_one() {
        let store = Store::default();
        assert_eq!(store.next_id, 1);
        assert!(store.widget_visible);
        assert!(store.items.is_empty());
        assert_eq!(store.tag_filter, None);
    }

    #[test]
    fn json_round_trip_keeps_all_fields() {
        let mut store = Store::default();
        store.items.push(TodoItem {
            id: 1,
            text: "交周报".into(),
            done: true,
            due: Some(dt(2026, 11, 20, 18, 0)),
            tag: Some("工作".into()),
        });
        store.next_id = 2;
        store.widget_visible = false;
        store.tag_filter = Some("工作".into());

        let json = serde_json::to_string_pretty(&store).unwrap();
        assert!(json.contains("2026-11-20T18:00:00"), "时间格式应可读：{json}");

        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back, store);
    }

    #[test]
    fn json_defaults_fill_missing_fields() {
        // 手写的最小 JSON 也能解析（向后兼容/容错）
        let store: Store = serde_json::from_str(r#"{ "items": [ { "id": 3, "text": "hi" } ] }"#)
            .expect("最小 JSON 应能解析");
        assert_eq!(store.next_id, 1);
        assert!(store.widget_visible, "缺省应视为展开");
        assert!(!store.items[0].done);
        assert_eq!(store.items[0].due, None);
    }
}
