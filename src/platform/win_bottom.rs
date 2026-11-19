//! 让窗口常驻桌面层（在所有普通窗口之下、Win+D 显示桌面时仍可见）。
//!
//! 方案来自 `SQSora/desktop_bottom` 的 Windows 实现，**不是** `SetParent(WorkerW)`：
//!
//! 1. 把窗口的 **owner**（`GWLP_HWNDPARENT`）设为 `GetShellWindow()`（Progman）。
//!    窗口仍然是普通顶层窗口，渲染链路（gpui 走 DirectComposition）完全不受影响；
//!    Win+D 抬起桌面时，被 owner 的窗口会跟着出现在桌面之上。
//! 2. 在 `WM_WINDOWPOSCHANGING` 里强制 `hwndInsertAfter = HWND_BOTTOM`，
//!    这样点击、任务栏激活都抬不到普通窗口上面。
//! 3. 同一个消息里拦掉 Win+D 把窗口挪到 `(-32000, -32000)` 并隐藏的动作。
//!
//! 拦截消息用 `SetWindowSubclass`（comctl32），`DefSubclassProc` 负责把其余消息
//! 原样转发给 gpui 自己的窗口过程，不需要自己保存原 proc 指针。
//!
//! 已知限制：Explorer 重启会连带销毁被 owner 的窗口（见 docs/plan.md 风险表）。

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SUBCLASSPROC, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GWLP_HWNDPARENT, HWND_BOTTOM, SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, SetWindowLongPtrW,
    SetWindowPos, ShowWindowAsync, WM_WINDOWPOSCHANGING, WINDOWPOS,
};

static SUBCLASSED: AtomicBool = AtomicBool::new(false);

const SUBCLASS_ID: usize = 0x57A5_0FA1;
/// Win+D 会把窗口挪到这个坐标附近来"隐藏"它。
const HIDDEN_POSITION: i32 = -30000;

/// 显示 / 隐藏窗口（收起展开用）。
///
/// 刻意不用"关窗再重建"：重建会重新走 owner 修改 + DirectComposition 建链，
/// 实测第二个窗口会呈现空白（窗口 Z 序正常、`WindowFromPoint` 也命中它，就是没内容）。
pub fn set_visible(hwnd: HWND, visible: bool) -> bool {
    let cmd = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
    unsafe { ShowWindowAsync(hwnd, cmd).as_bool() }
}

/// 窗口过程是否成功子类化（供日志/诊断用）。
pub fn subclassed() -> bool {
    SUBCLASSED.load(Ordering::Relaxed)
}

/// 把窗口压到桌面层。返回是否完全成功（owner 设置 + 子类化都成功）。
pub fn apply_bottom(hwnd: HWND) -> bool {
    unsafe {
        let shell = GetShellWindow();
        if shell.is_invalid() {
            return false;
        }

        // 诊断开关（临时）：分离「改 owner」与「强制置底」各自的影响
        if std::env::var_os("WASURENAI_NO_OWNER").is_none() {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, shell.0 as isize);
        }
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );

        let proc: SUBCLASSPROC = Some(bottom_proc);
        let ok = SetWindowSubclass(hwnd, proc, SUBCLASS_ID, 0).as_bool();
        SUBCLASSED.store(ok, Ordering::Relaxed);
        ok
    }
}

unsafe extern "system" fn bottom_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_WINDOWPOSCHANGING {
        let pos = lparam.0 as *mut WINDOWPOS;
        if !pos.is_null() {
            let pos = unsafe { &mut *pos };

            // Win+D 的隐藏动作：保持位置和大小，并清掉隐藏标志
            if pos.x <= HIDDEN_POSITION || pos.y <= HIDDEN_POSITION {
                pos.flags |= SWP_NOMOVE | SWP_NOSIZE;
                pos.flags.0 &= !SWP_HIDEWINDOW.0;
            }

            // 任何置顶/激活请求都压回最底层
            if std::env::var_os("WASURENAI_NO_BOTTOM").is_none() {
                pos.hwndInsertAfter = HWND_BOTTOM;
                pos.flags.0 &= !SWP_NOZORDER.0;
            }
        }
    }

    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
