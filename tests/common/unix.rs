use std::convert::TryInto;
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

pub(crate) struct Handle(u32);

impl Handle {
    pub(crate) fn new(process: &Child) -> io::Result<Self> {
        Ok(Self(process.id()))
    }

    fn as_pid(&self) -> pid_t {
        self.0.try_into().expect("process identifier is invalid")
    }

    pub(crate) unsafe fn is_running(&self) -> io::Result<bool> {
        check_syscall(libc::kill(self.as_pid(), 0))
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
