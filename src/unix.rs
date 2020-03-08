use std::convert::TryInto;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::process::ExitStatus;
use std::time::Duration;

use libc::pid_t;
use libc::EINTR;
use libc::ESRCH;
use libc::SIGKILL;

mod common;

#[derive(Debug)]
pub(crate) struct Handle(pid_t);

impl Handle {
    pub(crate) fn new(process: &Child) -> Self {
        Self(
            process
                .id()
                .try_into()
                .expect("returned process identifier is invalid"),
        )
    }

    fn check_syscall(result: i32) -> IoResult<()> {
        if result >= 0 {
            return Ok(());
        }

        let error = IoError::last_os_error();
        if let Some(error_code) = error.raw_os_error() {
            match error_code {
                0 => panic!("successful system call reported failure"),
                // This error is usually decoded to [ErrorKind::Other]:
                // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/mod.rs#L100-L123
                ESRCH => {
                    return Err(IoError::new(
                        IoErrorKind::NotFound,
                        "No such process",
                    ));
                }
                _ => {}
            };
        }
        Err(error)
    }

    pub(crate) fn terminate(&self) -> IoResult<()> {
        Self::check_syscall(unsafe { libc::kill(self.0, SIGKILL) })
    }

    pub(crate) fn wait_with_timeout(
        &self,
        time_limit: Duration,
    ) -> IoResult<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/process/process_unix.rs#L432-L441

        let process_id = self.0;
        common::run_with_timeout(
            move || {
                let mut exit_code = 0;
                loop {
                    let result = Self::check_syscall(unsafe {
                        libc::waitpid(process_id, &mut exit_code, 0)
                    });
                    match result {
                        Ok(()) => break,
                        Err(error) => {
                            if error.raw_os_error() != Some(EINTR) {
                                return Err(error);
                            }
                        }
                    }
                }
                Ok(ExitStatus::from_raw(exit_code))
            },
            time_limit,
        )?
        .transpose()
    }
}
