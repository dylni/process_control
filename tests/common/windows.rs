use std::io;
use std::os::windows::io::AsHandle;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::OwnedHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
use windows_sys::Win32::System::Threading::WaitForSingleObject;

fn raw_handle<T>(handle: &T) -> HANDLE
where
    T: AsRawHandle + ?Sized,
{
    handle.as_raw_handle() as _
}

pub(crate) struct Handle(OwnedHandle);

impl Handle {
    pub(crate) fn new(process: &Child) -> io::Result<Self> {
        process.as_handle().try_clone_to_owned().map(Self)
    }

    pub(crate) fn is_possibly_running(&self) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(raw_handle(&self.0), 0) } {
            WAIT_OBJECT_0 => Ok(false),
            WAIT_TIMEOUT => Ok(true),
            _ => Err(io::Error::last_os_error()),
        }
    }
}
