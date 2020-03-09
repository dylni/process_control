//! This crate allows terminating a process without a mutable reference.
//! [`ProcessTerminator::terminate`] is designed to operate in this manner and
//! is the reason this crate exists. It intentionally does not require a
//! reference of any kind to the [`Child`] instance, allowing for maximal
//! flexibility in working with processes.
//!
//! Typically, it is not possible to terminate a process during a call to
//! [`Child::wait`] or [`Child::wait_with_output`] in another thread, since
//! [`Child::kill`] takes a mutable reference. However, since this crate
//! creates its own termination method, there is no issue, allowing system
//! resources to be freed when using methods such as
//! [`ChildExt::with_output_timeout`].
//!
//! Crate [wait-timeout] has a similar purpose, but it does not provide the
//! same flexibility. It does not allow reading the entire output of a process
//! within the time limit or terminating a process based on other signals. This
//! crate aims to fill in those gaps and simplify the implementation, now that
//! [`Receiver::recv_timeout`] exists.
//!
//! # Implementation
//!
//! All traits are [sealed], meaning that they can only be implemented by this
//! crate. Otherwise, backward compatibility would be more difficult to
//! maintain for new features.
//!
//! # Examples
//!
//! ```
//! use std::io::Error as IoError;
//! use std::io::ErrorKind as IoErrorKind;
//! # use std::io::Result as IoResult;
//! use std::process::Command;
//! use std::process::Stdio;
//! use std::time::Duration;
//!
//! use process_control::ChildExt;
//!
//! # fn main() -> IoResult<()> {
//! let process = Command::new("echo")
//!     .arg("hello")
//!     .stdout(Stdio::piped())
//!     .spawn()?;
//!
//! let output = process
//!     .with_output_timeout(Duration::from_secs(1))
//!     .terminating()
//!     .wait()?
//!     .ok_or_else(|| {
//!         IoError::new(IoErrorKind::TimedOut, "Process timed out")
//!     })?;
//! assert_eq!(b"hello", &output.stdout[..5]);
//! #     Ok(())
//! # }
//! ```
//!
//! [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
//! [`Child::kill`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.kill
//! [`Child::wait`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait
//! [`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
//! [`ChildExt::with_output_timeout`]: trait.ChildExt.html#tymethod.with_output_timeout
//! [`ProcessTerminator::terminate`]: struct.ProcessTerminator.html#method.terminate
//! [`Receiver::recv_timeout`]: https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html#method.recv_timeout
//! [sealed]: https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed
//! [wait-timeout]: https://crates.io/crates/wait-timeout

#![doc(
    html_root_url = "https://docs.rs/process_control/*",
    test(attr(deny(warnings)))
)]

use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::process::Child;
use std::process::ExitStatus;
use std::process::Output;
use std::time::Duration;

mod common;

#[cfg(unix)]
#[path = "unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

/// A wrapper that stores enough information to terminate a process.
///
/// Instances can only be constructed using [`ChildExt::terminator`].
///
/// [`ChildExt::terminator`]: trait.ChildExt.html#tymethod.terminator
#[derive(Debug)]
pub struct ProcessTerminator(imp::Handle);

impl ProcessTerminator {
    /// Terminates a process as immediately as the operating system allows.
    ///
    /// Behavior should be equivalent to calling [`Child::kill`] for the same
    /// process. The guarantees on the result of that method are also
    /// maintained; different [`ErrorKind`] variants may be returned in the
    /// future for the same type of failure. Allowing these breakages is
    /// required to be compatible with the [`Error`] type.
    ///
    /// # Panics
    ///
    /// Panics if the operating system gives conflicting indicators of whether
    /// the termination signal was accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Result as IoResult;
    /// use std::process::Command;
    /// use std::thread;
    /// use std::thread::JoinHandle;
    ///
    /// use process_control::ChildExt;
    ///
    /// # fn main() -> IoResult<()> {
    /// let mut process = Command::new("echo").spawn()?;
    /// let process_terminator = process.terminator();
    ///
    /// let thread: JoinHandle<IoResult<_>> = thread::spawn(move || {
    ///     process.wait()?;
    ///     println!("waited");
    ///     Ok(())
    /// });
    ///
    /// // [process.kill] requires a mutable reference.
    /// process_terminator.terminate()?;
    /// thread.join().expect("thread panicked")?;
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`Child::kill`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.kill
    /// [`Error`]: https://doc.rust-lang.org/std/io/struct.Error.html
    /// [`ErrorKind`]: https://doc.rust-lang.org/std/io/enum.ErrorKind.html
    #[inline]
    pub fn terminate(&self) -> IoResult<()> {
        self.0.terminate()
    }

