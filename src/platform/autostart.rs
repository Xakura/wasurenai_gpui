//! Windows 开机自启动：读写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`。

use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{PCWSTR, w};

const RUN_SUBKEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("Wasurenai");

/// 当前是否已在注册表 Run 键里启用了开机自启。
pub fn is_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let err = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_SUBKEY, None, KEY_READ, &mut hkey);
        if err != ERROR_SUCCESS {
            return false;
        }
        let mut size = 0u32;
        let query = RegQueryValueExW(hkey, VALUE_NAME, None, None, None, Some(&mut size));
        let _ = RegCloseKey(hkey);
        // 带空终止符的 UTF-16 至少大于 2 字节才算有效路径
        query == ERROR_SUCCESS && size > 2
    }
}

/// 启用或取消开机自启。
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe()
            .map_err(|err| format!("无法获取程序路径：{err}"))?;
        let command = format_run_command(&exe);
        let wide = to_utf16_bytes(&command);
        unsafe {
            let mut hkey = HKEY::default();
            let err = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                RUN_SUBKEY,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut hkey,
                None,
            );
            if err != ERROR_SUCCESS {
                return Err(format!("打开开机启动注册表失败（错误码 {}）", err.0));
            }
            let write_err = RegSetValueExW(hkey, VALUE_NAME, None, REG_SZ, Some(&wide));
            let _ = RegCloseKey(hkey);
            if write_err != ERROR_SUCCESS {
                return Err(format!("写入开机启动项失败（错误码 {}）", write_err.0));
            }
        }
    } else {
        unsafe {
            let mut hkey = HKEY::default();
            let err = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_SUBKEY, None, KEY_WRITE, &mut hkey);
            if err == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            if err != ERROR_SUCCESS {
                return Err(format!("打开开机启动注册表失败（错误码 {}）", err.0));
            }
            let del_err = RegDeleteValueW(hkey, VALUE_NAME);
            let _ = RegCloseKey(hkey);
            if del_err != ERROR_SUCCESS && del_err != ERROR_FILE_NOT_FOUND {
                return Err(format!("删除开机启动项失败（错误码 {}）", del_err.0));
            }
        }
    }
    Ok(())
}

/// 把可执行文件路径包进双引号，防止路径带空格时开机启动解析出错。
pub(crate) fn format_run_command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// 编码成带结尾 NUL 的 UTF-16LE 字节流供 `REG_SZ` 使用。
pub(crate) fn to_utf16_bytes(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn format_run_command_quotes_spaces() {
        let path = PathBuf::from(r"C:\Program Files\Wasurenai\wasurenai_gpui.exe");
        assert_eq!(
            format_run_command(&path),
            r#""C:\Program Files\Wasurenai\wasurenai_gpui.exe""#
        );
    }

    #[test]
    fn to_utf16_bytes_ends_with_nul() {
        let bytes = to_utf16_bytes("AB");
        assert_eq!(bytes, vec![b'A', 0, b'B', 0, 0, 0]);
    }
}
