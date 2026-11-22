//! 桌面层 / 置顶层共用的待办视图。
//!
//! 同一个窗口有两种内容：
//! - **面板**：标签筛选条 + 全部待办（平常沉在桌面层）。勾选已完成时先原地停留 220ms
//!   防连点误触，再用基于真实行高的 FLIP 位移动画平滑下移到底部已完成区。
//! - **提醒**：有到期未确认的条目时自动切换过来，并让窗口上浮到最上层；
//!   逐条「知道了」/「全部知道了」/ 勾选完成后自动回到面板并回落桌面层。
//!
//! 这个窗口是"裸窗口"（没有 `gpui_base::Root`），所以不能依赖 Root 提供的
//! Tooltip / Dialog / Popover / Notification；勾选框用 [`crate::ui::widgets::tick_box`] 手绘。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use chrono::NaiveDateTime;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::core::model::TodoId;
use crate::core::ordering;
use crate::platform::win_layer;
use crate::ui::state::AppState;
use crate::ui::theme::panel;
use crate::ui::widgets::tick_box;
use crate::ui::windows::hwnd_of;

const ROW_GAP: f32 = 2.0;
const DEFAULT_ROW_HEIGHT: f32 = 30.0;
/// 勾选后原地停留时间，避免手快连点导致下一行直接顶到鼠标下误触。
const HOLD_DURATION: Duration = Duration::from_millis(220);
/// 列表重排滑移动画时长。
const SLIDE_DURATION: Duration = Duration::from_millis(320);

/// 面板整窗的过渡动效类型。
///
/// - 收起/展开：横向抽屉缩入/弹出（`Expand` / `Collapse`）
/// - Top/Bottom 层级切换：垂直浮沉 + 淡入淡出（`FloatIn` / `DropFadeOut` / `SettleFadeIn`）
/// - `Hidden`：全透明静止态（隐藏期间及 `Bottom -> Top` 升层预清帧使用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMotion {
    Idle,
    Hidden,
    Expand,
    Collapse,
    FloatIn,
    DropFadeOut,
    SettleFadeIn,
}

impl PanelMotion {
    fn duration(self) -> Option<Duration> {
        match self {
            Self::Idle | Self::Hidden => None,
            Self::Expand => Some(Duration::from_millis(280)),
            Self::Collapse => Some(Duration::from_millis(240)),
            Self::FloatIn => Some(Duration::from_millis(260)),
            Self::DropFadeOut => Some(Duration::from_millis(180)),
            Self::SettleFadeIn => Some(Duration::from_millis(200)),
        }
    }
}

pub struct WidgetWindow {
    state: Entity<AppState>,
    /// 第一帧做的窗口级初始化（置底）是否已执行。
    initialized: bool,
    /// 勾选后停留期间冻结的展示顺序。
    settling_order: Option<Vec<TodoId>>,
    /// 停留计时器代数，连点多项时合并成一次平移动画。
    settle_generation: u64,
    /// 用于检测标签筛选切换，切换时取消在途停留/动画。
    last_tag_filter: Option<String>,
    /// 每行真实高度缓存（prepaint 阶段采集，用来算多行文本下的准确位移）。
    row_heights: Rc<RefCell<HashMap<TodoId, f32>>>,
    /// 本轮滑移动画每个条目的初始 Y 偏移（`old_y - new_y`）。
    slide_deltas: HashMap<TodoId, f32>,
    /// 正在勾选停留或滑移中的条目（短暂保留浅高亮方便视线追踪）。
    sliding_checked: HashSet<TodoId>,
    /// 动画序列号，保证每次重排使用全新的 `ElementId` 从 `t=0` 播放。
    anim_seq: u64,
    /// 当前整窗过渡动效与序号。
    motion: PanelMotion,
    motion_seq: u64,
}