    /// Terminates a process as immediately as the operating system allows,
    /// ignoring errors about the process no longer existing.
    ///
    /// For more information, see [`terminate`].
    ///
    /// [`terminate`]: #method.terminate
    #[inline]
    pub fn terminate_if_necessary(&self) -> IoResult<()> {
        let result = self.terminate();
        if let Err(error) = &result {
            if error.kind() == IoErrorKind::NotFound {
                return Ok(());
            }
        }
        result
    }
}

#[deprecated(since = "0.4.0", note = "use `ChildExt` instead")]
pub trait Terminator: private::Sealed + Sized {
    #[deprecated(since = "0.4.0", note = "use `ChildExt::terminator` instead")]
    #[must_use]
    fn terminator(&self) -> ProcessTerminator;

    #[deprecated(
        since = "0.4.0",
        note = "use `ChildExt::with_timeout` instead"
    )]
    fn wait_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>>;

    #[deprecated(
        since = "0.4.0",
        note = "use `ChildExt::with_output_timeout` instead"
    )]
    fn wait_for_output_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>>;

    #[deprecated(
        since = "0.4.0",
        note = "use `ChildExt::with_timeout` instead"
    )]
    fn wait_with_terminating_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>>;

    #[deprecated(
        since = "0.4.0",
        note = "use `ChildExt::with_output_timeout` instead"
    )]
    fn wait_for_output_with_terminating_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>>;
}

#[allow(deprecated)]
impl Terminator for Child {
    #[inline]
    fn terminator(&self) -> ProcessTerminator {
        ChildExt::terminator(self)
    }

    #[inline]
    fn wait_with_timeout(
        mut self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>> {
        self.with_timeout(time_limit)
            .wait()
            .map(|x| x.map(|x| (x, self)))
    }

    #[inline]
    fn wait_for_output_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>> {
        self.with_output_timeout(time_limit).wait()
    }

    #[inline]
    fn wait_with_terminating_timeout(
        mut self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>> {
        self.with_timeout(time_limit)
            .terminating()
            .wait()
            .map(|x| x.map(|x| (x, self)))
    }

    #[inline]
    fn wait_for_output_with_terminating_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>> {
        self.with_output_timeout(time_limit).terminating().wait()
    }
}

macro_rules! r#impl {
    (
        $struct:ident $(< $lifetime:lifetime >)? ,
        $process_type:ty ,
        $return_type:ty ,
        $wait_fn:expr $(,)?
    ) => {
        /// A temporary wrapper for a process timeout.
        ///
        /// **Do not use this type explicitly.** It is not part of the backward
        /// compatibility guarantee of this crate. Only its methods should be
        /// used.
        #[derive(Debug)]
        pub struct $struct$(<$lifetime>)? {
            process: $process_type,
            handle: imp::Handle,
            time_limit: Duration,
            terminator: Option<ProcessTerminator>,
        }

        impl$(<$lifetime>)? $struct$(<$lifetime>)? {
            fn new(process: $process_type, time_limit: Duration) -> Self {
                Self {
                    handle: imp::Handle::new(&process),
                    process,
                    time_limit,
                    terminator: None,
                }
            }

            /// Causes the process to be terminated if it exceeds the time
            /// limit.
            ///
            /// Errors terminating the process will be ignored, as they are
            /// often less important than the result. To catch those errors,
            /// [`ProcessTerminator::terminate`] should be called explicitly
            /// instead.
            ///
            /// [`ProcessTerminator::terminate`]: struct.ProcessTerminator.html#method.terminate
            #[inline]
            #[must_use]
            pub fn terminating(mut self) -> Self {
                self.terminator =
                    Some(<Child as ChildExt>::terminator(&self.process));
                self
            }

            /// Runs the process to completion, aborting if it exceeds the time
            /// limit.
            ///
            /// A separate thread will be created to wait on the process
            /// without blocking the current thread.
            ///
            /// If the time limit is exceeded before the process finishes,
            /// `Ok(None)` will be returned. However, the process will not be
            /// terminated in that case unless [`terminating`] is called
            /// beforehand. It is recommended to always call that method to
            /// allow system resources to be freed.
            ///
            /// The stdin handle to the process, if it exists, will be closed
            /// before waiting. Otherwise, the process would be guaranteed to
            /// time out.
            ///
            /// [`terminating`]: #method.terminating
            #[inline]
            pub fn wait(mut self) -> IoResult<Option<$return_type>> {
                self.process.stdin.take();

                let terminator = self.terminator.take();
                let result = $wait_fn(self);
                if let Some(terminator) = terminator {
                    // Errors terminating a process are less important than the
                    // result.
                    let _ = terminator.terminate();
                }
                result
            }
        }
    };
}

r#impl!(
    _ExitStatusTimeout<'a>,
    &'a mut Child,
    ExitStatus,
    |x: Self| x.handle.wait_with_timeout(x.time_limit),
);

