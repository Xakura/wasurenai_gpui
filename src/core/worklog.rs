//! 已完成工作日志：按时间顺序自动生成 Markdown 并原子落盘。
//!
//! 数据源直接来自 `Store.items` 中所有 `done == true` 的条目：
//! - 勾选完成 → 自动写入日志文件；
//! - 误勾取消或修改文案/标签 → 重新生成覆盖，不留幽灵记录。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::core::model::TodoItem;
use crate::core::store;

const LOG_FILENAME: &str = "completed_log.md";

/// 默认自动日志文件路径：`%APPDATA%\wasurenai_gpui\completed_log.md`。
pub fn log_path() -> PathBuf {
    log_path_for(&store::data_path())
}

/// 根据 `todos.json` 路径推导同目录的日志文件路径（便于测试隔离）。
pub fn log_path_for(store_path: &Path) -> PathBuf {
    store_path.with_file_name(LOG_FILENAME)
}

/// 按完成日期分组的已完成记录。
pub struct DayGroup<'a> {
    /// `None` 表示升级前已完成、缺少 `completed_at` 的历史记录。
    pub date: Option<NaiveDate>,
    pub items: Vec<&'a TodoItem>,
}

/// 收集所有已完成事项并按日期分组。
///
/// - 组内一律按 `(completed_at, id)` 升序（时间顺序）。
/// - `newest_first = false`：文件导出用，按时间顺序（更早完成 → 旧日期 → 新日期）。
/// - `newest_first = true`：界面预览用，最近日期置顶（新日期 → 旧日期 → 更早完成）。
pub fn completed_groups(items: &[TodoItem], newest_first: bool) -> Vec<DayGroup<'_>> {
    let mut dated: BTreeMap<NaiveDate, Vec<&TodoItem>> = BTreeMap::new();
    let mut undated: Vec<&TodoItem> = Vec::new();

    for item in items.iter().filter(|item| item.done) {
        if let Some(dt) = item.completed_at {
            dated.entry(dt.date()).or_default().push(item);
        } else {
            undated.push(item);
        }
    }

    for list in dated.values_mut() {
        list.sort_by_key(|item| (item.completed_at, item.id));
    }
    undated.sort_by_key(|item| item.id);

    let mut groups = Vec::with_capacity(dated.len() + usize::from(!undated.is_empty()));
    if newest_first {
        for (date, list) in dated.into_iter().rev() {
            groups.push(DayGroup {
                date: Some(date),
                items: list,
            });
        }
        if !undated.is_empty() {
            groups.push(DayGroup {
                date: None,
                items: undated,
            });
        }
    } else {
        if !undated.is_empty() {
            groups.push(DayGroup {
                date: None,
                items: undated,
            });
        }
        for (date, list) in dated {
            groups.push(DayGroup {
                date: Some(date),
                items: list,
            });
        }
    }
    groups
}

/// 渲染为按时间顺序排列的 Markdown 日志。
pub fn render_markdown(items: &[TodoItem]) -> String {
    let groups = completed_groups(items, false);
    let mut out = String::from("# Wasurenai 已完成工作日志\n");

    if groups.is_empty() {
        out.push_str("\n暂无已完成的工作记录。\n");
        return out;
    }

    for group in groups {
        out.push('\n');
        match group.date {
            Some(date) => {
                let _ = writeln!(out, "## {}", date.format("%Y-%m-%d"));
            }
            None => {
                out.push_str("## 更早完成\n");
            }
        }
        out.push('\n');

        for item in group.items {
            out.push_str("- ");
            if let Some(dt) = item.completed_at {
                let _ = write!(out, "[{}] ", dt.format("%H:%M"));
            }
            out.push_str(&item.text);
            for tag in &item.tags {
                let _ = write!(out, " `#{tag}`");
            }
            out.push('\n');
        }
    }

    out
}

/// 将已完成工作日志原子写入目标路径（先写 `.tmp` 再 `rename`）。
pub fn sync_to(items: &[TodoItem], path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = render_markdown(items);
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "wasurenai_worklog_{name}_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&base);
            fs::create_dir_all(&base).expect("mkdir temp");
            Self(base)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn dt(day: u32, hour: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, min, 0)
            .unwrap()
    }

    fn done_item(id: u64, text: &str, tags: &[&str], completed_at: Option<NaiveDateTime>) -> TodoItem {
        TodoItem {
            id,
            text: text.to_string(),
            done: true,
            due: None,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            created_at: None,
            completed_at,
        }
    }

    #[test]
    fn render_markdown_empty_state() {
        let md = render_markdown(&[]);
        assert!(md.contains("# Wasurenai 已完成工作日志"));
        assert!(md.contains("暂无已完成的工作记录"));
    }

    #[test]
    fn render_markdown_chronological_order_and_tags() {
        let items = vec![
            TodoItem::new(1, "未完成事项不应导出"),
            done_item(2, "下午提交代码", &["研发", "后端"], Some(dt(25, 16, 40))),
            done_item(3, "上午开会", &["工作"], Some(dt(25, 9, 30))),
            done_item(4, "昨日整理文档", &[], Some(dt(24, 18, 5))),
            done_item(5, "历史遗留完成项", &["历史"], None),
        ];

        let md = render_markdown(&items);
        let expected = "\
# Wasurenai 已完成工作日志

## 更早完成

- 历史遗留完成项 `#历史`

## 2026-11-24

- [18:05] 昨日整理文档

## 2026-11-25

- [09:30] 上午开会 `#工作`
- [16:40] 下午提交代码 `#研发` `#后端`
";
        assert_eq!(md, expected);
    }

    #[test]
    fn completed_groups_newest_first_orders_descending_dates() {
        let items = vec![
            done_item(1, "旧", &[], Some(dt(24, 10, 0))),
            done_item(2, "新", &[], Some(dt(25, 10, 0))),
            done_item(3, "无日期", &[], None),
        ];
        let groups = completed_groups(&items, true);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].date, NaiveDate::from_ymd_opt(2026, 11, 25));
        assert_eq!(groups[1].date, NaiveDate::from_ymd_opt(2026, 11, 24));
        assert_eq!(groups[2].date, None);
    }

    #[test]
    fn sync_to_writes_atomically() {
        let dir = TempDir::new("atomic");
        let path = dir.0.join("completed_log.md");
        let items = vec![done_item(1, "完成任务", &["工作"], Some(dt(25, 11, 0)))];

        sync_to(&items, &path).expect("sync_to");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("- [11:00] 完成任务 `#工作`"));
        assert!(!path.with_extension("md.tmp").exists());
    }
}
