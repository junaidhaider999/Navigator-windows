//! Single-instance lock via named mutex (`Agent/workflow/06-windows-apis.md`).

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;

    pub struct Guard(HANDLE);

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("another Navigator instance is already running")]
        AlreadyRunning,
        #[error("Windows API error: {0}")]
        Win32(#[from] windows::core::Error),
    }

    pub fn acquire() -> Result<Guard, Error> {
        unsafe {
            let h = CreateMutexW(None, true, w!(r"Local\Navigator.SingleInstance.M2"))?;
            let err = windows::Win32::Foundation::GetLastError();
            if err == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(h);
                return Err(Error::AlreadyRunning);
            }
            Ok(Guard(h))
        }
    }
}

#[cfg(windows)]
pub use win::{Error, acquire};