r#impl!(_OutputTimeout, Child, Output, |x: Self| {
    let time_limit = x.time_limit;
    common::run_with_timeout(|| x.process.wait_with_output(), time_limit)?
        .transpose()
});

/// Extensions to [`Child`] for easily terminating processes.
///
/// For more information, see [the module-level documentation][module].
///
/// [module]: index.html
/// [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
pub trait ChildExt: private::Sealed {
    /// Creates an instance of [`ProcessTerminator`] for this process.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io::Result as IoResult;
    /// use std::process::Command;
    ///
    /// use process_control::ChildExt;
    ///
    /// # fn main() -> IoResult<()> {
    /// let process = Command::new("echo").spawn()?;
    /// # #[allow(unused_variables)]
    /// let process_terminator = process.terminator();
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`ProcessTerminator`]: struct.ProcessTerminator.html
    #[must_use]
    fn terminator(&self) -> ProcessTerminator;

    /// Creates an instance of [`_ExitStatusTimeout`] for this process.
    ///
    /// This method parallels [`Child::wait`] when the process must finish
    /// within a time limit.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io::Result as IoResult;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    ///
    /// # fn main() -> IoResult<()> {
    /// let exit_status = Command::new("echo")
    ///     .spawn()?
    ///     .with_timeout(Duration::from_secs(1))
    ///     .terminating()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(exit_status.success());
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`Child::wait`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait
    /// [`_ExitStatusTimeout`]: struct._ExitStatusTimeout.html
    #[must_use]
    fn with_timeout(&mut self, time_limit: Duration)
        -> _ExitStatusTimeout<'_>;

    /// Creates an instance of [`_OutputTimeout`] for this process.
    ///
    /// This method parallels [`Child::wait_with_output`] when the process must
    /// finish within a time limit.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io::Result as IoResult;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    ///
    /// # fn main() -> IoResult<()> {
    /// let output = Command::new("echo")
    ///     .spawn()?
    ///     .with_output_timeout(Duration::from_secs(1))
    ///     .terminating()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(output.status.success());
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
    /// [`_OutputTimeout`]: struct._OutputTimeout.html
    #[must_use]
    fn with_output_timeout(self, time_limit: Duration) -> _OutputTimeout;
}

impl ChildExt for Child {
    #[inline]
    fn terminator(&self) -> ProcessTerminator {
        ProcessTerminator(imp::Handle::new(self))
    }

    #[inline]
    fn with_timeout(
        &mut self,
        time_limit: Duration,
    ) -> _ExitStatusTimeout<'_> {
        _ExitStatusTimeout::new(self, time_limit)
    }

    #[inline]
    fn with_output_timeout(self, time_limit: Duration) -> _OutputTimeout {
        _OutputTimeout::new(self, time_limit)
    }
}

mod private {
    use std::process::Child;

    pub trait Sealed {}
    impl Sealed for Child {}
}
