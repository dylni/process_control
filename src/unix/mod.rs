use std::marker::PhantomData;
use std::process::Child;
use std::time::Duration;

#[cfg(all(target_env = "gnu", target_os = "linux"))]
use libc::__rlimit_resource_t;

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

macro_rules! if_memory_limit {
    ( $($item:item)+ ) => {
    $(
        #[cfg(process_control_memory_limit)]
        $item
    )+
    };
}

if_memory_limit! {
    use std::convert::TryFrom;
    use std::ptr;

    use libc::rlimit;
    use libc::RLIMIT_AS;
}

macro_rules! if_raw_pid {
    ( $($item:item)+ ) => {
    $(
        #[cfg(any(process_control_memory_limit, process_control_unix_waitid))]
        $item
    )+
    };
}

if_raw_pid! {
    use std::convert::TryInto;
    use std::io;
    use std::os::raw::c_int;

    use libc::pid_t;
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

if_raw_pid! {
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
                static_assert!(
                    mem::size_of::<pid_t>() <= mem::size_of::<id_t>(),
                );

                self.0 as _
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct Process<'a> {
    #[cfg(not(process_control_unix_waitid))]
    inner: &'a mut Child,
    #[cfg(any(process_control_memory_limit, process_control_unix_waitid))]
    pid: RawPid,
    _marker: PhantomData<&'a ()>,
}

impl<'a> Process<'a> {
    pub(super) fn new(process: &'a mut Child) -> Self {
        Self {
            #[cfg(any(
                process_control_memory_limit,
                process_control_unix_waitid,
            ))]
            pid: RawPid::new(process),
            #[cfg(not(process_control_unix_waitid))]
            inner: process,
            _marker: PhantomData,
        }
    }

    if_memory_limit! {
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

        pub(super) fn set_memory_limit(
            &mut self,
            limit: usize,
        ) -> io::Result<()> {
            self.set_limit(RLIMIT_AS, limit)
        }
    }

    pub(super) fn wait(
        &mut self,
        time_limit: Option<Duration>,
    ) -> WaitResult<ExitStatus> {
        wait::wait(self, time_limit)
    }
}