impl WidgetWindow {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        // store 变化（例如在编辑窗口里改内容）时重画
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        let last_tag_filter = state.read(cx).store.tag_filter.clone();
        Self {
            state,
            initialized: false,
            settling_order: None,
            settle_generation: 0,
            last_tag_filter,
            row_heights: Rc::new(RefCell::new(HashMap::new())),
            slide_deltas: HashMap::new(),
            sliding_checked: HashSet::new(),
            anim_seq: 0,
            motion: PanelMotion::Idle,
            motion_seq: 0,
        }
    }

    /// 播放整窗过渡动效。
    ///
    /// 入场动效播完自动归回 `Idle`；退场动效（`Collapse` / `DropFadeOut`）保持末帧
    /// 透明度 0，交由 `windows.rs` 在隐藏或切层后再复位，避免结束瞬间闪回 1 帧。
    pub fn play_motion(&mut self, motion: PanelMotion, cx: &mut Context<Self>) {
        self.motion = motion;
        self.motion_seq += 1;
        let seq = self.motion_seq;

        if matches!(
            motion,
            PanelMotion::Expand | PanelMotion::FloatIn | PanelMotion::SettleFadeIn
        ) && let Some(dur) = motion.duration()
        {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(dur + Duration::from_millis(40))
                    .await;
                this.update(cx, |view, cx| {
                    if view.motion_seq == seq {
                        view.motion = PanelMotion::Idle;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }

        cx.notify();
    }

    /// 切到全透明静止态：收起隐藏期间或 `Bottom -> Top` 升层前先向交换链写入透明帧，
    /// 避免旧的不透明画面在 `HWND_TOPMOST` / `ShowWindow` 瞬间闪现 1 帧。
    pub fn set_hidden(&mut self, cx: &mut Context<Self>) {
        self.motion = PanelMotion::Hidden;
        self.motion_seq += 1;
        cx.notify();
    }

    /// 首帧：接管窗口（owner + 子类化 + 压到桌面层）。
    fn init_window(&mut self, window: &Window) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        let Some(hwnd) = hwnd_of(window) else {
            eprintln!("[wasurenai] 拿不到窗口 HWND，置底失败");
            return;
        };
        let ok = win_layer::attach(hwnd);
        println!(
            "[wasurenai] 桌面层接管={ok} 子类化={} hwnd={hwnd:?}",
            win_layer::subclassed()
        );
    }

    fn current_panel_order(&self, cx: &App) -> Vec<TodoId> {
        let app = self.state.read(cx);
        let visible_ids: Vec<TodoId> = ordering::visible(&app.store)
            .into_iter()
            .map(|item| item.id)
            .collect();
        match &self.settling_order {
            Some(frozen) => apply_frozen_order(frozen, &visible_ids),
            None => visible_ids,
        }
    }

    fn on_toggle_done(&mut self, id: TodoId, next_done: bool, cx: &mut Context<Self>) {
        let now = chrono::Local::now().naive_local();
        let is_reminder = {
            let app = self.state.read(cx);
            !app.due_items(now).is_empty()
        };
        if is_reminder {
            self.state.update(cx, |app, cx| {
                let changed = app.set_done(id, next_done);
                app.commit_click(changed, cx);
            });
            return;
        }

        let old_order = self.current_panel_order(cx);
        let changed = self.state.update(cx, |app, cx| {
            let changed = app.set_done(id, next_done);
            app.commit_click(changed, cx);
            changed
        });
        if !changed {
            return;
        }

        if next_done {
            self.settling_order = Some(old_order);
            self.sliding_checked.insert(id);
            self.slide_deltas.clear();
            self.anim_seq += 1;
            self.settle_generation += 1;
            let generation = self.settle_generation;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(HOLD_DURATION).await;
                this.update(cx, |view, cx| view.finish_settle(generation, cx))
                    .ok();
            })
            .detach();
            cx.notify();
        } else if self.settling_order.is_some() {
            // 在停留期内原地反悔取消勾选：直接撤销高亮，无需任何位置跳动
            self.sliding_checked.remove(&id);
            if self.sliding_checked.is_empty() {
                self.settling_order = None;
                self.settle_generation += 1;
            }
            cx.notify();
        } else {
            self.sliding_checked.remove(&id);
            self.start_slide(old_order, cx);
        }
    }

    fn finish_settle(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.settle_generation != generation {
            return;
        }
        let Some(old_order) = self.settling_order.take() else {
            return;
        };
        self.start_slide(old_order, cx);
    }

    fn start_slide(&mut self, old_order: Vec<TodoId>, cx: &mut Context<Self>) {
        let new_order: Vec<TodoId> = {
            let app = self.state.read(cx);
            ordering::visible(&app.store)
                .into_iter()
                .map(|item| item.id)
                .collect()
        };
        let deltas = {
            let heights = self.row_heights.borrow();
            compute_slide_deltas(&old_order, &new_order, &heights, ROW_GAP)
        };
        self.slide_deltas = deltas;
        self.anim_seq += 1;

        if !self.slide_deltas.is_empty() {
            let seq = self.anim_seq;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(SLIDE_DURATION + Duration::from_millis(50))
                    .await;
                this.update(cx, |view, cx| {
                    if view.anim_seq == seq {
                        view.slide_deltas.clear();
                        view.sliding_checked.clear();
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        } else {
            self.sliding_checked.clear();
        }
        cx.notify();
    }
}

impl Render for WidgetWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.init_window(window);

        let state = self.state.clone();
        let now = chrono::Local::now().naive_local();

        let (reminder, active_tag, tags, pending) = {
            let app = state.read(cx);
            (
                !app.due_items(now).is_empty(),
                app.store.tag_filter.clone(),
                ordering::tags(&app.store.items),
                ordering::pending_count(&app.store.items),
            )
        };

        if reminder || self.last_tag_filter != active_tag {
            self.last_tag_filter = active_tag.clone();
            self.settling_order = None;
            self.slide_deltas.clear();
            self.sliding_checked.clear();
            self.settle_generation += 1;
            self.anim_seq += 1;
        }

        let rows: Vec<Row> = {
            let app = state.read(cx);
            if reminder {
                app.due_items(now)
                    .into_iter()
                    .map(|item| Row::from_item(item, now))
                    .collect()
            } else {
                let sorted = ordering::visible(&app.store);
                let by_id: HashMap<TodoId, &crate::core::model::TodoItem> =
                    sorted.iter().map(|item| (item.id, *item)).collect();
                let ordered_ids = match &self.settling_order {
                    Some(frozen) => {
                        let visible_ids: Vec<TodoId> = sorted.iter().map(|i| i.id).collect();
                        apply_frozen_order(frozen, &visible_ids)
                    }
                    None => sorted.iter().map(|i| i.id).collect(),
                };
                ordered_ids
                    .into_iter()
                    .filter_map(|id| by_id.get(&id).map(|item| Row::from_item(item, now)))
                    .collect()
            }
        };

        let empty = rows.is_empty();
        let view = cx.entity();
        let anim_seq = self.anim_seq;
        let is_top = win_layer::current_layer() == win_layer::Layer::Top;

        let card = div()
            .id("widget-card")
            .relative()
            .size_full()
            .bg(rgba(panel::BG))
            .rounded(px(panel::RADIUS))
            .border_1()
            .border_color(if is_top {
                rgba(panel::BORDER_TOP)
            } else {
                rgba(panel::BORDER)
            })
            .p(px(panel::PADDING))
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(13.0))
            .text_color(rgb(panel::TEXT))
            .child(if reminder {
                reminder_header(state.clone(), rows.len())
            } else {
                panel_header(pending)
            })
            // 标签筛选条只在面板模式下出现
            .when(!reminder && !tags.is_empty(), |body| {
                body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .child(tag_chip(
                            state.clone(),
                            "全部".to_string(),
                            None,
                            active_tag.is_none(),
                        ))
                        .children(tags.into_iter().map(|tag| {
                            let active = active_tag.as_deref() == Some(tag.as_str());
                            tag_chip(state.clone(), tag.clone(), Some(tag), active)
                        })),
                )
            })
            .child(
                div()
                    .id("todo-list")
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .flex_1()
                    .overflow_y_scroll()
                    .when(empty, |list| {
                        list.child(
                            div()
                                .pt(px(6.0))
                                .text_size(px(12.0))
                                .text_color(rgb(panel::MUTED))
                                .child(if reminder {
                                    "没有到期的待办"
                                } else if active_tag.is_some() {
                                    "这个标签下没有待办"
                                } else {
                                    "暂无待办"
                                }),
                        )
                    })
                    .children(rows.into_iter().map(|row| {
                        let id = row.id;
                        let delta = self.slide_deltas.get(&id).copied();
                        let highlighted = self.sliding_checked.contains(&id);
                        todo_row(
                            state.clone(),
                            view.clone(),
                            self.row_heights.clone(),
                            row,
                            reminder,
                            highlighted,
                            delta,
                            anim_seq,
                        )
                    })),
            );

        let body = if let Some(dur) = self.motion.duration() {
            let motion = self.motion;
            let anim_id = ElementId::Name(format!("panel-motion-{}", self.motion_seq).into());
            card.with_animation(anim_id, Animation::new(dur), move |el, progress| {
                let (dx, dy, opacity) = motion_frame(motion, progress);
                el.left(px(dx)).top(px(dy)).opacity(opacity)
            })
            .into_any_element()
        } else if self.motion == PanelMotion::Hidden {
            card.opacity(0.0).into_any_element()
        } else {
            card.into_any_element()
        };

        div().size_full().overflow_hidden().child(body)
    }
}

