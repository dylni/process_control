use std::convert::TryInto;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::mem;
use std::os::raw::c_int;
use std::os::raw::c_uint;
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::process::ExitStatus as ProcessExitStatus;
use std::sync::mpsc;
use std::thread::Builder as ThreadBuilder;
use std::time::Duration;

use libc::pid_t;
use libc::EINTR;
use libc::ESRCH;
use libc::SIGKILL;

extern "C" {
    fn wait_for_process(pid: pid_t, exit_status: *mut ExitStatus) -> c_int;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub(crate) struct ExitStatus {
    value: c_int,
    terminated: bool,
}

impl ExitStatus {
    pub(crate) fn success(self) -> bool {
        !self.terminated && self.value == 0
    }

    fn convert_value(self, normal_exit: bool) -> Option<c_uint> {
        if self.terminated == normal_exit {
            None
        } else {
            Some(self.value.try_into().expect("exit value is invalid"))
        }
    }

    pub(crate) fn code(self) -> Option<c_uint> {
        self.convert_value(true)
    }

    pub(crate) fn signal(self) -> Option<c_uint> {
        self.convert_value(false)
    }
}

impl From<ProcessExitStatus> for ExitStatus {
    fn from(status: ProcessExitStatus) -> Self {
        if let Some(exit_code) = status.code() {
            Self {
                value: exit_code,
                terminated: false,
            }
        } else if let Some(signal) = status.signal() {
            Self {
                value: signal,
                terminated: true,
            }
        } else {
            unreachable!()
        }
    }
}

pub(crate) fn run_with_timeout<TReturn>(
    get_result_fn: impl 'static + FnOnce() -> TReturn + Send,
    time_limit: Duration,
) -> IoResult<Option<TReturn>>
where
    TReturn: 'static + Send,
{
    let (result_sender, result_receiver) = mpsc::channel();
    let _ = ThreadBuilder::new()
        .spawn(move || result_sender.send(get_result_fn()))?;

    Ok(result_receiver.recv_timeout(time_limit).ok())
}

#[derive(Debug)]
pub(crate) struct Handle(pid_t);

impl Handle {
    pub(crate) fn new(process: &Child) -> IoResult<Self> {
        Ok(Self::inherited(process))
    }

    pub(crate) fn inherited(process: &Child) -> Self {
        Self(
            process
                .id()
                .try_into()
                .expect("returned process identifier is invalid"),
        )
    }

    fn check_syscall(result: c_int) -> IoResult<()> {
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
        run_with_timeout(
            move || {
                let mut exit_status: ExitStatus = unsafe { mem::zeroed() };
                loop {
                    let result = Self::check_syscall(unsafe {
                        wait_for_process(process_id, &mut exit_status)
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
                Ok(exit_status)
            },
            time_limit,
        )?
        .transpose()
    }
}
