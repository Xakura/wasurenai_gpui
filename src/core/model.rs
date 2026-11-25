//! 数据结构。这里只放"数据长什么样"，行为放在 [`crate::core::store`]。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

pub type TodoId = u64;

/// 一条待办。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "TodoItemRaw")]
pub struct TodoItem {
    pub id: TodoId,
    pub text: String,
    #[serde(default)]
    pub done: bool,
    /// 截止时间（本地时间）。JSON 里写作 `"2026-11-20T18:00:00"`，没有就是 `null`。
    #[serde(default)]
    pub due: Option<NaiveDateTime>,
    /// 多标签裸词（不带 `#`，已规范化去重）。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 建立时间（本地时间，不展示给用户，供日志与追溯）。
    #[serde(default)]
    pub created_at: Option<NaiveDateTime>,
    /// 完成时间（本地时间，不展示给用户，用于当日过滤与日志排序）。
    #[serde(default)]
    pub completed_at: Option<NaiveDateTime>,
}

/// 反序列化兼容结构：支持旧版单标签 `"tag": "工作"` 自动迁移到 `tags`。
#[derive(Deserialize)]
struct TodoItemRaw {
    id: TodoId,
    text: String,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    due: Option<NaiveDateTime>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    created_at: Option<NaiveDateTime>,
    #[serde(default)]
    completed_at: Option<NaiveDateTime>,
}

impl From<TodoItemRaw> for TodoItem {
    fn from(raw: TodoItemRaw) -> Self {
        let mut tags = normalize_tags(raw.tags);
        if let Some(legacy) = raw.tag.as_deref().and_then(normalize_tag)
            && !tags.contains(&legacy)
        {
            tags.push(legacy);
        }
        Self {
            id: raw.id,
            text: raw.text,
            done: raw.done,
            due: raw.due,
            tags,
            created_at: raw.created_at,
            completed_at: raw.completed_at,
        }
    }
}

impl TodoItem {
    /// 创建一条新待办（无建立时间；业务入口优先使用 [`Self::new_at`]）。`text` 会被 trim。
    pub fn new(id: TodoId, text: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into().trim().to_string(),
            done: false,
            due: None,
            tags: Vec::new(),
            created_at: None,
            completed_at: None,
        }
    }

    /// 创建一条带建立时间的新待办。
    pub fn new_at(id: TodoId, text: impl Into<String>, created_at: NaiveDateTime) -> Self {
        let mut item = Self::new(id, text);
        item.created_at = Some(created_at);
        item
    }

    /// 是否已逾期（未完成 且 有截止时间 且 截止时间 <= now）。
    pub fn is_overdue(&self, now: NaiveDateTime) -> bool {
        !self.done && self.due.is_some_and(|due| due <= now)
    }

    /// 是否包含指定标签。
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// 落盘的整个应用状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default = "default_next_id")]
    pub next_id: TodoId,
    #[serde(default)]
    pub items: Vec<TodoItem>,
    /// 预定义工作标签（用户在设置中管理，新建/编辑待办时勾选）。
    #[serde(default)]
    pub predefined_tags: Vec<String>,
    /// 桌面层窗口是否展开。
    #[serde(default = "default_true")]
    pub widget_visible: bool,
    /// 标签筛选，`None` = 全部。
    #[serde(default)]
    pub tag_filter: Option<String>,
    /// 全局快捷键（规范字符串，如 `Ctrl+Alt+Shift+W`）。`None` = 用默认值。
    #[serde(default)]
    pub hotkey: Option<String>,
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
            predefined_tags: Vec::new(),
            widget_visible: true,
            tag_filter: None,
            hotkey: None,
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

/// 批量规范化并保序去重标签列表。
pub fn normalize_tags<I, S>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Vec::new();
    for item in raw {
        if let Some(tag) = normalize_tag(item.as_ref())
            && !out.contains(&tag)
        {
            out.push(tag);
        }
    }
    out
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
        assert!(item.tags.is_empty());
        assert_eq!(item.created_at, None);
        assert_eq!(item.completed_at, None);
    }

    #[test]
    fn normalize_tag_strips_hash_and_blank() {
        assert_eq!(normalize_tag("#工作"), Some("工作".into()));
        assert_eq!(normalize_tag("  生活 "), Some("生活".into()));
        assert_eq!(normalize_tag("  #  "), None);
        assert_eq!(normalize_tag(""), None);
    }

    #[test]
    fn normalize_tags_dedups_and_preserves_order() {
        assert_eq!(
            normalize_tags(["#工作", "研发", " 工作 ", "", "  # ", "研发"]),
            vec!["工作".to_string(), "研发".to_string()]
        );
    }

    #[test]
    fn store_default_is_visible_with_next_id_one() {
        let store = Store::default();
        assert_eq!(store.next_id, 1);
        assert!(store.widget_visible);
        assert!(store.items.is_empty());
        assert!(store.predefined_tags.is_empty());
        assert_eq!(store.tag_filter, None);
    }

    #[test]
    fn json_round_trip_keeps_all_fields() {
        let store = Store {
            predefined_tags: vec!["工作".into(), "研发".into()],
            items: vec![TodoItem {
                id: 1,
                text: "交周报".into(),
                done: true,
                due: Some(dt(2026, 11, 20, 18, 0)),
                tags: vec!["工作".into(), "研发".into()],
                created_at: Some(dt(2026, 11, 20, 9, 0)),
                completed_at: Some(dt(2026, 11, 20, 17, 30)),
            }],
            next_id: 2,
            widget_visible: false,
            tag_filter: Some("工作".into()),
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&store).unwrap();
        assert!(json.contains("2026-11-20T18:00:00"), "时间格式应可读：{json}");
        assert!(json.contains("2026-11-20T17:30:00"), "完成时间应保留：{json}");

        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back, store);
    }

    #[test]
    fn json_migrates_legacy_single_tag_to_tags_vec() {
        let store: Store = serde_json::from_str(
            r##"{ "items": [ { "id": 3, "text": "hi", "tag": "#工作" } ] }"##,
        )
        .expect("旧版单标签 JSON 应能解析");
        assert_eq!(store.next_id, 1);
        assert!(store.widget_visible, "缺省应视为展开");
        assert!(!store.items[0].done);
        assert_eq!(store.items[0].due, None);
        assert_eq!(store.items[0].tags, vec!["工作".to_string()]);
        assert_eq!(store.items[0].created_at, None);
        assert_eq!(store.items[0].completed_at, None);
    }
}