fn panel_header(pending: usize) -> AnyElement {
    div()
        .flex()
        .items_baseline()
        .gap_2()
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(panel::MUTED))
                .child("待办"),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(panel::ACCENT))
                .child(format!("{pending} 项未完成")),
        )
        .into_any_element()
}

fn reminder_header(state: Entity<AppState>, count: usize) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(panel::DANGER))
                .child(format!("到期提醒 · {count} 项")),
        )
        .child(div().flex_1())
        .child(
            div()
                .id("ack-all")
                .px(px(7.0))
                .py(px(2.0))
                .rounded(px(5.0))
                .bg(rgb(panel::ACCENT))
                .text_size(px(11.0))
                .text_color(rgb(0xffffff))
                .child("全部知道了")
                .on_click(move |_event, _window, cx| {
                    state.update(cx, |app, cx| {
                        app.ack_all(chrono::Local::now().naive_local());
                        app.commit_runtime(true, cx);
                    });
                }),
        )
        .into_any_element()
}

/// 一行要画的数据（从 `Store` 复制出来，避免渲染期间持有借用）。
struct Row {
    id: TodoId,
    text: String,
    done: bool,
    due: Option<NaiveDateTime>,
    overdue: bool,
}

impl Row {
    fn from_item(item: &crate::core::model::TodoItem, now: NaiveDateTime) -> Self {
        Self {
            id: item.id,
            text: item.text.clone(),
            done: item.done,
            due: item.due,
            overdue: item.is_overdue(now),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn todo_row(
    state: Entity<AppState>,
    view: Entity<WidgetWindow>,
    row_heights: Rc<RefCell<HashMap<TodoId, f32>>>,
    row: Row,
    reminder: bool,
    highlighted: bool,
    slide_delta: Option<f32>,
    anim_seq: u64,
) -> AnyElement {
    let id = row.id;
    let acker = state;

    let row_el = div()
        .id(("todo-row", id as usize))
        .relative()
        .flex()
        .items_center()
        .gap_2()
        .px(px(4.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .when(highlighted, |el| el.bg(rgba(panel::ROW_HOVER)))
        .hover(|style| style.bg(rgba(panel::ROW_HOVER)))
        .child(
            canvas(
                move |bounds, _, _| {
                    row_heights
                        .borrow_mut()
                        .insert(id, f32::from(bounds.size.height));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .child(tick_box(
            ("todo-done", id as usize),
            row.done,
            move |next, _window, cx| {
                view.update(cx, |this, cx| this.on_toggle_done(id, next, cx));
            },
        ))
        .child(
            div()
                .flex_1()
                .text_color(if row.done {
                    rgb(panel::MUTED)
                } else {
                    rgb(panel::TEXT)
                })
                .when(row.done, |text| text.line_through())
                .child(row.text),
        )
        .when_some(row.due, |line, due| {
            line.child(
                div()
                    .text_size(px(11.0))
                    .text_color(if row.overdue {
                        rgb(panel::DANGER)
                    } else {
                        rgb(panel::MUTED)
                    })
                    .child(due.format("%m-%d %H:%M").to_string()),
            )
        })
        .when(reminder, |line| {
            line.child(
                div()
                    .id(("todo-ack", id as usize))
                    .px(px(7.0))
                    .py(px(1.0))
                    .rounded(px(5.0))
                    .bg(rgba(panel::CHIP))
                    .text_size(px(11.0))
                    .text_color(rgb(panel::CHIP_TEXT))
                    .child("知道了")
                    .on_click(move |_event, _window, cx| {
                        acker.update(cx, |app, cx| {
                            app.ack(id);
                            app.commit_runtime(true, cx);
                        });
                    }),
            )
        });

    if let Some(delta) = slide_delta {
        let anim_id = ElementId::Name(format!("todo-slide-{id}-{anim_seq}").into());
        row_el
            .with_animation(
                anim_id,
                Animation::new(SLIDE_DURATION).with_easing(ease_out_cubic),
                move |el, t| el.top(px(delta * (1.0 - t))),
            )
            .into_any_element()
    } else {
        row_el.into_any_element()
    }
}

/// 标签筛选条上的一颗按钮。`tag = None` 表示「全部」。
fn tag_chip(
    state: Entity<AppState>,
    label: String,
    tag: Option<String>,
    active: bool,
) -> impl IntoElement {
    div()
        .id(format!("tag-chip-{label}"))
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .bg(if active {
            rgb(panel::ACCENT)
        } else {
            rgba(panel::CHIP)
        })
        .text_size(px(11.0))
        .text_color(rgb(panel::CHIP_TEXT))
        .child(label)
        .on_click(move |_event, _window, cx| {
            state.update(cx, |app, cx| {
                let changed = match &tag {
                    Some(tag) => app.toggle_tag_filter(tag),
                    None => app.clear_tag_filter(),
                };
                app.commit_click(changed, cx);
            });
        })
}

fn ease_out_cubic(t: f32) -> f32 {
    let clamped = t.clamp(0.0, 1.0);
    1.0 - (1.0 - clamped).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t.clamp(0.0, 1.0).powi(3)
}

/// 计算面板在指定动效与归一化进度 `0..=1` 下的 `(left, top, opacity)`。
fn motion_frame(motion: PanelMotion, progress: f32) -> (f32, f32, f32) {
    match motion {
        PanelMotion::Idle => (0.0, 0.0, 1.0),
        PanelMotion::Hidden => (0.0, 0.0, 0.0),
        PanelMotion::Expand => {
            let t = ease_out_cubic(progress);
            (260.0 * (1.0 - t), 0.0, t)
        }
        PanelMotion::Collapse => {
            let t = ease_in_cubic(progress);
            (260.0 * t, 0.0, 1.0 - t)
        }
        PanelMotion::FloatIn => {
            let t = ease_out_cubic(progress);
            (0.0, 16.0 * (1.0 - t), t)
        }
        PanelMotion::DropFadeOut => {
            let t = ease_in_cubic(progress);
            (0.0, 14.0 * t, 1.0 - t)
        }
        PanelMotion::SettleFadeIn => {
            let t = ease_out_cubic(progress);
            (0.0, -8.0 * (1.0 - t), t)
        }
    }
}

/// 按冻结顺序保留仍然可见的条目，并把新出现的条目补在末尾。
fn apply_frozen_order(frozen: &[TodoId], visible_ids: &[TodoId]) -> Vec<TodoId> {
    let visible_set: HashSet<TodoId> = visible_ids.iter().copied().collect();
    let mut result: Vec<TodoId> = frozen
        .iter()
        .copied()
        .filter(|id| visible_set.contains(id))
        .collect();
    let already: HashSet<TodoId> = result.iter().copied().collect();
    for &id in visible_ids {
        if !already.contains(&id) {
            result.push(id);
        }
    }
    result
}

/// 根据新旧顺序和真实行高计算每个位置发生变化条目的初始 Y 偏移（`old_y - new_y`）。
fn compute_slide_deltas(
    old_order: &[TodoId],
    new_order: &[TodoId],
    heights: &HashMap<TodoId, f32>,
    gap: f32,
) -> HashMap<TodoId, f32> {
    let positions = |order: &[TodoId]| -> HashMap<TodoId, f32> {
        let mut map = HashMap::with_capacity(order.len());
        let mut y = 0.0_f32;
        for &id in order {
            map.insert(id, y);
            let h = heights.get(&id).copied().unwrap_or(DEFAULT_ROW_HEIGHT);
            y += h + gap;
        }
        map
    };

    let old_y = positions(old_order);
    let new_y = positions(new_order);
    let mut deltas = HashMap::new();
    for (&id, &ny) in &new_y {
        if let Some(&oy) = old_y.get(&id) {
            let delta = oy - ny;
            if delta.abs() > 0.5 {
                deltas.insert(id, delta);
            }
        }
    }
    deltas
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        PanelMotion, apply_frozen_order, compute_slide_deltas, ease_in_cubic, ease_out_cubic,
        motion_frame,
    };

    #[test]
    fn ease_out_cubic_bounds_and_monotonic() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert!(ease_out_cubic(0.25) > 0.25, "ease-out 前段应更快");
        assert!(ease_out_cubic(0.5) < ease_out_cubic(0.75));
    }

    #[test]
    fn apply_frozen_order_preserves_order_and_handles_add_remove() {
        let frozen = [3, 1, 2];
        let visible = [1, 2, 4]; // 3 被删，4 新增
        assert_eq!(apply_frozen_order(&frozen, &visible), vec![1, 2, 4]);
    }

    #[test]
    fn compute_slide_deltas_moves_checked_down_and_others_up() {
        let mut heights = HashMap::new();
        heights.insert(1, 44.0); // 多行文本条目
        heights.insert(2, 30.0);
        heights.insert(3, 30.0);

        // old: [1(y=0), 2(y=46), 3(y=78)]
        // new: [2(y=0), 3(y=32), 1(y=64)]
        let deltas = compute_slide_deltas(&[1, 2, 3], &[2, 3, 1], &heights, 2.0);
        assert_eq!(deltas.get(&1).copied(), Some(-64.0)); // 1 从 0 下移到 64，初始偏移 -64
        assert_eq!(deltas.get(&2).copied(), Some(46.0)); // 2 从 46 上移到 0，初始偏移 +46
        assert_eq!(deltas.get(&3).copied(), Some(46.0)); // 3 从 78 上移到 32，初始偏移 +46
    }

    #[test]
    fn ease_in_cubic_bounds_and_monotonic() {
        assert_eq!(ease_in_cubic(0.0), 0.0);
        assert_eq!(ease_in_cubic(1.0), 1.0);
        assert!(ease_in_cubic(0.5) < 0.5, "ease-in 前段应更慢");
        assert!(ease_in_cubic(0.25) < ease_in_cubic(0.75));
    }

    #[test]
    fn motion_frames_distinguish_drawer_and_layer_transitions() {
        // 抽屉展开/收起只动水平位移 X
        assert_eq!(motion_frame(PanelMotion::Expand, 0.0), (260.0, 0.0, 0.0));
        assert_eq!(motion_frame(PanelMotion::Expand, 1.0), (0.0, 0.0, 1.0));
        assert_eq!(motion_frame(PanelMotion::Collapse, 0.0), (0.0, 0.0, 1.0));
        assert_eq!(motion_frame(PanelMotion::Collapse, 1.0), (260.0, 0.0, 0.0));

        // 静止与全透明预清态
        assert_eq!(motion_frame(PanelMotion::Idle, 0.5), (0.0, 0.0, 1.0));
        assert_eq!(motion_frame(PanelMotion::Hidden, 0.0), (0.0, 0.0, 0.0));
        assert_eq!(motion_frame(PanelMotion::Hidden, 1.0), (0.0, 0.0, 0.0));

        // Top/Bottom 切换只动垂直位移 Y + 淡入淡出
        assert_eq!(motion_frame(PanelMotion::FloatIn, 0.0), (0.0, 16.0, 0.0));
        assert_eq!(motion_frame(PanelMotion::FloatIn, 1.0), (0.0, 0.0, 1.0));
        assert_eq!(motion_frame(PanelMotion::DropFadeOut, 0.0), (0.0, 0.0, 1.0));
        assert_eq!(motion_frame(PanelMotion::DropFadeOut, 1.0), (0.0, 14.0, 0.0));
        assert_eq!(motion_frame(PanelMotion::SettleFadeIn, 0.0), (0.0, -8.0, 0.0));
        assert_eq!(motion_frame(PanelMotion::SettleFadeIn, 1.0), (0.0, 0.0, 1.0));
    }
}
