use std::convert::TryInto;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::ExitStatusExt;
use std::process::Child;
use std::process::ExitStatus;
use std::time::Duration;

use winapi::shared::minwindef::BOOL;
use winapi::shared::minwindef::DWORD;
use winapi::shared::minwindef::TRUE;
use winapi::shared::winerror::ERROR_ACCESS_DENIED;
use winapi::shared::winerror::ERROR_INVALID_HANDLE;
use winapi::shared::winerror::ERROR_SUCCESS;
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::minwinbase::STILL_ACTIVE;
use winapi::um::processthreadsapi::GetExitCodeProcess;
use winapi::um::processthreadsapi::TerminateProcess;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::WAIT_OBJECT_0;
use winapi::um::winnt::HANDLE;

#[derive(Debug)]
pub(crate) struct Handle(HANDLE);

impl Handle {
    pub(crate) fn new(process: &Child) -> Self {
        Self(process.as_raw_handle() as HANDLE)
    }

    fn not_found_error() -> IoError {
        IoError::new(IoErrorKind::NotFound, "The handle is invalid.")
    }

    fn raw_os_error(error: &IoError) -> Option<DWORD> {
        error.raw_os_error().and_then(|x| x.try_into().ok())
    }

    fn last_error() -> IoError {
        let error = IoError::last_os_error();
        if let Some(error_code) = Self::raw_os_error(&error) {
            match error_code {
                ERROR_SUCCESS => {
                    panic!("successful system call reported failure");
                }
                // This error is usually decoded to [ErrorKind::Other]:
                // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/mod.rs#L55-L82
                ERROR_INVALID_HANDLE => return Self::not_found_error(),
                _ => {}
            };
        }
        error
    }

    fn check_syscall(result: BOOL) -> IoResult<()> {
        if result == TRUE {
            Ok(())
        } else {
            Err(Self::last_error())
        }
    }

    fn get_exit_code(&self) -> IoResult<DWORD> {
        let mut exit_code = 0;
        Self::check_syscall(unsafe {
            GetExitCodeProcess(self.0, &mut exit_code)
        })?;
        Ok(exit_code)
    }

    pub(crate) fn terminate(&self) -> IoResult<()> {
        let result =
            Self::check_syscall(unsafe { TerminateProcess(self.0, 1) });
        if let Err(error) = &result {
            // [TerminateProcess] fails if the process is being destroyed:
            // https://github.com/haskell/process/pull/111
            if Self::raw_os_error(error) == Some(ERROR_ACCESS_DENIED) {
                if let Ok(exit_code) = self.get_exit_code() {
                    if exit_code != STILL_ACTIVE {
                        return Err(Self::not_found_error());
                    }
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
        match unsafe { WaitForSingleObject(self.0, time_limit_ms) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => return Ok(None),
            _ => return Err(Self::last_error()),
        }

        let exit_code = self.get_exit_code()?;
        Ok(Some(ExitStatus::from_raw(exit_code)))
    }
}

// SAFETY: Process handles are thread-safe:
// https://stackoverflow.com/questions/12212628/win32-handles-and-multithread/12214212#12214212
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
