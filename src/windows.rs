use std::convert::TryInto;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::ExitStatusExt;
use std::process::Child;
pub(super) use std::process::ExitStatus;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::DuplicateHandle;
use windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS;
use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows_sys::Win32::Foundation::ERROR_INVALID_HANDLE;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::STATUS_PENDING;
use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::TerminateProcess;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::System::Threading::WAIT_OBJECT_0;

// https://github.com/microsoft/windows-rs/issues/881
#[allow(clippy::upper_case_acronyms)]
type BOOL = i32;
#[allow(clippy::upper_case_acronyms)]
type DWORD = u32;

const TRUE: BOOL = 1;
const STILL_ACTIVE: DWORD = STATUS_PENDING as DWORD;

#[derive(Debug)]
struct RawHandle(HANDLE);

// SAFETY: Process handles are thread-safe:
// https://stackoverflow.com/questions/12212628/win32-handles-and-multithread/12214212#12214212
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

#[derive(Debug)]
pub(super) struct Handle {
    handle: RawHandle,
    duplicated: bool,
}

impl Handle {
    fn get_handle(process: &Child) -> HANDLE {
        process.as_raw_handle()
    }

    fn raw_os_error(error: &io::Error) -> Option<DWORD> {
        error.raw_os_error().and_then(|x| x.try_into().ok())
    }

    fn not_found_error() -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, "The handle is invalid.")
    }

    fn check_syscall(result: BOOL) -> io::Result<()> {
        if result == TRUE {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn new(process: &Child) -> io::Result<Self> {
        let parent_handle = unsafe { GetCurrentProcess() };
        let mut handle = ptr::null_mut();
        Self::check_syscall(unsafe {
            DuplicateHandle(
                parent_handle,
                Self::get_handle(process),
                parent_handle,
                &mut handle,
                0,
                TRUE,
                DUPLICATE_SAME_ACCESS,
            )
        })?;
        Ok(Self {
            handle: RawHandle(handle),
            duplicated: true,
        })
    }

    pub(super) fn inherited(process: &Child) -> Self {
        Self {
            handle: RawHandle(Self::get_handle(process)),
            duplicated: false,
        }
    }

    const fn raw(&self) -> HANDLE {
        self.handle.0
    }

    fn get_exit_code(&self) -> io::Result<DWORD> {
        let mut exit_code = 0;
        Self::check_syscall(unsafe {
            GetExitCodeProcess(self.raw(), &mut exit_code)
        })?;
        Ok(exit_code)
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        let result =
            Self::check_syscall(unsafe { TerminateProcess(self.raw(), 1) });
        if let Err(error) = &result {
            if let Some(error_code) = Self::raw_os_error(error) {
                match error_code {
                    ERROR_ACCESS_DENIED => {
                        if let Ok(exit_code) = self.get_exit_code() {
                            if exit_code != STILL_ACTIVE {
                                return Err(Self::not_found_error());
                            }
                        }
                    }
                    // This error is usually decoded to [ErrorKind::Other]:
                    // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/mod.rs#L55-L82
                    ERROR_INVALID_HANDLE => {
                        return Err(Self::not_found_error());
                    }
                    _ => {}
                }
            }
        }
        result
    }

    pub(super) fn wait_with_timeout(
        &self,
        time_limit: Duration,
    ) -> io::Result<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/process.rs#L334-L344

        let mut remaining_time_limit = time_limit.as_millis();
        while remaining_time_limit > 0 {
            let time_limit =
                remaining_time_limit.try_into().unwrap_or(DWORD::MAX);

            match unsafe { WaitForSingleObject(self.raw(), time_limit) } {
                WAIT_OBJECT_0 => {
                    let exit_code = self.get_exit_code()?;
                    return Ok(Some(ExitStatus::from_raw(exit_code)));
                }
                WAIT_TIMEOUT => {}
                _ => return Err(io::Error::last_os_error()),
            }

            remaining_time_limit -= u128::from(time_limit);
        }
        Ok(None)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.duplicated {
            let _ = unsafe { CloseHandle(self.raw()) };
        }
    }
}
