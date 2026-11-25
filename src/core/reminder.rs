//! 到期提醒的判定。纯函数，不涉及 UI、不做定时。
//!
//! 触发规则（与 `docs/plan.md` 的「到点强制置顶，必须手动关」一致）：
//! 未完成 且 有截止时间 且 已到期 且 本次运行还没被「知道了」确认过。

use std::collections::HashSet;

use chrono::NaiveDateTime;

use crate::core::model::{Store, TodoId, TodoItem};

/// 需要弹出的提醒条目，按截止时间升序（越早到期越靠前）。
pub fn due_items<'a>(
    store: &'a Store,
    now: NaiveDateTime,
    acked: &HashSet<TodoId>,
) -> Vec<&'a TodoItem> {
    let mut items: Vec<&TodoItem> = store
        .items
        .iter()
        .filter(|item| item.is_overdue(now) && !acked.contains(&item.id))
        .collect();
    items.sort_by_key(|item| (item.due, item.id));
    items
}

/// 是否有需要弹出的提醒（省去调用方判断空 vec 的样板代码）。
pub fn has_due(store: &Store, now: NaiveDateTime, acked: &HashSet<TodoId>) -> bool {
    store
        .items
        .iter()
        .any(|item| item.is_overdue(now) && !acked.contains(&item.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
    }

    fn item(id: u64, done: bool, due: Option<NaiveDateTime>) -> TodoItem {
        TodoItem {
            id,
            text: format!("item{id}"),
            done,
            due,
            tags: Vec::new(),
            created_at: None,
            completed_at: None,
        }
    }

    fn ids(items: Vec<&TodoItem>) -> Vec<u64> {
        items.into_iter().map(|item| item.id).collect()
    }

    #[test]
    fn only_overdue_pending_items_are_due() {
        let now = dt(19, 12);
        let store = Store {
            items: vec![
                item(1, false, Some(dt(19, 11))), // 到期
                item(2, false, Some(dt(19, 13))), // 还没到
                item(3, true, Some(dt(19, 11))),  // 已完成
                item(4, false, None),             // 没有截止时间
            ],
            ..Default::default()
        };

        assert_eq!(ids(due_items(&store, now, &HashSet::new())), vec![1]);
    }

    #[test]
    fn acked_items_are_skipped() {
        let now = dt(19, 12);
        let store = Store {
            items: vec![item(1, false, Some(dt(19, 11))), item(2, false, Some(dt(19, 10)))],
            ..Default::default()
        };

        let acked: HashSet<TodoId> = [2u64].into_iter().collect();
        assert_eq!(ids(due_items(&store, now, &acked)), vec![1]);
        assert!(has_due(&store, now, &acked));
    }

    #[test]
    fn due_items_sorted_by_due_time() {
        let now = dt(19, 12);
        let store = Store {
            items: vec![
                item(1, false, Some(dt(19, 10))),
                item(2, false, Some(dt(18, 10))),
                item(3, false, Some(dt(19, 11))),
            ],
            ..Default::default()
        };

        assert_eq!(ids(due_items(&store, now, &HashSet::new())), vec![2, 1, 3]);
    }

    #[test]
    fn nothing_due_returns_false() {
        let now = dt(19, 12);
        let store = Store {
            items: vec![item(1, false, Some(dt(19, 13)))],
            ..Default::default()
        };

        assert!(!has_due(&store, now, &HashSet::new()));
        assert!(due_items(&store, now, &HashSet::new()).is_empty());
    }
}
