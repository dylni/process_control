use std::convert::TryInto;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::process::ExitStatus as ProcessExitStatus;
use std::ptr;
use std::time::Duration;

use winapi::shared::minwindef::BOOL;
use winapi::shared::minwindef::DWORD;
use winapi::shared::minwindef::TRUE;
use winapi::shared::winerror::ERROR_ACCESS_DENIED;
use winapi::shared::winerror::ERROR_INVALID_HANDLE;
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::handleapi::CloseHandle;
use winapi::um::handleapi::DuplicateHandle;
use winapi::um::minwinbase::STILL_ACTIVE;
use winapi::um::processthreadsapi::GetCurrentProcess;
use winapi::um::processthreadsapi::GetExitCodeProcess;
use winapi::um::processthreadsapi::TerminateProcess;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::WAIT_OBJECT_0;
use winapi::um::winnt::DUPLICATE_SAME_ACCESS;
use winapi::um::winnt::HANDLE;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExitStatus(i64);

impl ExitStatus {
    fn new(value: DWORD) -> Self {
        Self(value.into())
    }

    pub(crate) fn success(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn code(self) -> Option<i64> {
        Some(self.0)
    }
}

impl From<ProcessExitStatus> for ExitStatus {
    fn from(status: ProcessExitStatus) -> Self {
        if let Some(exit_code) = status.code() {
            Self(exit_code.into())
        } else {
            unreachable!()
        }
    }
}

impl Display for ExitStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "exit code: {}", self.0)
    }
}

#[derive(Debug)]
struct RawHandle(HANDLE);

// SAFETY: Process handles are thread-safe:
// https://stackoverflow.com/questions/12212628/win32-handles-and-multithread/12214212#12214212
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

#[derive(Debug)]
pub(crate) struct Handle {
    handle: RawHandle,
    duplicated: bool,
}

impl Handle {
    fn get_handle(process: &Child) -> HANDLE {
        process.as_raw_handle() as HANDLE
    }

    fn raw_os_error(error: &IoError) -> Option<DWORD> {
        error.raw_os_error().and_then(|x| x.try_into().ok())
    }

    fn not_found_error() -> IoError {
        IoError::new(IoErrorKind::NotFound, "The handle is invalid.")
    }

    fn check_syscall(result: BOOL) -> IoResult<()> {
        if result == TRUE {
            Ok(())
        } else {
            Err(IoError::last_os_error())
        }
    }

    pub(crate) fn new(process: &Child) -> IoResult<Self> {
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

    pub(crate) fn inherited(process: &Child) -> Self {
        Self {
            handle: RawHandle(Self::get_handle(process)),
            duplicated: false,
        }
    }

    const fn raw(&self) -> HANDLE {
        self.handle.0
    }

    fn get_exit_code(&self) -> IoResult<DWORD> {
        let mut exit_code = 0;
        Self::check_syscall(unsafe {
            GetExitCodeProcess(self.raw(), &mut exit_code)
        })?;
        Ok(exit_code)
    }

    pub(crate) fn terminate(&self) -> IoResult<()> {
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

    pub(crate) fn wait_with_timeout(
        &self,
        time_limit: Duration,
    ) -> IoResult<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/process.rs#L334-L344

        let time_limit_ms = time_limit
            .as_millis()
            .try_into()
            .unwrap_or_else(|_| DWORD::max_value());
        match unsafe { WaitForSingleObject(self.raw(), time_limit_ms) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => return Ok(None),
            _ => return Err(IoError::last_os_error()),
        }

        let exit_code = self.get_exit_code()?;
        Ok(Some(ExitStatus::new(exit_code)))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.duplicated {
            let _ = unsafe { CloseHandle(self.raw()) };
        }
    }
}
