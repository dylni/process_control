use std::convert::TryInto;
use std::io;
use std::iter::FusedIterator;
use std::mem;
use std::num::NonZeroU32;
use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::ptr;
use std::time::Duration;
use std::time::Instant;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::BOOL;
use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::STILL_ACTIVE;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
use windows_sys::Win32::System::JobObjects::JobObjectExtendedLimitInformation;
use windows_sys::Win32::System::JobObjects::SetInformationJobObject;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_BASIC_LIMIT_INFORMATION;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_JOB_MEMORY;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::System::Threading::IO_COUNTERS;

use super::WaitResult;

mod exit_status;
pub(super) use exit_status::ExitStatus;

macro_rules! assert_matches {
    ( $result:expr , $expected_result:pat $(,)? ) => {{
        let result = $result;
        #[allow(clippy::redundant_pattern_matching)]
        if !matches!(result, $expected_result) {
            panic!(
                "assertion failed: `(left matches right)`
  left: `{:?}`
 right: `{:?}`",
                result,
                stringify!($expected_result),
            );
        }
    }};
}

const EXIT_SUCCESS: u32 = 0;
const TRUE: BOOL = 1;

fn raw_os_error(error: &io::Error) -> Option<u32> {
    error.raw_os_error().and_then(|x| x.try_into().ok())
}

fn check_syscall(result: BOOL) -> io::Result<()> {
    if result == TRUE {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

const fn size_of_val_raw<T>(_: *const T) -> usize {
    mem::size_of::<T>()
}

#[derive(Debug)]
struct RawHandle(HANDLE);

impl RawHandle {
    fn new(process: &Child) -> Self {
        Self(process.as_raw_handle() as _)
    }
}

// SAFETY: Process handles are thread-safe:
// https://stackoverflow.com/questions/12212628/win32-handles-and-multithread/12214212#12214212
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

#[derive(Debug)]
struct JobHandle(Option<RawHandle>);

impl JobHandle {
    fn init(&mut self) -> io::Result<&RawHandle> {
        assert_matches!(&self.0, None);

        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null_mut()) };
        if handle == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(self.0.insert(RawHandle(handle)))
    }

    fn close(&mut self) -> io::Result<()> {
        if let Some(handle) = self.0.take() {
            check_syscall(unsafe { CloseHandle(handle.0) })?;
        }
        Ok(())
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

struct TimeLimits {
    time_limit: Option<Duration>,
    start: Instant,
}

impl TimeLimits {
    fn new(time_limit: Option<Duration>) -> Self {
        Self {
            time_limit,
            start: Instant::now(),
        }
    }
}

impl FusedIterator for TimeLimits {}

impl Iterator for TimeLimits {
    type Item = NonZeroU32;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(time_limit) = self.time_limit else {
            const NON_ZERO_INFINITE: NonZeroU32 =
                if let Some(result) = NonZeroU32::new(INFINITE) {
                    result
                } else {
                    unreachable!();
                };

            return Some(NON_ZERO_INFINITE);
        };

        let mut time_limit = time_limit
            .saturating_sub(self.start.elapsed())
            .as_millis()
            .try_into()
            .unwrap_or(u32::MAX);
        if time_limit == INFINITE {
            time_limit -= 1;
        }
        NonZeroU32::new(time_limit)
    }
}

#[derive(Debug)]
pub(super) struct Handle<'a> {
    process: &'a mut Child,
    handle: RawHandle,
    job_handle: JobHandle,
}

impl<'a> Handle<'a> {
    pub(super) fn new(process: &'a mut Child) -> Self {
        Self {
            handle: RawHandle::new(process),
            process,
            job_handle: JobHandle(None),
        }
    }

    fn get_exit_code(&self) -> io::Result<u32> {
        let mut exit_code = 0;
        check_syscall(unsafe {
            GetExitCodeProcess(self.handle.0, &mut exit_code)
        })?;
        Ok(exit_code)
    }

    pub(super) fn set_memory_limit(&mut self, limit: usize) -> io::Result<()> {
        self.job_handle.close()?;

        let job_handle = self.job_handle.init()?;
        let job_information: *const _ =
            &JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    PerProcessUserTimeLimit: 0,
                    PerJobUserTimeLimit: 0,
                    LimitFlags: JOB_OBJECT_LIMIT_JOB_MEMORY,
                    MinimumWorkingSetSize: 0,
                    MaximumWorkingSetSize: 0,
                    ActiveProcessLimit: 0,
                    Affinity: 0,
                    PriorityClass: 0,
                    SchedulingClass: 0,
                },
                IoInfo: IO_COUNTERS {
                    ReadOperationCount: 0,
                    WriteOperationCount: 0,
                    OtherOperationCount: 0,
                    ReadTransferCount: 0,
                    WriteTransferCount: 0,
                    OtherTransferCount: 0,
                },
                ProcessMemoryLimit: 0,
                JobMemoryLimit: limit,
                PeakProcessMemoryUsed: 0,
                PeakJobMemoryUsed: 0,
            };
        let result = check_syscall(unsafe {
            SetInformationJobObject(
                job_handle.0,
                JobObjectExtendedLimitInformation,
                job_information.cast(),
                size_of_val_raw(job_information)
                    .try_into()
                    .expect("job information too large for WinAPI"),
            )
        });
        if let Err(error) = &result {
            // This error will occur when the job has a low memory limit.
            return if raw_os_error(error) == Some(ERROR_INVALID_PARAMETER) {
                self.process.kill()
            } else {
                result
            };
        }

        check_syscall(unsafe {
            AssignProcessToJobObject(job_handle.0, self.handle.0)
        })
    }

    pub(super) fn wait(
        &mut self,
        time_limit: Option<Duration>,
    ) -> WaitResult<ExitStatus> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/process.rs#L334-L344

        for time_limit in TimeLimits::new(time_limit) {
            match unsafe {
                WaitForSingleObject(self.handle.0, time_limit.get())
            } {
                WAIT_OBJECT_0 => {
                    return self
                        .get_exit_code()
                        .map(|x| Some(ExitStatus::new(x)));
                }
                WAIT_TIMEOUT => {}
                _ => return Err(io::Error::last_os_error()),
            }
        }
        Ok(None)
    }
}

pub(super) fn terminate_if_running(process: &mut Child) -> io::Result<()> {
    process.kill().or_else(|error| {
        if raw_os_error(&error) == Some(ERROR_ACCESS_DENIED)
            && matches!(
                Handle::new(process).get_exit_code(),
                Ok(x) if x.try_into() != Ok(STILL_ACTIVE),
            )
        {
            Ok(())
        } else {
            Err(error)
        }
    })
}
