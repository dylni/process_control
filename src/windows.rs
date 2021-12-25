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
use windows_sys::Win32::System::WindowsProgramming::INFINITE;

// https://github.com/microsoft/windows-rs/issues/881
#[allow(clippy::upper_case_acronyms)]
type BOOL = i32;
#[allow(clippy::upper_case_acronyms)]
type DWORD = u32;

const TRUE: BOOL = 1;
const STILL_ACTIVE: DWORD = STATUS_PENDING as DWORD;

fn raw_os_error(error: &io::Error) -> Option<DWORD> {
    error.raw_os_error().and_then(|x| x.try_into().ok())
}

fn check_syscall(result: BOOL) -> io::Result<()> {
    if result == TRUE {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Debug)]
struct RawHandle(HANDLE);

// SAFETY: Process handles are thread-safe:
// https://stackoverflow.com/questions/12212628/win32-handles-and-multithread/12214212#12214212
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

#[derive(Debug)]
pub(super) struct SharedHandle {
    handle: RawHandle,
    pub(super) time_limit: Option<Duration>,
}

impl SharedHandle {
    pub(super) unsafe fn new(process: &Child) -> Self {
        Self {
            handle: RawHandle(process.as_raw_handle()),
            time_limit: None,
        }
    }

    const fn as_raw(&self) -> HANDLE {
        self.handle.0
    }

    fn get_exit_code(&self) -> io::Result<DWORD> {
        let mut exit_code = 0;
        check_syscall(unsafe {
            GetExitCodeProcess(self.as_raw(), &mut exit_code)
        })?;
        Ok(exit_code)
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        let result =
            check_syscall(unsafe { TerminateProcess(self.as_raw(), 1) });
        if let Err(error) = &result {
            if let Some(error_code) = raw_os_error(error) {
                let not_found = match error_code {
                    ERROR_ACCESS_DENIED => {
                        matches!(
                            self.get_exit_code(),
                            Ok(x) if x != STILL_ACTIVE,
                        )
                    }
                    // This error is usually decoded to [ErrorKind::Other]:
                    // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/mod.rs#L55-L82
                    ERROR_INVALID_HANDLE => true,
                    _ => false,
                };
                if not_found {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "The handle is invalid.",
                    ));
                }
            }
        }
        result
    }

    pub(super) fn wait(&mut self) -> io::Result<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/process.rs#L334-L344

        let mut remaining_time_limit = self
            .time_limit
            .map(|x| x.as_millis())
            .unwrap_or_else(|| INFINITE.into());
        while remaining_time_limit > 0 {
            let time_limit =
                remaining_time_limit.try_into().unwrap_or(DWORD::MAX);

            match unsafe { WaitForSingleObject(self.as_raw(), time_limit) } {
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

#[derive(Debug)]
pub(super) struct DuplicatedHandle(SharedHandle);

impl DuplicatedHandle {
    pub(super) fn new(process: &Child) -> io::Result<Self> {
        let parent_handle = unsafe { GetCurrentProcess() };
        let mut handle = ptr::null_mut();
        check_syscall(unsafe {
            DuplicateHandle(
                parent_handle,
                process.as_raw_handle(),
                parent_handle,
                &mut handle,
                0,
                TRUE,
                DUPLICATE_SAME_ACCESS,
            )
        })?;
        Ok(Self(SharedHandle {
            handle: RawHandle(handle),
            time_limit: None,
        }))
    }

    pub(super) const unsafe fn as_inner(&self) -> &SharedHandle {
        &self.0
    }
}

impl Drop for DuplicatedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0.as_raw()) };
    }
}
