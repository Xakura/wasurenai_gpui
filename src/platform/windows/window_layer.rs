//! Windows 桌面层（Bottom）/ 置顶层（Top）切换。
//!
//! 方案来自 `SQSora/desktop_bottom` 的 Windows 实现，**不是** `SetParent(WorkerW)`：
//!
//! - 把窗口的 **owner**（`GWLP_HWNDPARENT`）设为 `GetShellWindow()`（Progman）。
//!   窗口仍是普通顶层窗口，渲染链路（gpui 走 DirectComposition）不受影响；
//!   Win+D 抬起桌面时，被 owner 的窗口会跟着出现在桌面之上（**这是 Win+D 仍可见的关键**）。
//! - 在 `WM_WINDOWPOSCHANGING` 里强制 `hwndInsertAfter = HWND_BOTTOM`，
//!   点击、任务栏激活都抬不到普通窗口上面；同一个消息里拦掉 Win+D 的隐藏移动。
//! - 快捷键 / 到期提醒要把窗口"上浮"时，只需 `SetWindowPos(HWND_TOPMOST)` 并把
//!   [`WindowLayer`] 切成 `Top`：此时不再强制置底，窗口就浮到最上层（owner 保持不变，
//!   所以仍然不进任务栏）。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY,
    DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SUBCLASSPROC, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWLP_HWNDPARENT, GetShellWindow, GetWindowLongPtrW, HWND_BOTTOM, HWND_NOTOPMOST,
    HWND_TOPMOST, IsWindowVisible, RegisterWindowMessageW, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, WINDOWPOS, WM_WINDOWPOSCHANGING, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, w};

use crate::platform::WindowLayer;

static SUBCLASSED: AtomicBool = AtomicBool::new(false);
static LAYER: AtomicU8 = AtomicU8::new(WindowLayer::Bottom as u8);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

const SUBCLASS_ID: usize = 0x57A5_0FA1;
/// Win+D 会把窗口挪到这个坐标附近来"隐藏"它。
const HIDDEN_POSITION: i32 = -30000;

pub fn current_layer() -> WindowLayer {
    match LAYER.load(Ordering::Relaxed) {
        x if x == WindowLayer::Top as u8 => WindowLayer::Top,
        _ => WindowLayer::Bottom,
    }
}

/// 窗口过程是否成功子类化（供日志/诊断用）。
pub fn subclassed() -> bool {
    SUBCLASSED.load(Ordering::Relaxed)
}

/// 接管窗口：清掉任务栏属性、设 owner、子类化、压到桌面层。返回是否完全成功。
pub fn attach(hwnd: HWND) -> bool {
    unsafe {
        let tc = RegisterWindowMessageW(w!("TaskbarCreated"));
        TASKBAR_CREATED.store(tc, Ordering::Relaxed);

        ensure_tool_window(hwnd);
        disable_dwm_frame(hwnd);

        let shell = GetShellWindow();
        if shell.is_invalid() {
            return false;
        }

        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, shell.0 as isize);

        let proc: SUBCLASSPROC = Some(layer_proc);
        let ok = SetWindowSubclass(hwnd, proc, SUBCLASS_ID, 0).as_bool();
        SUBCLASSED.store(ok, Ordering::Relaxed);

        let moved = set_layer(hwnd, WindowLayer::Bottom);
        ok && moved
    }
}

/// 兜底去掉 `WS_EX_APPWINDOW` 并加上 `WS_EX_TOOLWINDOW`，确保不进任务栏与 Alt+Tab。
unsafe fn ensure_tool_window(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let desired = (ex & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0;
        if desired != ex {
            let visible = IsWindowVisible(hwnd).as_bool();
            if visible {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired as isize);
            if visible {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }
}

/// 禁用 Win11 DWM 默认圆角、系统边框、非客户区渲染与内置显隐过渡，避免叠出四角灰框或与 GPUI 动效冲突。
unsafe fn disable_dwm_frame(hwnd: HWND) {
    unsafe {
        let corner = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner).cast(),
            std::mem::size_of_val(&corner) as u32,
        );
        let border = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&raw const border).cast(),
            std::mem::size_of_val(&border) as u32,
        );
        let ncrp = DWMNCRP_DISABLED;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            (&raw const ncrp).cast(),
            std::mem::size_of_val(&ncrp) as u32,
        );
        let disable_transitions = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const disable_transitions).cast(),
            std::mem::size_of_val(&disable_transitions) as u32,
        );
    }
}

/// 切换层级。`Top` 时窗口浮到最上层，`Bottom` 时压回桌面层。
pub fn set_layer(hwnd: HWND, layer: WindowLayer) -> bool {
    LAYER.store(layer as u8, Ordering::Relaxed);

    unsafe {
        let no_activate = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        match layer {
            WindowLayer::Top => {
                SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, no_activate).is_ok()
            }
            WindowLayer::Bottom => {
                let normal = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, no_activate);
                let bottom = SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, no_activate);
                normal.is_ok() && bottom.is_ok()
            }
        }
    }
}

/// 显示 / 隐藏窗口（收起展开用）。
pub fn set_visible(hwnd: HWND, visible: bool) -> bool {
    let cmd = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
    unsafe {
        let _ = ShowWindow(hwnd, cmd);
        IsWindowVisible(hwnd).as_bool() == visible
    }
}

unsafe extern "system" fn layer_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // Explorer 重启后会广播 TaskbarCreated：重新把 owner 挂回新的 Progman 并恢复层级
    let tc = TASKBAR_CREATED.load(Ordering::Relaxed);
    if tc != 0 && msg == tc {
        unsafe {
            disable_dwm_frame(hwnd);
            let shell = GetShellWindow();
            if !shell.is_invalid() {
                SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, shell.0 as isize);
            }
        }
        let layer = current_layer();
        let ok = set_layer(hwnd, layer);
        println!("[wasurenai] Explorer 重启后恢复桌面层 owner 与层级={layer:?} ok={ok}");
    }

    // 只有置于桌面层时才需要拦消息：置顶状态下让系统正常处理。
    if msg == WM_WINDOWPOSCHANGING && current_layer() == WindowLayer::Bottom {
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
