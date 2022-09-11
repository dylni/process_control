use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::os::raw::c_int;
use std::os::unix::process::ExitStatusExt;
use std::process;

use libc::EXIT_SUCCESS;

if_waitid! {
    use libc::siginfo_t;
    use libc::CLD_CONTINUED;
    use libc::CLD_DUMPED;
    use libc::CLD_EXITED;
    use libc::CLD_KILLED;
    use libc::CLD_STOPPED;
    use libc::CLD_TRAPPED;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitStatusKind {
    Continued,
    Dumped,
    Exited,
    Killed,
    Stopped,
    #[cfg(process_control_unix_waitid)]
    Trapped,
    Uncategorized,
}

impl ExitStatusKind {
    if_waitid! {
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
}

macro_rules! code_method {
    ( $method:ident , $($kind_token:tt)+ ) => {
        pub(crate) fn $method(self) -> Option<c_int> {
            matches!(self.kind, $($kind_token)+).then(|| self.value)
        }
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExitStatus {
    value: c_int,
    kind: ExitStatusKind,
}

impl ExitStatus {
    if_waitid! {
        pub(super) unsafe fn new(process_info: siginfo_t) -> Self {
            Self {
                value: unsafe { process_info.si_status() },
                kind: ExitStatusKind::new(process_info.si_code),
            }
        }
    }

    pub(crate) fn success(self) -> bool {
        self.code() == Some(EXIT_SUCCESS)
    }

    pub(crate) fn continued(self) -> bool {
        self.kind == ExitStatusKind::Continued
    }

    pub(crate) fn core_dumped(self) -> bool {
        self.kind == ExitStatusKind::Dumped
    }

    code_method!(code, ExitStatusKind::Exited);
    code_method!(signal, ExitStatusKind::Dumped | ExitStatusKind::Killed);
    code_method!(stopped_signal, ExitStatusKind::Stopped);
}

impl Display for ExitStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.kind {
            ExitStatusKind::Continued => {
                f.write_str("continued (WIFCONTINUED)")
            }
            ExitStatusKind::Dumped => {
                write!(f, "signal: {} (core dumped)", self.value)
            }
            ExitStatusKind::Exited => write!(f, "exit code: {}", self.value),
            ExitStatusKind::Killed => write!(f, "signal: {}", self.value),
            ExitStatusKind::Stopped => {
                write!(f, "stopped (not terminated) by signal: {}", self.value)
            }
            #[cfg(process_control_unix_waitid)]
            ExitStatusKind::Trapped => f.write_str("trapped"),
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
                kind: if value.core_dumped() {
                    ExitStatusKind::Dumped
                } else {
                    ExitStatusKind::Killed
                },
            }
        } else if let Some(signal) = value.stopped_signal() {
            Self {
                value: signal,
                kind: ExitStatusKind::Stopped,
            }
        } else {
            Self {
                value: value.into_raw(),
                kind: if value.continued() {
                    ExitStatusKind::Continued
                } else {
                    ExitStatusKind::Uncategorized
                },
            }
        }
    }
}
