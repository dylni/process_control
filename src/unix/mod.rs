use std::io;
use std::marker::PhantomData;
pub(super) use std::os::fd::OwnedFd;
use std::os::raw::c_int;
use std::process::Child;
use std::time::Duration;

#[cfg(all(target_env = "gnu", target_os = "linux"))]
use libc::__rlimit_resource_t;

use super::WaitResult;

macro_rules! if_waitid {
    ( $($item:item)+ ) => {
    $(
        #[::attr_alias::eval]
        #[attr_alias(unix_waitid)]
        $item
    )+
    };
}

mod exit_status;
pub(super) use exit_status::ExitStatus;

mod read;
pub(super) use read::read2;

mod wait;

macro_rules! if_memory_limit {
    ( $($item:item)+ ) => {
    $(
        #[::attr_alias::eval]
        #[attr_alias(memory_limit)]
        $item
    )+
    };
}

if_memory_limit! {
    use std::ptr;

    use libc::rlimit;
    use libc::RLIMIT_AS;
}

macro_rules! if_raw_pid {
    ( $($item:item)+ ) => {
    $(
        #[::attr_alias::eval]
        #[attr_alias(raw_pid)]
        $item
    )+
    };
}

if_raw_pid! {
    use libc::pid_t;
}

if_waitid! {
    use libc::id_t;
}

if_waitid! {
    macro_rules! static_assert {
        ( $condition:expr ) => {
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

#[attr_alias::eval]
#[attr_alias(raw_pid)]
#[derive(Debug)]
struct RawPid(pid_t);

#[attr_alias::eval]
#[attr_alias(raw_pid)]
impl RawPid {
    fn new(process: &Child) -> Self {
        let pid: u32 = process.id();
        Self(pid.try_into().expect("process identifier is invalid"))
    }

    #[attr_alias(unix_waitid)]
    const fn as_id(&self) -> id_t {
        static_assert!(pid_t::MAX == i32::MAX);
        static_assert!(size_of::<pid_t>() <= size_of::<id_t>());

        self.0 as _
    }
}

#[attr_alias::eval]
#[derive(Debug)]
pub(super) struct Process<'a> {
    #[attr_alias(unix_waitid, cfg(not(*)))]
    inner: &'a mut Child,
    #[attr_alias(raw_pid)]
    pid: RawPid,
    _marker: PhantomData<&'a ()>,
}

#[attr_alias::eval]
impl<'a> Process<'a> {
    pub(super) fn new(process: &'a mut Child) -> Self {
        Self {
            #[attr_alias(raw_pid)]
            pid: RawPid::new(process),
            #[attr_alias(unix_waitid, cfg(not(*)))]
            inner: process,
            _marker: PhantomData,
        }
    }

    #[attr_alias(memory_limit)]
    fn set_limit(
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

    #[attr_alias(memory_limit)]
    pub(super) fn set_memory_limit(&mut self, limit: usize) -> io::Result<()> {
        self.set_limit(RLIMIT_AS, limit)
    }

    pub(super) fn wait(
        &mut self,
        time_limit: Option<Duration>,
    ) -> WaitResult<ExitStatus> {
        wait::wait(self, time_limit)
    }
}
