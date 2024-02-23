use std::io;
use std::os::raw::c_int;
use std::process::Child;

use libc::pid_t;
use libc::ESRCH;

fn check_syscall(result: c_int) -> io::Result<()> {
    if result >= 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) struct Handle(pid_t);

impl Handle {
    pub(crate) fn new(process: &Child) -> io::Result<Self> {
        let pid = process.id();
        Ok(Self(pid.try_into().expect("process identifier is invalid")))
    }

    pub(crate) fn is_possibly_running(&self) -> io::Result<bool> {
        check_syscall(unsafe { libc::kill(self.0, 0) })
            .map(|()| true)
            .or_else(|error| {
                if error.raw_os_error() == Some(ESRCH) {
                    Ok(false)
                } else {
                    Err(error)
                }
            })
    }
}
