//! Windows 编辑窗口标题栏与任务栏图标加载（从 exe 内嵌 `.res` 资源 ID=1 读取）。

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON,
    SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
};
use windows::core::PCWSTR;

/// 从 exe 内嵌资源（ID = 1）加载图标并应用到窗口标题栏与任务栏。
///
/// 在未链接 `.res` 资源的环境下静默降级，不报错。
pub fn apply_window_icon(hwnd: HWND) {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let hinst = HINSTANCE(module.0);
        // Win32 MAKEINTRESOURCEW(1)：把整数资源 ID 1 编进低 16 位指针传入 LoadImageW
        let res_id = PCWSTR(std::ptr::without_provenance(1));

        let small_w = GetSystemMetrics(SM_CXSMICON).max(16);
        let small_h = GetSystemMetrics(SM_CYSMICON).max(16);
        if let Ok(handle) = LoadImageW(Some(hinst), res_id, IMAGE_ICON, small_w, small_h, LR_SHARED)
        {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(handle.0 as isize)),
            );
        }

        let big_w = GetSystemMetrics(SM_CXICON).max(32);
        let big_h = GetSystemMetrics(SM_CYICON).max(32);
        if let Ok(handle) = LoadImageW(Some(hinst), res_id, IMAGE_ICON, big_w, big_h, LR_SHARED) {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(handle.0 as isize)),
            );
        }
    }
}
