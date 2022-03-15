use std::any;
use std::cell::Cell;
use std::convert::TryInto;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
use std::mem;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::ExitStatusExt;
use std::process;
use std::process::Child;
use std::ptr;
use std::time::Duration;

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

// https://github.com/microsoft/windows-rs/issues/881
#[allow(clippy::upper_case_acronyms)]
type BOOL = i32;
#[allow(clippy::upper_case_acronyms)]
type DWORD = u32;

const EXIT_SUCCESS: DWORD = 0;
const TRUE: BOOL = 1;

trait ReplaceNone<T> {
    fn replace_none(&self, value: T);
}

impl<T> ReplaceNone<T> for Cell<Option<T>>
where
    T: Debug,
{
    fn replace_none(&self, value: T) {
        let replaced = self.replace(Some(value));
        if let Some(replaced) = replaced {
            fail(&replaced);
        }

        #[inline(never)]
        #[cold]
        #[track_caller]
        fn fail(replaced: &dyn Debug) -> ! {
            panic!(
                "called `Cell::replace_none()` on a `Some` value: {:?}",
                replaced,
            );
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub(super) struct ExitStatus(DWORD);

impl ExitStatus {
    pub(super) const fn success(self) -> bool {
        self.0 == EXIT_SUCCESS
    }

    pub(super) fn code(self) -> Option<DWORD> {
        Some(self.0)
    }
}

impl Display for ExitStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&process::ExitStatus::from_raw(self.0), f)
    }
}

impl From<process::ExitStatus> for ExitStatus {
    fn from(value: process::ExitStatus) -> Self {
        Self(value.code().expect("process has no exit code") as u32)
    }
}

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
        check_syscall(GetExitCodeProcess(self.0, &mut exit_code))?;
        Ok(exit_code)
    }

    unsafe fn terminate(&self) -> io::Result<()> {
        let result = check_syscall(TerminateProcess(self.0, 1));
        if let Err(error) = &result {
            if let Some(error_code) = raw_os_error(error) {
                let not_found = match error_code {
                    ERROR_ACCESS_DENIED => {
                        matches!(
                            self.get_exit_code(),
                            Ok(x) if x.try_into() != Ok(STILL_ACTIVE),
                        )
                    }
                    // This error is usually decoded to [ErrorKind::Other]:
                    // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/mod.rs#L55-L82
                    ERROR_INVALID_HANDLE => true,
                    _ => false,
                };
                if not_found {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "The handle is invalid.",
                    ));
                }
            }
        }
        result
    }
}

// SAFETY: Process handles are thread-safe:
// https://stackoverflow.com/questions/12212628/win32-handles-and-multithread/12214212#12214212
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

struct JobHandle(Cell<Option<RawHandle>>);

impl JobHandle {
    fn close(&self) -> io::Result<()> {
        if let Some(handle) = self.0.take() {
            check_syscall(unsafe { CloseHandle(handle.0) })?;
        }
        Ok(())
    }
}

impl Debug for JobHandle {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(any::type_name::<Cell<Option<RawHandle>>>(), f)
    }
}

#[derive(Debug)]
pub(super) struct SharedHandle {
    handle: RawHandle,
    pub(super) memory_limit: Option<usize>,
    pub(super) time_limit: Option<Duration>,
    job_handle: JobHandle,
}

impl SharedHandle {
    pub(super) unsafe fn new(process: &Child) -> Self {
        Self {
            handle: RawHandle::new(process),
            memory_limit: None,
            time_limit: None,
            job_handle: JobHandle(Cell::new(None)),
        }
    }

    fn set_memory_limit(&mut self) -> io::Result<()> {
        self.job_handle.close()?;

        let memory_limit = if let Some(memory_limit) = self.memory_limit {
            memory_limit
        } else {
            return Ok(());
        };

        let job_handle =
            unsafe { CreateJobObjectW(ptr::null(), ptr::null_mut()) };
        if job_handle == 0 {
            return Err(io::Error::last_os_error());
        }
        self.job_handle.0.replace_none(RawHandle(job_handle));

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
            JobMemoryLimit: memory_limit,
            PeakProcessMemoryUsed: 0,
            PeakJobMemoryUsed: 0,
        };
        let job_information_ptr: *const _ = &job_information;
        let result = check_syscall(unsafe {
            SetInformationJobObject(
                job_handle,
                JobObjectExtendedLimitInformation,
                job_information_ptr.cast(),
                mem::size_of_val(&job_information)
                    .try_into()
                    .expect("job information too large for WinAPI"),
            )
        });
        match result {
            Ok(()) => {}
            // This error will occur when the job has a low memory limit.
            Err(ref error) => {
                return if raw_os_error(error) == Some(ERROR_INVALID_PARAMETER)
                {
                    self.job_handle.close()?;
                    unsafe { self.handle.terminate() }
                } else {
                    result
                };
            }
        }

        check_syscall(unsafe {
            AssignProcessToJobObject(job_handle, self.handle.0)
        })
    }

    pub(super) fn wait(&mut self) -> io::Result<Option<ExitStatus>> {
        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/windows/process.rs#L334-L344

        self.set_memory_limit()?;

        let mut remaining_time_limit = self
            .time_limit
            .map(|x| x.as_millis())
            .unwrap_or_else(|| INFINITE.into());
        while remaining_time_limit > 0 {
            let time_limit =
                remaining_time_limit.try_into().unwrap_or(DWORD::MAX);

            match unsafe { WaitForSingleObject(self.handle.0, time_limit) } {
                WAIT_OBJECT_0 => {
                    return unsafe { self.handle.get_exit_code() }
                        .map(|x| Some(ExitStatus(x)));
                }
                WAIT_TIMEOUT => {}
                _ => return Err(io::Error::last_os_error()),
            }

            remaining_time_limit -= u128::from(time_limit);
        }
        Ok(None)
    }
}

impl Drop for SharedHandle {
    fn drop(&mut self) {
        let _ = self.job_handle.close();
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
        self.0.terminate()
    }
}

impl Drop for DuplicatedHandle {
    fn drop(&mut self) {
        #[rustfmt::skip]
        let _ = unsafe { CloseHandle(self.0.0) };
    }
}
