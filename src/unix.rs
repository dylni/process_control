use std::convert::TryInto;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
use std::mem::MaybeUninit;
use std::os::raw::c_int;
use std::os::unix::process::ExitStatusExt;
use std::process;
use std::process::Child;
use std::sync::mpsc;
use std::thread;
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

    fn get_value(self, normal_exit: bool) -> Option<c_int> {
        if self.terminated == normal_exit {
            None
        } else {
            Some(self.value)
        }
    }

    pub(crate) fn code(self) -> Option<c_int> {
        self.get_value(true)
    }

    pub(crate) fn signal(self) -> Option<c_int> {
        self.get_value(false)
    }
}

impl Display for ExitStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if self.terminated {
            write!(formatter, "signal: {}", self.value)
        } else {
            write!(formatter, "exit code: {}", self.value)
        }
    }
}

impl From<process::ExitStatus> for ExitStatus {
    fn from(status: process::ExitStatus) -> Self {
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
) -> io::Result<Option<TReturn>>
where
    TReturn: 'static + Send,
{
    let (result_sender, result_receiver) = mpsc::channel();
    let _ = thread::Builder::new()
        .spawn(move || result_sender.send(get_result_fn()))?;

    Ok(result_receiver.recv_timeout(time_limit).ok())
}

#[derive(Debug)]
pub(crate) struct Handle(pid_t);

impl Handle {
    fn check_syscall(result: c_int) -> io::Result<()> {
        if result >= 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(crate) fn new(process: &Child) -> io::Result<Self> {
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

    pub(crate) unsafe fn terminate(&self) -> io::Result<()> {
        let result = Self::check_syscall(libc::kill(self.0, SIGKILL));
        if let Err(error) = &result {
            // This error is usually decoded to [ErrorKind::Other]:
            // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/mod.rs#L100-L123
            if error.raw_os_error() == Some(ESRCH) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "No such process",
                ));
            }
        }
        result
    }

    pub(crate) fn wait_with_timeout(
        &self,
        time_limit: Duration,
    ) -> io::Result<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/process/process_unix.rs#L432-L441

        let process_id = self.0;
        run_with_timeout(
            move || {
                let mut exit_status = MaybeUninit::uninit();
                loop {
                    let result = Self::check_syscall(unsafe {
                        wait_for_process(process_id, exit_status.as_mut_ptr())
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
                Ok(unsafe { exit_status.assume_init() })
            },
            time_limit,
        )?
        .transpose()
    }
}
