use std::convert::TryFrom;
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
use std::ptr;
use std::thread;
use std::time::Duration;

use libc::__rlimit_resource_t;
use libc::pid_t;
use libc::rlimit;
use libc::CLD_EXITED;
use libc::EINTR;
use libc::ESRCH;
use libc::P_PID;
use libc::RLIMIT_AS;
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
        (self.terminated ^ normal_exit).then(|| self.value)
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

fn check_syscall(result: c_int) -> io::Result<()> {
    if result >= 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn run_with_timeout<TReturn>(
    get_result_fn: impl 'static + FnOnce() -> TReturn + Send,
    time_limit: Option<Duration>,
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

    Ok(time_limit
        .map(|x| result_receiver.recv_timeout(x).ok())
        .unwrap_or_else(|| {
            Some(result_receiver.recv().expect("channel was disconnected"))
        }))
}

#[derive(Debug)]
struct RawPid(u32);

impl RawPid {
    fn new(process: &Child) -> Self {
        Self(process.id())
    }

    fn as_pid(&self) -> pid_t {
        self.0.try_into().expect("process identifier is invalid")
    }
}

#[derive(Debug)]
pub(super) struct SharedHandle {
    pid: RawPid,
    pub(super) memory_limit: Option<usize>,
    pub(super) time_limit: Option<Duration>,
}

impl SharedHandle {
    pub(super) unsafe fn new(process: &Child) -> Self {
        Self {
            pid: RawPid::new(process),
            memory_limit: None,
            time_limit: None,
        }
    }

    unsafe fn set_limit(
        &mut self,
        resource: __rlimit_resource_t,
        limit: usize,
    ) -> io::Result<()> {
        #[cfg(target_pointer_width = "32")]
        type PointerWidth = u32;
        #[cfg(target_pointer_width = "64")]
        type PointerWidth = u64;
        #[cfg(not(any(
            target_pointer_width = "32",
            target_pointer_width = "64",
        )))]
        compile_error!("unsupported pointer width");

        let limit = PointerWidth::try_from(limit)
            .expect("`usize` too large for pointer width");

        check_syscall(libc::prlimit(
            self.pid.as_pid(),
            resource,
            &rlimit {
                rlim_cur: limit,
                rlim_max: limit,
            },
            ptr::null_mut(),
        ))
    }

    pub(super) fn wait(&mut self) -> io::Result<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/process/process_unix.rs#L432-L441

        if let Some(memory_limit) = self.memory_limit {
            unsafe {
                self.set_limit(RLIMIT_AS, memory_limit)?;
            }
        }

        let pid = self.pid.0;
        run_with_timeout(
            move || loop {
                let mut process_info = MaybeUninit::uninit();
                let result = check_syscall(unsafe {
                    libc::waitid(
                        P_PID,
                        pid,
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
            self.time_limit,
        )?
        .transpose()
    }
}

#[derive(Debug)]
pub(super) struct DuplicatedHandle(RawPid);

impl DuplicatedHandle {
    #[allow(clippy::unnecessary_wraps)]
    pub(super) fn new(process: &Child) -> io::Result<Self> {
        Ok(Self(RawPid::new(process)))
    }

    pub(super) unsafe fn terminate(&self) -> io::Result<()> {
        check_syscall(libc::kill(self.0.as_pid(), SIGKILL)).map_err(|error| {
            // This error is usually decoded to [ErrorKind::Other]:
            // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/mod.rs#L100-L123
            if error.raw_os_error() == Some(ESRCH) {
                io::Error::new(io::ErrorKind::NotFound, "No such process")
            } else {
                error
            }
        })
    }
}
