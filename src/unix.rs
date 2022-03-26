use std::convert::TryInto;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
use std::mem;
use std::mem::MaybeUninit;
use std::os::raw::c_int;
use std::os::unix::process::ExitStatusExt;
use std::process;
use std::process::Child;
use std::thread;
use std::time::Duration;

#[cfg(all(target_env = "gnu", target_os = "linux"))]
use libc::__rlimit_resource_t;
use libc::id_t;
use libc::pid_t;
use libc::CLD_CONTINUED;
use libc::CLD_DUMPED;
use libc::CLD_EXITED;
use libc::CLD_KILLED;
use libc::CLD_STOPPED;
use libc::CLD_TRAPPED;
use libc::EINTR;
use libc::ESRCH;
use libc::EXIT_SUCCESS;
use libc::P_PID;
use libc::SIGKILL;
use libc::WEXITED;
use libc::WNOWAIT;
use libc::WSTOPPED;

if_memory_limit! {
    use std::convert::TryFrom;
    use std::ptr;

    use libc::rlimit;
    use libc::RLIMIT_AS;
}

macro_rules! static_assert {
    ( $condition:expr ) => {
        const _: () = [()][if $condition { 0 } else { 1 }];
    };
}

#[cfg(any(
    all(target_env = "musl", target_os = "linux"),
    target_os = "android",
))]
type LimitResource = c_int;
#[cfg(all(target_env = "gnu", target_os = "linux"))]
type LimitResource = __rlimit_resource_t;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitStatusKind {
    Continued,
    Dumped,
    Exited,
    Killed,
    Stopped,
    Trapped,
    Uncategorized,
}

impl ExitStatusKind {
    const fn new(raw: c_int) -> Self {
        match raw {
            CLD_CONTINUED => Self::Continued,
            CLD_DUMPED => Self::Dumped,
            CLD_EXITED => Self::Exited,
            CLD_KILLED => Self::Killed,
            CLD_STOPPED => Self::Stopped,
            CLD_TRAPPED => Self::Trapped,
            _ => Self::Uncategorized,
        }
    }
}

macro_rules! code_method {
    ( $method:ident , $($kind_token:tt)+ ) => {
        pub(super) fn $method(self) -> Option<c_int> {
            matches!(self.kind, $($kind_token)+).then(|| self.value)
        }
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct ExitStatus {
    value: c_int,
    kind: ExitStatusKind,
}

impl ExitStatus {
    pub(super) fn success(self) -> bool {
        self.code() == Some(EXIT_SUCCESS)
    }

    pub(super) const fn continued(self) -> bool {
        matches!(self.kind, ExitStatusKind::Continued)
    }

    pub(super) const fn core_dumped(self) -> bool {
        matches!(self.kind, ExitStatusKind::Dumped)
    }

    code_method!(code, ExitStatusKind::Exited);
    code_method!(signal, ExitStatusKind::Dumped | ExitStatusKind::Killed);
    code_method!(stopped_signal, ExitStatusKind::Stopped);
}

impl Display for ExitStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.kind {
            ExitStatusKind::Continued => write!(f, "continued (WIFCONTINUED)"),
            ExitStatusKind::Dumped => {
                write!(f, "signal: {} (core dumped)", self.value)
            }
            ExitStatusKind::Exited => write!(f, "exit code: {}", self.value),
            ExitStatusKind::Killed => write!(f, "signal: {}", self.value),
            ExitStatusKind::Stopped => {
                write!(f, "stopped (not terminated) by signal: {}", self.value)
            }
            ExitStatusKind::Trapped => write!(f, "trapped"),
            ExitStatusKind::Uncategorized => {
                write!(f, "uncategorized wait status: {}", self.value)
            }
        }
    }
}

impl From<process::ExitStatus> for ExitStatus {
    fn from(value: process::ExitStatus) -> Self {
        if let Some(exit_code) = value.code() {
            Self {
                value: exit_code,
                kind: ExitStatusKind::Exited,
            }
        } else if let Some(signal) = value.signal() {
            Self {
                value: signal,
                kind: ExitStatusKind::Killed,
            }
        } else {
            Self {
                value: -1,
                kind: ExitStatusKind::Uncategorized,
            }
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

pub(super) fn run_with_time_limit<F, R>(
    run_fn: F,
    time_limit: Option<Duration>,
) -> io::Result<Option<R>>
where
    F: 'static + FnOnce() -> R + Send,
    R: 'static + Send,
{
    let time_limit = if let Some(time_limit) = time_limit {
        time_limit
    } else {
        return Ok(Some(run_fn()));
    };

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

    thread::Builder::new()
        .spawn(move || result_sender.send(run_fn()))
        .map(|_| result_receiver.recv_timeout(time_limit).ok())
}

const INVALID_PID_ERROR: &str = "process identifier is invalid";

#[derive(Debug)]
struct RawPid(pid_t);

impl RawPid {
    fn new(process: &Child) -> Self {
        let pid: u32 = process.id();
        Self(pid.try_into().expect(INVALID_PID_ERROR))
    }

    const fn as_id(&self) -> id_t {
        static_assert!(pid_t::MAX == i32::MAX);
        static_assert!(mem::size_of::<pid_t>() <= mem::size_of::<id_t>());

        self.0 as _
    }
}

#[derive(Debug)]
pub(super) struct SharedHandle {
    pid: RawPid,
    #[cfg(any(
        target_os = "android",
        all(
            target_os = "linux",
            any(target_env = "gnu", target_env = "musl"),
        ),
    ))]
    pub(super) memory_limit: Option<usize>,
    pub(super) time_limit: Option<Duration>,
}

impl SharedHandle {
    pub(super) unsafe fn new(process: &Child) -> Self {
        Self {
            pid: RawPid::new(process),
            #[cfg(any(
                target_os = "android",
                all(
                    target_os = "linux",
                    any(target_env = "gnu", target_env = "musl"),
                ),
            ))]
            memory_limit: None,
            time_limit: None,
        }
    }

    if_memory_limit! {
        unsafe fn set_limit(
            &mut self,
            resource: LimitResource,
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

            #[cfg_attr(
                not(target_os = "freebsd"),
                allow(clippy::useless_conversion),
            )]
            let limit = PointerWidth::try_from(limit)
                .expect("`usize` too large for pointer width")
                .into();

            check_syscall(libc::prlimit(
                self.pid.0,
                resource,
                &rlimit {
                    rlim_cur: limit,
                    rlim_max: limit,
                },
                ptr::null_mut(),
            ))
        }
    }

    pub(super) fn wait(&mut self) -> io::Result<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/process/process_unix.rs#L432-L441

        #[cfg(any(
            target_os = "android",
            all(
                target_os = "linux",
                any(target_env = "gnu", target_env = "musl"),
            ),
        ))]
        if let Some(memory_limit) = self.memory_limit {
            unsafe {
                self.set_limit(RLIMIT_AS, memory_limit)?;
            }
        }

        let pid = self.pid.as_id();
        run_with_time_limit(
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
                            kind: ExitStatusKind::new(process_info.si_code),
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

    #[rustfmt::skip]
    pub(super) unsafe fn terminate(&self) -> io::Result<()> {
        check_syscall(libc::kill(self.0.0, SIGKILL)).map_err(|error| {
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
