//! 单实例守卫：通过命名互斥体阻止重复启动第二个进程。

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

/// 持有一个全局命名互斥体；进程退出时 `Drop` 释放句柄。
pub struct SingleInstanceGuard(Option<HANDLE>);

impl SingleInstanceGuard {
    /// 尝试获取单实例锁。若已有实例在运行，关闭句柄并返回 `None`。
    pub fn acquire() -> Option<Self> {
        unsafe {
            let Ok(handle) = CreateMutexW(None, false, w!("Local\\WasurenaiGpuiSingleInstance"))
            else {
                // 创建失败时不阻塞正常使用，放行并打日志
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
