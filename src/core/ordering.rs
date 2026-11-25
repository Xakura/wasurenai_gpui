//! 展示顺序与筛选。纯函数，输入 `Store` 与日期，输出要画的条目引用。

use std::cmp::Ordering;

use chrono::NaiveDate;

use crate::core::model::{Store, TodoItem};

/// 某条待办在指定日期 `today` 是否应显示在待办列表中：
///
/// - 未完成事项：始终显示；
/// - 已完成事项：仅当天完成（`completed_at.date() == today`）才显示；
///   昨日及更早完成的（以及无 `completed_at` 的历史已完成项）不再显示，但保留在 Store 中供日志导出。
pub fn is_visible_on(item: &TodoItem, today: NaiveDate) -> bool {
    !item.done || item.completed_at.is_some_and(|dt| dt.date() == today)
}

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

/// 应用当日可见性与标签筛选后的展示列表。
pub fn visible_on(store: &Store, today: NaiveDate) -> Vec<&TodoItem> {
    let iter = store
        .items
        .iter()
        .filter(|item| is_visible_on(item, today));
    match store.tag_filter.as_deref() {
        Some(tag) => sorted(iter.filter(|item| item.has_tag(tag))),
        None => sorted(iter),
    }
}

/// 指定日期可见待办中出现过的所有标签，去重后按字典序。用于渲染桌面筛选条。
pub fn tags_on(items: &[TodoItem], today: NaiveDate) -> Vec<String> {
    let mut tags: Vec<String> = items
        .iter()
        .filter(|item| is_visible_on(item, today))
        .flat_map(|item| item.tags.iter().cloned())
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// 未完成条数，用于托盘 tooltip 与页签角标。
pub fn pending_count(items: &[TodoItem]) -> usize {
    items.iter().filter(|item| !item.done).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn dt(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
    }

    fn item(
        id: u64,
        done: bool,
        due: Option<NaiveDateTime>,
        tags: &[&str],
        completed_at: Option<NaiveDateTime>,
    ) -> TodoItem {
        TodoItem {
            id,
            text: format!("item{id}"),
            done,
            due,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            created_at: None,
            completed_at,
        }
    }

    fn ids(items: Vec<&TodoItem>) -> Vec<u64> {
        items.into_iter().map(|item| item.id).collect()
    }

    #[test]
    fn pending_items_sort_before_done() {
        let items = [
            item(1, true, None, &[], Some(dt(25, 10))),
            item(2, false, None, &[], None),
            item(3, true, Some(dt(19, 9)), &[], Some(dt(25, 11))),
        ];
        assert_eq!(ids(sorted(items.iter())), vec![2, 1, 3]);
    }

    #[test]
    fn due_items_sort_ascending_and_undated_go_last() {
        let items = [
            item(1, false, None, &[], None),
            item(2, false, Some(dt(20, 18)), &[], None),
            item(3, false, Some(dt(19, 9)), &[], None),
        ];
        assert_eq!(ids(sorted(items.iter())), vec![3, 2, 1]);
    }

    #[test]
    fn done_items_ignore_due_and_sort_by_id() {
        let items = [
            item(3, true, Some(dt(19, 9)), &[], Some(dt(25, 9))),
            item(1, true, None, &[], Some(dt(25, 10))),
            item(2, true, Some(dt(20, 9)), &[], Some(dt(25, 11))),
        ];
        assert_eq!(ids(sorted(items.iter())), vec![1, 2, 3]);
    }

    #[test]
    fn visible_on_hides_yesterday_and_legacy_done_items() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let store = Store {
            items: vec![
                item(1, false, None, &[], None),                     // 未完成 -> 显示
                item(2, true, None, &[], Some(dt(25, 14))),          // 今日完成 -> 显示
                item(3, true, None, &[], Some(dt(24, 23))),          // 昨日完成 -> 隐藏
                item(4, true, None, &[], None),                      // 历史无完成时间 -> 隐藏
            ],
            ..Default::default()
        };
        assert_eq!(ids(visible_on(&store, today)), vec![1, 2]);
    }

    #[test]
    fn tag_filter_matches_any_of_multiple_tags() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let mut store = Store {
            items: vec![
                item(1, false, None, &["工作", "研发"], None),
                item(2, false, None, &["生活"], None),
                item(3, false, None, &[], None),
            ],
            ..Default::default()
        };

        store.tag_filter = Some("研发".into());
        assert_eq!(ids(visible_on(&store, today)), vec![1]);

        store.tag_filter = None;
        assert_eq!(ids(visible_on(&store, today)), vec![1, 2, 3]);
    }

    #[test]
    fn tags_on_excludes_yesterday_done_items() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let items = [
            item(1, false, None, &["生活", "工作"], None),
            item(2, true, None, &["今日"], Some(dt(25, 12))),
            item(3, true, None, &["昨日"], Some(dt(24, 18))),
        ];
        assert_eq!(
            tags_on(&items, today),
            vec!["今日".to_string(), "工作".to_string(), "生活".to_string()]
        );
    }

    #[test]
    fn pending_count_ignores_done() {
        let items = [
            item(1, false, None, &[], None),
            item(2, true, None, &[], Some(dt(25, 10))),
            item(3, false, None, &[], None),
        ];
        assert_eq!(pending_count(&items), 2);
    }
}
