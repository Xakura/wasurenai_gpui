//! 打开本地文件或在资源管理器中定位文件。
//!
//! 只做薄封装，不涉及任何待办数据结构。

use std::path::Path;
use std::process::Command;

/// 使用系统默认关联程序打开指定文件（例如用默认 Markdown / 文本编辑器打开日志）。
pub fn open_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("文件不存在：{}", path.display()));
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{PCWSTR, w};

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

    #[cfg(not(windows))]
    {
        let _ = path;
        Err("仅支持 Windows 平台".to_string())
    }
}

/// 打开文件所在目录并在 Windows 资源管理器中选中该文件。
pub fn reveal_in_explorer(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
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

    #[cfg(not(windows))]
    {
        let _ = path;
        Err("仅支持 Windows 平台".to_string())
    }
}
