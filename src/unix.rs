use std::convert::TryFrom;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::panic;
use std::process::Child;

use libc::kill;
use libc::ESRCH;
use libc::SIGKILL;

#[derive(Debug)]
pub(crate) struct Process(i32);

impl Process {
    pub(crate) fn new(process: &Child) -> Self {
        Self(
            i32::try_from(process.id())
                .expect("returned process id is invalid"),
        )
    }

    pub(crate) fn terminate(&self) -> IoResult<()> {
        if unsafe { kill(self.0, SIGKILL) } == 0 {
            return Ok(());
        }

        let mut error = IoError::last_os_error();
        if let Some(error_code) = error.raw_os_error() {
            error = match error_code {
                0 => panic!("successful termination reported failure"),
                ESRCH => {
                    IoError::new(IoErrorKind::NotFound, "No such process")
                }
                _ => error,
            };
        }
        Err(error)
    }
}
