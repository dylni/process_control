//! Implementations copied and modified from The Rust Programming Language.
//!
//! Sources:
//! - <https://github.com/rust-lang/rust/blob/835ed0021e149cacb2d464cdbc35816b5d551c0e/library/std/src/sys/windows/handle.rs>
//! - <https://github.com/rust-lang/rust/blob/835ed0021e149cacb2d464cdbc35816b5d551c0e/library/std/src/sys/windows/pipe.rs>
//!
//! Copyrights:
//! - Copyrights in the Rust project are retained by their contributors. No
//!   copyright assignment is required to contribute to the Rust project.
//!
//!   Some files include explicit copyright notices and/or license notices.
//!   For full authorship information, see the version control history or
//!   <https://thanks.rust-lang.org>
//!
//!   <https://github.com/rust-lang/rust/blob/835ed0021e149cacb2d464cdbc35816b5d551c0e/COPYRIGHT>
//! - Modifications copyright (c) 2024 dylni (<https://github.com/dylni>)<br>
//!   <https://github.com/dylni/normpath/blob/master/COPYRIGHT>

use std::io;
use std::mem;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ops::DerefMut;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::ptr;

use windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE;
use windows_sys::Win32::Foundation::ERROR_HANDLE_EOF;
use windows_sys::Win32::Foundation::ERROR_IO_PENDING;
use windows_sys::Win32::Foundation::FALSE;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::TRUE;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Threading::CreateEventW;
use windows_sys::Win32::System::Threading::WaitForMultipleObjects;
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::System::IO::CancelIo;
use windows_sys::Win32::System::IO::GetOverlappedResult;
use windows_sys::Win32::System::IO::OVERLAPPED;
use windows_sys::Win32::System::IO::OVERLAPPED_0;

use crate::control::Pipe;

macro_rules! static_assert {
    ( $condition:expr ) => {
        const _: () = assert!($condition, "static assertion failed");
    };
}

struct Event {
    inner: Box<OVERLAPPED>,
    _handle: OwnedHandle,
}

impl Event {
    fn new(manual_reset: bool, initial_state: bool) -> io::Result<Self> {
        let event = unsafe {
            CreateEventW(
                ptr::null_mut(),
                manual_reset.into(),
                initial_state.into(),
                ptr::null(),
            )
        };
        if event.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self {
                inner: Box::new(OVERLAPPED {
                    Internal: 0,
                    InternalHigh: 0,
                    Anonymous: OVERLAPPED_0 {
                        Pointer: ptr::null_mut(),
                    },
                    hEvent: event,
                }),
                _handle: unsafe { OwnedHandle::from_raw_handle(event) },
            })
        }
    }
}

impl Deref for Event {
    type Target = OVERLAPPED;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Event {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[inline(always)]
const fn u32_to_usize(n: u32) -> usize {
    // This assertion should never fail.
    static_assert!(size_of::<u32>() <= size_of::<usize>());
    n as usize
}

impl Pipe {
    fn raw(&self) -> HANDLE {
        self.inner.as_raw_handle()
    }

    fn overlapped_result(&self, event: &Event) -> io::Result<usize> {
        let mut read_length = 0;
        super::check_syscall(unsafe {
            GetOverlappedResult(self.raw(), &**event, &mut read_length, TRUE)
        })
        .map(|()| u32_to_usize(read_length))
        .or_else(|error| {
            if matches!(
                super::raw_os_error(&error),
                Some(ERROR_HANDLE_EOF | ERROR_BROKEN_PIPE)
            ) {
                Ok(0)
            } else {
                Err(error)
            }
        })
    }

    fn cancel_io(&self) -> io::Result<()> {
        super::check_syscall(unsafe { CancelIo(self.raw()) }).map(drop)
    }
}

struct AsyncPipe<'a> {
    inner: Pipe,
    event: ManuallyDrop<Event>,
    buffer: &'a mut Vec<u8>,
    reading: bool,
}

impl<'a> AsyncPipe<'a> {
    fn new(pipe: Pipe, buffer: &'a mut Vec<u8>) -> io::Result<Self> {
        debug_assert!(buffer.is_empty());

        Ok(Self {
            inner: pipe,
            event: ManuallyDrop::new(Event::new(true, true)?),
            buffer,
            reading: false,
        })
    }

