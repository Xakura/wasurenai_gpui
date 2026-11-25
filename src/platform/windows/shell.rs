//! Windows 文件打开与资源管理器定位。

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

pub fn open_file(path: &Path) -> Result<(), String> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if (result.0 as isize) <= 32 {
        return Err(format!("无法打开文件（Win32 错误码 {}）", result.0 as isize));
    }
    Ok(())
}

pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let mut cmd = Command::new("explorer.exe");
    if path.exists() {
        cmd.arg(format!("/select,{}", path.display()));
    } else if let Some(parent) = path.parent() {
        cmd.arg(parent);
    } else {
        return Err(format!("无法定位目录：{}", path.display()));
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|err| format!("打开资源管理器失败：{err}"))
}
