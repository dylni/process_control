use std::convert::TryInto;
use std::io;
use std::marker::PhantomData;
use std::os::raw::c_int;
use std::process::Child;
use std::time::Duration;

#[cfg(all(target_env = "gnu", target_os = "linux"))]
use libc::__rlimit_resource_t;
use libc::pid_t;
use libc::ESRCH;
use libc::SIGKILL;

use super::WaitResult;

macro_rules! if_waitid {
    ( $($item:item)+ ) => {
        $(
            #[cfg(process_control_unix_waitid)]
            $item
        )+
    };
}

mod exit_status;
pub(super) use exit_status::ExitStatus;

mod wait;

if_memory_limit! {
    use std::convert::TryFrom;
    use std::ptr;

    use libc::rlimit;
    use libc::RLIMIT_AS;
}

if_waitid! {
    use std::mem;

    use libc::id_t;
}

if_waitid! {
    macro_rules! static_assert {
        ( $condition:expr $(,)? ) => {
            const _: () = assert!($condition, "static assertion failed");
        };
    }
}

#[cfg(any(
    all(target_env = "musl", target_os = "linux"),
    target_os = "android",
))]
type LimitResource = c_int;
#[cfg(all(target_env = "gnu", target_os = "linux"))]
type LimitResource = __rlimit_resource_t;

fn check_syscall(result: c_int) -> io::Result<()> {
    if result >= 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Debug)]
struct RawPid(pid_t);

impl RawPid {
    fn new(process: &Child) -> Self {
        let pid: u32 = process.id();
        Self(pid.try_into().expect("process identifier is invalid"))
    }

    if_waitid! {
        const fn as_id(&self) -> id_t {
            static_assert!(pid_t::MAX == i32::MAX);
            static_assert!(mem::size_of::<pid_t>() <= mem::size_of::<id_t>());

            self.0 as _
        }
    }
}

#[derive(Debug)]
pub(super) struct Handle<'a> {
    #[cfg(not(process_control_unix_waitid))]
    process: &'a mut Child,
    #[cfg(any(process_control_memory_limit, process_control_unix_waitid))]
    pid: RawPid,
    _marker: PhantomData<&'a ()>,
}

impl<'a> Handle<'a> {
    pub(super) fn new(process: &'a mut Child) -> Self {
        Self {
            #[cfg(any(
                process_control_memory_limit,
                process_control_unix_waitid,
            ))]
            pid: RawPid::new(process),
            #[cfg(not(process_control_unix_waitid))]
            process,
            _marker: PhantomData,
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
                allow(clippy::useless_conversion)
            )]
            let limit = PointerWidth::try_from(limit)
                .expect("`usize` too large for pointer width")
                .into();

            check_syscall(unsafe {
                libc::prlimit(
                    self.pid.0,
                    resource,
                    &rlimit {
                        rlim_cur: limit,
                        rlim_max: limit,
                    },
                    ptr::null_mut(),
                )
            })
        }

        pub(super) fn set_memory_limit(
            &mut self,
            limit: usize,
        ) -> io::Result<()> {
            unsafe { self.set_limit(RLIMIT_AS, limit) }
        }
    }

    pub(super) fn wait(
        &mut self,
        time_limit: Option<Duration>,
    ) -> WaitResult<ExitStatus> {
        wait::wait(self, time_limit)
    }
}

#[derive(Debug)]
pub(super) struct DuplicatedHandle(RawPid);

impl DuplicatedHandle {
    pub(super) fn new(process: &Child) -> io::Result<Self> {
        Ok(Self(RawPid::new(process)))
    }

    #[rustfmt::skip]
    pub(super) unsafe fn terminate(&self) -> io::Result<()> {
        check_syscall(unsafe { libc::kill(self.0.0, SIGKILL) }).map_err(
            |error| {
                // This error is usually decoded to [ErrorKind::Uncategorized]:
                // https://github.com/rust-lang/rust/blob/11381a5a3a84ab1915d8c2a7ce369d4517c662a0/library/std/src/sys/unix/mod.rs#L138-L185
                if error.raw_os_error() == Some(ESRCH) {
                    io::Error::new(io::ErrorKind::NotFound, "No such process")
                } else {
                    error
                }
            },
        )
    }
}

pub(super) fn terminate_if_running(process: &mut Child) -> io::Result<()> {
    process.kill()
}
