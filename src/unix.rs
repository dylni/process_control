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
use std::thread;
use std::time::Duration;

use libc::id_t;
use libc::CLD_EXITED;
use libc::EINTR;
use libc::ESRCH;
use libc::P_PID;
use libc::SIGKILL;
use libc::WEXITED;
use libc::WNOWAIT;
use libc::WSTOPPED;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub(super) struct ExitStatus {
    value: c_int,
    terminated: bool,
}

impl ExitStatus {
    pub(super) fn success(self) -> bool {
        !self.terminated && self.value == 0
    }

    fn get_value(self, normal_exit: bool) -> Option<c_int> {
        Some(self.value).filter(|_| self.terminated != normal_exit)
    }

    pub(super) fn code(self) -> Option<c_int> {
        self.get_value(true)
    }

    pub(super) fn signal(self) -> Option<c_int> {
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
    fn from(value: process::ExitStatus) -> Self {
        if let Some(exit_code) = value.code() {
            Self {
                value: exit_code,
                terminated: false,
            }
        } else if let Some(signal) = value.signal() {
            Self {
                value: signal,
                terminated: true,
            }
        } else {
            unreachable!()
        }
    }
}

pub(super) fn run_with_timeout<TReturn>(
    get_result_fn: impl 'static + FnOnce() -> TReturn + Send,
    time_limit: Duration,
) -> io::Result<Option<TReturn>>
where
    TReturn: 'static + Send,
{
    let (result_sender, result_receiver) = {
        #[cfg(feature = "crossbeam-channel")]
        {
            crossbeam_channel::bounded(0)
        }
        #[cfg(not(feature = "crossbeam-channel"))]
        {
            use std::sync::mpsc;

            mpsc::channel()
        }
    };
    let _ = thread::Builder::new()
        .spawn(move || result_sender.send(get_result_fn()))?;

    Ok(result_receiver.recv_timeout(time_limit).ok())
}

#[derive(Debug)]
pub(super) struct Handle(id_t);

impl Handle {
    fn check_syscall(result: c_int) -> io::Result<()> {
        if result >= 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn new(process: &Child) -> io::Result<Self> {
        Ok(Self::inherited(process))
    }

    pub(super) fn inherited(process: &Child) -> Self {
        #[allow(clippy::useless_conversion)]
        Self(process.id().into())
    }

    pub(super) unsafe fn terminate(&self) -> io::Result<()> {
        let process_id =
            self.0.try_into().expect("process identifier is invalid");
        let result = Self::check_syscall(libc::kill(process_id, SIGKILL));
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

    pub(super) fn wait_with_timeout(
        &self,
        time_limit: Duration,
    ) -> io::Result<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/process/process_unix.rs#L432-L441

        let process_id = self.0;
        run_with_timeout(
            move || loop {
                let mut process_info = MaybeUninit::uninit();
                let result = Self::check_syscall(unsafe {
                    libc::waitid(
                        P_PID,
                        process_id,
                        process_info.as_mut_ptr(),
                        WEXITED | WNOWAIT | WSTOPPED,
                    )
                });
                match result {
                    Ok(()) => {
                        let process_info =
                            unsafe { process_info.assume_init() };
                        break Ok(ExitStatus {
                            value: unsafe { process_info.si_status() },
                            terminated: process_info.si_code != CLD_EXITED,
                        });
                    }
                    Err(error) => {
                        if error.raw_os_error() != Some(EINTR) {
                            break Err(error);
                        }
                    }
                }
            },
            time_limit,
        )?
        .transpose()
    }
}
