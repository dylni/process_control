use std::convert::TryInto;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::process::Child;

use libc::pid_t;
use libc::ESRCH;
use libc::SIGKILL;

#[derive(Debug)]
pub(crate) struct Process(pid_t);

impl Process {
    pub(crate) fn new(process: &Child) -> Self {
        Self(
            process
                .id()
                .try_into()
                .expect("returned process identifier is invalid"),
        )
    }

    pub(crate) fn terminate(&self) -> IoResult<()> {
        if unsafe { libc::kill(self.0, SIGKILL) } == 0 {
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
