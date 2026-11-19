//! 桌面层（bottom）/ 置顶层（top）切换。
//!
//! 方案来自 `SQSora/desktop_bottom` 的 Windows 实现，**不是** `SetParent(WorkerW)`：
//!
//! - 把窗口的 **owner**（`GWLP_HWNDPARENT`）设为 `GetShellWindow()`（Progman）。
//!   窗口仍是普通顶层窗口，渲染链路（gpui 走 DirectComposition）不受影响；
//!   Win+D 抬起桌面时，被 owner 的窗口会跟着出现在桌面之上（**这是 Win+D 仍可见的关键**）。
//! - 在 `WM_WINDOWPOSCHANGING` 里强制 `hwndInsertAfter = HWND_BOTTOM`，
//!   点击、任务栏激活都抬不到普通窗口上面；同一个消息里拦掉 Win+D 的隐藏移动。
//! - 快捷键 / 到期提醒要把窗口"上浮"时，只需 `SetWindowPos(HWND_TOPMOST)` 并把
//!   [`Layer`] 切成 `Top`：此时不再强制置底，窗口就浮到最上层（owner 保持不变，
//!   所以仍然不进任务栏）。
//!
//! 已知限制：Explorer 重启会连带销毁被 owner 的窗口（见 docs/plan.md 风险表）。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SUBCLASSPROC, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GWLP_HWNDPARENT, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOPMOST, SWP_FRAMECHANGED,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOWNOACTIVATE, SetWindowLongPtrW, SetWindowPos, ShowWindowAsync, WM_WINDOWPOSCHANGING,
    WINDOWPOS,
};

static SUBCLASSED: AtomicBool = AtomicBool::new(false);
static LAYER: AtomicU8 = AtomicU8::new(Layer::Bottom as u8);

const SUBCLASS_ID: usize = 0x57A5_0FA1;
/// Win+D 会把窗口挪到这个坐标附近来"隐藏"它。
const HIDDEN_POSITION: i32 = -30000;

/// 窗口当前所处的层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// 桌面层：在所有普通窗口之下，Win+D 时仍可见。
    Bottom = 0,
    /// 置顶层：浮在所有窗口之上（快捷键 / 到期提醒）。
    Top = 1,
}

pub fn current_layer() -> Layer {
    match LAYER.load(Ordering::Relaxed) {
        x if x == Layer::Top as u8 => Layer::Top,
        _ => Layer::Bottom,
    }
}

/// 窗口过程是否成功子类化（供日志/诊断用）。
pub fn subclassed() -> bool {
    SUBCLASSED.load(Ordering::Relaxed)
}

/// 接管窗口：设 owner、子类化、压到桌面层。返回是否完全成功。
pub fn attach(hwnd: HWND) -> bool {
    unsafe {
        let shell = GetShellWindow();
        if shell.is_invalid() {
            return false;
        }

        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, shell.0 as isize);

        let proc: SUBCLASSPROC = Some(layer_proc);
        let ok = SetWindowSubclass(hwnd, proc, SUBCLASS_ID, 0).as_bool();
        SUBCLASSED.store(ok, Ordering::Relaxed);

        let moved = set_layer(hwnd, Layer::Bottom);
        ok && moved
    }
}

/// 切换层级。`Top` 时窗口浮到最上层，`Bottom` 时压回桌面层。
pub fn set_layer(hwnd: HWND, layer: Layer) -> bool {
    LAYER.store(layer as u8, Ordering::Relaxed);

    unsafe {
        let no_activate = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        match layer {
            Layer::Top => SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                no_activate | SWP_FRAMECHANGED,
            )
            .is_ok(),
            Layer::Bottom => {
                let normal = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, no_activate);
                let bottom = SetWindowPos(
                    hwnd,
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    0,
                    0,
                    no_activate | SWP_FRAMECHANGED,
                );
                normal.is_ok() && bottom.is_ok()
            }
        }
    }
}

/// 显示 / 隐藏窗口（收起展开用）。
///
/// 刻意不用"关窗再重建"：重建要重新走 owner 修改 + DirectComposition 建链，成本高且脆弱。
pub fn set_visible(hwnd: HWND, visible: bool) -> bool {
    let cmd = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
    unsafe { ShowWindowAsync(hwnd, cmd).as_bool() }
}

unsafe extern "system" fn layer_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // 只有置于桌面层时才需要拦消息：置顶状态下让系统正常处理。
    if msg == WM_WINDOWPOSCHANGING && current_layer() == Layer::Bottom {
        let pos = lparam.0 as *mut WINDOWPOS;
        if !pos.is_null() {
            let pos = unsafe { &mut *pos };

            // Win+D 的隐藏动作：保持位置和大小，并清掉隐藏标志
            if pos.x <= HIDDEN_POSITION || pos.y <= HIDDEN_POSITION {
                pos.flags |= SWP_NOMOVE | SWP_NOSIZE;
                pos.flags.0 &= !SWP_HIDEWINDOW.0;
            }

            // 任何置顶/激活请求都压回最底层
            pos.hwndInsertAfter = HWND_BOTTOM;
            pos.flags.0 &= !SWP_NOZORDER.0;
        }
    }

    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
