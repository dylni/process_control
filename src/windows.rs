use std::convert::TryFrom;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::RawHandle;
use std::panic;
use std::process::Child;

use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::ERROR_INVALID_HANDLE;
use winapi::shared::winerror::ERROR_SUCCESS;
use winapi::um::processthreadsapi::TerminateProcess;
use winapi::um::winnt::HANDLE;

#[derive(Debug)]
pub(crate) struct Process(RawHandle);

impl Process {
    pub(crate) fn new(process: &Child) -> Self {
        Self(process.as_raw_handle())
    }

    pub(crate) fn terminate(&self) -> IoResult<()> {
        if unsafe { TerminateProcess(self.0 as HANDLE, 1) } != 0 {
            return Ok(());
        }

        let mut error = IoError::last_os_error();
        if let Some(error_code) =
            error.raw_os_error().and_then(|x| DWORD::try_from(x).ok())
        {
            error = match error_code {
                ERROR_SUCCESS => {
                    panic!("successful termination reported failure");
                }
                ERROR_INVALID_HANDLE => IoError::new(
                    IoErrorKind::NotFound,
                    "The handle is invalid.",
                ),
                _ => error,
            };
        }
        Err(error)
    }
}