    unsafe fn finish_read(&mut self, read_length: usize) -> io::Result<bool> {
        debug_assert!(read_length <= self.buffer.spare_capacity_mut().len());

        let index = self.buffer.len();
        unsafe {
            self.buffer.set_len(index + read_length);
        }
        let eof = read_length == 0;
        if !eof {
            self.buffer.reserve(1);
            self.inner.run_filter(self.buffer, index)?;
        }
        self.reading = false;
        Ok(!eof)
    }

    fn result(&mut self) -> io::Result<bool> {
        if !self.reading {
            return Ok(true);
        }
        self.inner
            .overlapped_result(&self.event)
            .and_then(|x| unsafe { self.finish_read(x) })
    }

    fn read_overlapped(&mut self) -> io::Result<Option<usize>> {
        debug_assert!(!self.reading);

        let buffer = self.buffer.spare_capacity_mut();
        let max_length = buffer.len().try_into().unwrap_or(u32::MAX);
        let mut length = 0;
        super::check_syscall(unsafe {
            ReadFile(
                self.inner.raw(),
                buffer.as_mut_ptr().cast(),
                max_length,
                &mut length,
                &mut **self.event,
            )
        })
        .map(|()| Some(u32_to_usize(length)))
        .or_else(|error| match super::raw_os_error(&error) {
            Some(ERROR_IO_PENDING) => Ok(None),
            Some(ERROR_BROKEN_PIPE) => Ok(Some(0)),
            _ => Err(error),
        })
    }

    fn next_result(&mut self) -> io::Result<bool> {
        macro_rules! continue_if_idle {
            ( $result:expr ) => {{
                let result = $result;
                if !matches!(result, Ok(true)) {
                    return result;
                }
            }};
        }

        continue_if_idle!(self.result());
        while let Some(read_length) = self.read_overlapped()? {
            continue_if_idle!(unsafe { self.finish_read(read_length) });
        }
        self.reading = true;
        Ok(true)
    }
}

impl Drop for AsyncPipe<'_> {
    fn drop(&mut self) {
        if self.reading
            && (self.inner.cancel_io().is_err() || self.result().is_err())
        {
            // Upon failure, overlapped IO operations may still be in progress,
            // so leaking memory is required to ensure that pointers remain
            // valid.
            mem::forget(mem::take(self.buffer));
        } else {
            unsafe {
                ManuallyDrop::drop(&mut self.event);
            }
        }
    }
}

pub(crate) fn read2(pipes: [Option<Pipe>; 2]) -> io::Result<[Vec<u8>; 2]> {
    let mut buffers = [(); 2].map(|()| Vec::with_capacity(32));

    let mut pipes: Vec<_> = pipes
        .into_iter()
        .zip(&mut buffers)
        .filter_map(|(pipe, buffer)| pipe.map(|x| AsyncPipe::new(x, buffer)))
        .collect::<Result<_, _>>()?;

    let events: Vec<_> = pipes.iter().map(|x| x.event.hEvent).collect();

    let mut start = 0;
    debug_assert!(events.len() <= 2);
    let mut length = events.len() as _;

    while length != 0 {
        let mut index = unsafe {
            WaitForMultipleObjects(
                length,
                events.as_ptr().add(start),
                FALSE,
                INFINITE,
            )
        }
        .checked_sub(WAIT_OBJECT_0)
        .filter(|&x| x < length)
        .map(|x| x as usize)
        .ok_or_else(io::Error::last_os_error)?;

        index += start;
        if !pipes[index].next_result()? {
            start = index ^ 1;
            length -= 1;
        }
    }
    drop(pipes);
    Ok(buffers)
}
