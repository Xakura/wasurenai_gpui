//! 展示顺序与筛选。纯函数，输入 `Store`，输出要画的条目引用。

use std::cmp::Ordering;

use crate::core::model::{Store, TodoItem};

/// 展示顺序：
///
/// 1. 未完成在前，已完成沉底；
/// 2. 未完成里，有截止时间的按时间升序排在前面，没有截止时间的按 id 排在后面；
/// 3. 已完成内部只按 id 升序（已完成条目不再看截止时间）。
pub fn sorted<'a>(items: impl IntoIterator<Item = &'a TodoItem>) -> Vec<&'a TodoItem> {
    let mut items: Vec<&TodoItem> = items.into_iter().collect();
    items.sort_by(|a, b| compare(a, b));
    items
}

/// 单条比较规则，抽出来便于直接测。
pub fn compare(a: &TodoItem, b: &TodoItem) -> Ordering {
    a.done
        .cmp(&b.done)
        .then_with(|| {
            // 已完成条目不再参与截止时间排序
            if a.done {
                Ordering::Equal
            } else {
                match (a.due, b.due) {
                    (Some(left), Some(right)) => left.cmp(&right),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                }
            }
        })
        .then_with(|| a.id.cmp(&b.id))
}

/// 应用标签筛选后的展示列表。
pub fn visible(store: &Store) -> Vec<&TodoItem> {
    match store.tag_filter.as_deref() {
        Some(tag) => sorted(store.items.iter().filter(|item| match &item.tag {
            Some(item_tag) => item_tag == tag,
            None => false,
        })),
        None => sorted(store.items.iter()),
    }
}

/// 出现过的所有标签，去重后按字典序。用于渲染筛选条。
pub fn tags(items: &[TodoItem]) -> Vec<String> {
    let mut tags: Vec<String> = items.iter().filter_map(|item| item.tag.clone()).collect();
    tags.sort();
    tags.dedup();
    tags
}

/// 未完成条数，用于托盘 tooltip。
pub fn pending_count(items: &[TodoItem]) -> usize {
    items.iter().filter(|item| !item.done).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime};

    fn dt(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
    }

    fn item(id: u64, done: bool, due: Option<NaiveDateTime>, tag: Option<&str>) -> TodoItem {
        TodoItem {
            id,
            text: format!("item{id}"),
            done,
            due,
            tag: tag.map(str::to_string),
        }
    }

    fn ids(items: Vec<&TodoItem>) -> Vec<u64> {
        items.into_iter().map(|item| item.id).collect()
    }

    #[test]
    fn pending_items_sort_before_done() {
        let items = [
            item(1, true, None, None),
            item(2, false, None, None),
            item(3, true, Some(dt(19, 9)), None),
        ];
        assert_eq!(ids(sorted(items.iter())), vec![2, 1, 3]);
    }

    #[test]
    fn due_items_sort_ascending_and_undated_go_last() {
        let items = [
            item(1, false, None, None),
            item(2, false, Some(dt(20, 18)), None),
            item(3, false, Some(dt(19, 9)), None),
        ];
        assert_eq!(ids(sorted(items.iter())), vec![3, 2, 1]);
    }

    #[test]
    fn done_items_ignore_due_and_sort_by_id() {
        let items = [
            item(3, true, Some(dt(19, 9)), None),
            item(1, true, None, None),
            item(2, true, Some(dt(20, 9)), None),
        ];
        assert_eq!(ids(sorted(items.iter())), vec![1, 2, 3]);
    }

    #[test]
    fn same_due_sorted_by_id() {
        let items = [
            item(9, false, Some(dt(20, 18)), None),
            item(4, false, Some(dt(20, 18)), None),
        ];
        assert_eq!(ids(sorted(items.iter())), vec![4, 9]);
    }

    #[test]
    fn tag_filter_only_keeps_matching_tag() {
        let mut store = Store {
            items: vec![
                item(1, false, None, Some("工作")),
                item(2, false, None, Some("生活")),
                item(3, false, None, None),
            ],
            ..Default::default()
        };

        store.tag_filter = Some("工作".into());
        assert_eq!(ids(visible(&store)), vec![1]);

        store.tag_filter = None;
        assert_eq!(ids(visible(&store)), vec![1, 2, 3]);
    }

    #[test]
    fn tag_filter_with_no_match_returns_empty() {
        let store = Store {
            items: vec![item(1, false, None, Some("工作"))],
            tag_filter: Some("不存在".into()),
            ..Default::default()
        };
        assert!(visible(&store).is_empty());
    }

    #[test]
    fn tags_are_unique_and_sorted() {
        let items = [
            item(1, false, None, Some("生活")),
            item(2, false, None, Some("工作")),
            item(3, false, None, Some("工作")),
            item(4, false, None, None),
        ];
        assert_eq!(tags(&items), vec!["工作".to_string(), "生活".to_string()]);
    }

    #[test]
    fn pending_count_ignores_done() {
        let items = [
            item(1, false, None, None),
            item(2, true, None, None),
            item(3, false, None, None),
        ];
        assert_eq!(pending_count(&items), 2);
    }
}
