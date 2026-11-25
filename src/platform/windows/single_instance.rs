//! Windows 单实例守卫：通过命名互斥体阻止重复启动第二个进程。

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

pub struct SingleInstanceGuard(Option<HANDLE>);

impl SingleInstanceGuard {
    pub fn acquire() -> Option<Self> {
        unsafe {
            let Ok(handle) = CreateMutexW(None, false, w!("Local\\WasurenaiGpuiSingleInstance"))
            else {
                eprintln!("[wasurenai] 创建单实例互斥体失败，跳过检查");
                return Some(Self(None));
            };
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(Self(Some(handle)))
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }
}
