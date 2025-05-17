//! Implementations copied and modified from The Rust Programming Language.
//!
//! Sources:
//! - <https://github.com/rust-lang/rust/blob/835ed0021e149cacb2d464cdbc35816b5d551c0e/library/std/src/sys/unix/pipe.rs>
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
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::fd::RawFd;

use libc::fcntl;
use libc::pollfd;
use libc::F_GETFL;
use libc::F_SETFL;
use libc::O_NONBLOCK;
use libc::POLLIN;

use crate::control::Pipe;

impl Pipe {
    fn set_nonblocking(&mut self) -> io::Result<()> {
        let fd = self.as_raw_fd();
        let flags = unsafe { fcntl(fd, F_GETFL) };
        super::check_syscall(flags)?;
        super::check_syscall(unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) })
    }
}

impl AsRawFd for Pipe {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

struct AsyncPipe<'a> {
    inner: Pipe,
    buffer: &'a mut Vec<u8>,
}

impl<'a> AsyncPipe<'a> {
    fn new(mut pipe: Pipe, buffer: &'a mut Vec<u8>) -> io::Result<Self> {
        pipe.set_nonblocking()?;
        Ok(Self {
            inner: pipe,
            buffer,
        })
    }

    fn next_result(&mut self) -> io::Result<bool> {
        let index = self.buffer.len();
        let result = self
            .inner
            .inner
            .read_to_end(self.buffer)
            .map(|_| false)
            .or_else(|error| {
                if error.kind() == io::ErrorKind::WouldBlock {
                    Ok(true)
                } else {
                    Err(error)
                }
            })?;
        if self.buffer.len() != index {
            self.inner.run_filter(self.buffer, index)?;
        }
        Ok(result)
    }
}

pub(crate) fn read2(pipes: [Option<Pipe>; 2]) -> io::Result<[Vec<u8>; 2]> {
    const EMPTY_BUFFER: Vec<u8> = Vec::new();
    let mut buffers = [EMPTY_BUFFER; 2];

    let mut pipes: Vec<_> = pipes
        .into_iter()
        .zip(&mut buffers)
        .filter_map(|(pipe, buffer)| pipe.map(|x| AsyncPipe::new(x, buffer)))
        .collect::<Result<_, _>>()?;

    let mut fds: Vec<_> = pipes
        .iter_mut()
        .map(|pipe| pollfd {
            fd: pipe.inner.as_raw_fd(),
            events: POLLIN,
            revents: 0,
        })
        .collect();

    let mut start = 0;
    debug_assert!(fds.len() <= 2);
    let mut length: u8 = fds.len() as _;

    while length != 0 {
        let result = super::check_syscall(unsafe {
            libc::poll(fds.as_mut_ptr().add(start), length.into(), -1)
        });
        if let Err(error) = result {
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
            continue;
        }

        for i in (start..).take(length.into()) {
            if fds[i].revents != 0 && !pipes[i].next_result()? {
                start = i ^ 1;
                length -= 1;
            }
        }
    }
    Ok(buffers)
}
