use std::any;
use std::cell::Cell;
use std::convert::TryInto;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Formatter;
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
use windows_sys::Win32::Foundation::DuplicateHandle;
use windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS;
use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows_sys::Win32::Foundation::ERROR_INVALID_HANDLE;
use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::STILL_ACTIVE;
use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
use windows_sys::Win32::System::JobObjects::JobObjectExtendedLimitInformation;
use windows_sys::Win32::System::JobObjects::SetInformationJobObject;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_BASIC_LIMIT_INFORMATION;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_JOB_MEMORY;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::TerminateProcess;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::System::Threading::IO_COUNTERS;
use windows_sys::Win32::System::Threading::WAIT_OBJECT_0;
use windows_sys::Win32::System::WindowsProgramming::INFINITE;

use super::WaitResult;

mod exit_status;
pub(super) use exit_status::ExitStatus;

macro_rules! assert_matches {
    ( $result:expr , $expected_result:pat $(,)? ) => {{
        let result = $result;
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

// https://github.com/microsoft/windows-rs/issues/881
#[allow(clippy::upper_case_acronyms)]
type BOOL = i32;
#[allow(clippy::upper_case_acronyms)]
type DWORD = u32;
type NonZeroDword = NonZeroU32;

const EXIT_SUCCESS: DWORD = 0;
const TRUE: BOOL = 1;

fn raw_os_error(error: &io::Error) -> Option<DWORD> {
    error.raw_os_error().and_then(|x| x.try_into().ok())
}

fn check_syscall(result: BOOL) -> io::Result<()> {
    if result == TRUE {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Debug)]
struct RawHandle(HANDLE);

impl RawHandle {
    fn new(process: &Child) -> Self {
        Self(process.as_raw_handle() as _)
    }

    unsafe fn get_exit_code(&self) -> io::Result<DWORD> {
        let mut exit_code = 0;
        check_syscall(unsafe { GetExitCodeProcess(self.0, &mut exit_code) })?;
        Ok(exit_code)
    }

    unsafe fn terminate(&self) -> io::Result<()> {
        check_syscall(unsafe { TerminateProcess(self.0, 1) })
    }

    unsafe fn is_not_running_error(&self, error: &io::Error) -> bool {
        raw_os_error(error) == Some(ERROR_ACCESS_DENIED)
            && matches!(
                unsafe { self.get_exit_code() },
                Ok(x) if x.try_into() != Ok(STILL_ACTIVE),
            )
    }

    unsafe fn terminate_if_running(&self) -> io::Result<()> {
        unsafe { self.terminate() }.or_else(|error| {
            if unsafe { self.is_not_running_error(&error) } {
                Ok(())
            } else {
                Err(error)
            }
        })
    }
}

// SAFETY: Process handles are thread-safe:
// https://stackoverflow.com/questions/12212628/win32-handles-and-multithread/12214212#12214212
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

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

impl Debug for JobHandle {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        any::type_name::<Cell<Option<RawHandle>>>().fmt(f)
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

struct TimeLimits<'a> {
    handle: &'a SharedHandle,
    start: Instant,
}

impl FusedIterator for TimeLimits<'_> {}

impl Iterator for TimeLimits<'_> {
    type Item = NonZeroDword;

    fn next(&mut self) -> Option<Self::Item> {
        let time_limit = if let Some(time_limit) = self.handle.time_limit {
            time_limit
        } else {
            const NON_ZERO_INFINITE: NonZeroDword =
                if let Some(result) = NonZeroDword::new(INFINITE) {
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
            .unwrap_or(DWORD::MAX);
        if time_limit == INFINITE {
            time_limit -= 1;
        }
        NonZeroDword::new(time_limit)
    }
}

#[derive(Debug)]
pub(super) struct SharedHandle {
    handle: RawHandle,
    pub(super) time_limit: Option<Duration>,
    job_handle: JobHandle,
}

impl SharedHandle {
    pub(super) unsafe fn new(process: &Child) -> Self {
        Self {
            handle: RawHandle::new(process),
            time_limit: None,
            job_handle: JobHandle(None),
        }
    }

    pub(super) fn set_memory_limit(&mut self, limit: usize) -> io::Result<()> {
        self.job_handle.close()?;

        let job_handle = self.job_handle.init()?;
        let job_information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
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
        let job_information_ptr: *const _ = &job_information;
        let result = check_syscall(unsafe {
            SetInformationJobObject(
                job_handle.0,
                JobObjectExtendedLimitInformation,
                job_information_ptr.cast(),
                mem::size_of_val(&job_information)
                    .try_into()
                    .expect("job information too large for WinAPI"),
            )
        });
        if let Err(error) = &result {
            // This error will occur when the job has a low memory limit.
            return if raw_os_error(error) == Some(ERROR_INVALID_PARAMETER) {
                unsafe { self.handle.terminate() }
            } else {
                result
            };
        }

        check_syscall(unsafe {
            AssignProcessToJobObject(job_handle.0, self.handle.0)
        })
    }

    fn time_limits(&self) -> TimeLimits<'_> {
        TimeLimits {
            handle: self,
            start: Instant::now(),
        }
    }

    pub(super) fn wait(&mut self) -> WaitResult<ExitStatus> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/process.rs#L334-L344

        for time_limit in self.time_limits() {
            match unsafe {
                WaitForSingleObject(self.handle.0, time_limit.get())
            } {
                WAIT_OBJECT_0 => {
                    return unsafe { self.handle.get_exit_code() }
                        .map(|x| Some(ExitStatus::new(x)));
                }
                WAIT_TIMEOUT => {}
                _ => return Err(io::Error::last_os_error()),
            }
        }
        Ok(None)
    }
}

#[derive(Debug)]
pub(super) struct DuplicatedHandle(RawHandle);

impl DuplicatedHandle {
    pub(super) fn new(process: &Child) -> io::Result<Self> {
        let parent_handle = unsafe { GetCurrentProcess() };
        let mut handle = 0;
        check_syscall(unsafe {
            DuplicateHandle(
                parent_handle,
                RawHandle::new(process).0,
                parent_handle,
                &mut handle,
                0,
                TRUE,
                DUPLICATE_SAME_ACCESS,
            )
        })?;
        Ok(Self(RawHandle(handle)))
    }

    pub(super) unsafe fn terminate(&self) -> io::Result<()> {
        unsafe { self.0.terminate() }.map_err(|error| {
            // This error is usually decoded to [ErrorKind::Uncategorized]:
            // https://github.com/rust-lang/rust/blob/11381a5a3a84ab1915d8c2a7ce369d4517c662a0/library/std/src/sys/windows/mod.rs#L63-L128
            if unsafe { self.0.is_not_running_error(&error) }
                || raw_os_error(&error) == Some(ERROR_INVALID_HANDLE)
            {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "The handle is invalid.",
                )
            } else {
                error
            }
        })
    }
}

impl Drop for DuplicatedHandle {
    fn drop(&mut self) {
        #[rustfmt::skip]
        let _ = unsafe { CloseHandle(self.0.0) };
    }
}

pub(super) fn terminate_if_running(process: &mut Child) -> io::Result<()> {
    unsafe { RawHandle::new(process).terminate_if_running() }
}
