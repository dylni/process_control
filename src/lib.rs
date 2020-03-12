//! This crate allows running a process with a timeout, with the option to
//! terminate it automatically afterward. The latter is surprisingly difficult
//! to achieve on Unix, since process identifiers can be arbitrarily reassigned
//! when no longer used. Thus, it would be extremely easy to inadvertently
//! terminate an unexpected process. This crate protects against that
//! possibility.
//!
//! Methods for creating timeouts are available on [`ChildExt`], which is
//! implemented for [`Child`]. They each return a builder of options to
//! configure how the timeout should be applied.
//!
//! # Implementation
//!
//! All traits are [sealed], meaning that they can only be implemented by this
//! crate. Otherwise, backward compatibility would be more difficult to
//! maintain for new features.
//!
//! # Related Crates
//!
//! Crate [wait-timeout] has a similar purpose, but it does not provide the
//! same functionality. Processes cannot be terminated automatically, and there
//! is no counterpart to [`Child::wait_with_output`] for simply reading output
//! from a process with a timeout. This crate aims to fill in those gaps and
//! simplify the implementation, now that [`Receiver::recv_timeout`] exists.
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
//! [`ChildExt`]: trait.ChildExt.html
//! [`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
//! [`Receiver::recv_timeout`]: https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html#method.recv_timeout
//! [sealed]: https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed
//! [wait-timeout]: https://crates.io/crates/wait-timeout

#![doc(
    html_root_url = "https://docs.rs/process_control/*",
    test(attr(deny(warnings)))
)]
#![warn(unused_results)]

use std::io::Read;
use std::io::Result as IoResult;
use std::os::raw::c_uint;
use std::process::Child;
use std::process::ExitStatus as ProcessExitStatus;
use std::time::Duration;

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
    /// process. However, this method does not require a reference of any kind
    /// to the [`Child`] instance of the process, meaning that it can be called
    /// even in some unsafe circumstances.
    ///
    /// # Safety
    ///
    /// If the process is no longer running, a different process may be
    /// terminated on some operating systems. Reuse of process identifiers
    /// makes it impossible for this method to determine if the intended
    /// process still exists.
    ///
    /// Thus, this method should not be used in production code, as
    /// [`Child::kill`] more safely provides the same functionality. It is only
    /// used for testing in this crate and may be used similarly in others.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io::Result as IoResult;
    /// use std::path::Path;
    /// use std::process::Command;
    /// use std::thread;
    ///
    /// use process_control::ChildExt;
    ///
    /// # fn main() -> IoResult<()> {
    /// let dir = Path::new("hello");
    /// let mut process = Command::new("mkdir").arg(dir).spawn()?;
    /// let process_terminator = process.terminator()?;
    ///
    /// let thread = thread::spawn(move || process.wait());
    /// if !dir.exists() {
    ///     // [process.kill] requires a mutable reference.
    ///     unsafe { process_terminator.terminate()? }
    /// }
    ///
    /// let exit_status = thread.join().expect("thread panicked")?;
    /// println!("exited {}", exit_status);
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
    /// [`Child::kill`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.kill
    #[inline]
    pub unsafe fn terminate(&self) -> IoResult<()> {
        self.0.terminate()
    }
}

/// Equivalent to [`ExitStatus`] in the standard library but allows for greater
/// accuracy.
///
/// [`ExitStatus`]: https://doc.rust-lang.org/std/process/struct.ExitStatus.html
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ExitStatus(imp::ExitStatus);

impl ExitStatus {
    /// Equivalent to [`ExitStatus::success`].
    ///
    /// [`ExitStatus::success`]: https://doc.rust-lang.org/std/process/struct.ExitStatus.html#method.success
    #[inline]
    #[must_use]
    pub fn success(self) -> bool {
        self.0.success()
    }

    /// Equivalent to [`ExitStatus::code`].
    ///
    /// [`ExitStatus::code`]: https://doc.rust-lang.org/std/process/struct.ExitStatus.html#method.code
    #[inline]
    #[must_use]
    pub fn code(self) -> Option<c_uint> {
        self.0.code()
    }

    /// Equivalent to [`ExitStatusExt::signal`].
    ///
    /// *This method is only available on Unix systems.*
    ///
    /// [`ExitStatusExt::signal`]: https://doc.rust-lang.org/std/os/unix/process/trait.ExitStatusExt.html#tymethod.signal
    #[cfg(unix)]
    #[inline]
    #[must_use]
    pub fn signal(self) -> Option<c_uint> {
        self.0.signal()
    }
}

impl From<ProcessExitStatus> for ExitStatus {
    #[inline]
    fn from(status: ProcessExitStatus) -> Self {
        Self(status.into())
    }
}

/// Equivalent to [`Output`] in the standard library but holds an instance of
/// [`ExitStatus`] from this crate.
///
/// [`ExitStatus`]: struct.ExitStatus.html
/// [`Output`]: https://doc.rust-lang.org/std/process/struct.Output.html
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Output {
    /// Equivalent to [`Output::status`].
    ///
    /// [`Output::status`]: https://doc.rust-lang.org/std/process/struct.Output.html#structfield.status
    pub status: ExitStatus,

    /// Equivalent to [`Output::stdout`].
    ///
    /// [`Output::stdout`]: https://doc.rust-lang.org/std/process/struct.Output.html#structfield.stdout
    pub stdout: Vec<u8>,

    /// Equivalent to [`Output::stderr`].
    ///
    /// [`Output::stderr`]: https://doc.rust-lang.org/std/process/struct.Output.html#structfield.stderr
    pub stderr: Vec<u8>,
}

#[deprecated(since = "0.4.0", note = "use `ChildExt` instead")]
pub trait Terminator: private::Sealed + Sized {
    #[deprecated(since = "0.4.0", note = "use `ChildExt::terminator` instead")]
    fn terminator(&self) -> IoResult<ProcessTerminator>;

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
    fn terminator(&self) -> IoResult<ProcessTerminator> {
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
        $create_result_fn:expr $(,)?
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
            strict_errors: bool,
            terminate: bool,
        }

        impl$(<$lifetime>)? $struct$(<$lifetime>)? {
            fn new(process: $process_type, time_limit: Duration) -> Self {
                Self {
                    handle: imp::Handle::inherited(&process),
                    process,
                    time_limit,
                    strict_errors: false,
                    terminate: false,
                }
            }

            /// Causes [`wait`] to never suppress an error.
            ///
            /// Typically, errors terminating the process will be ignored, as
            /// they are often less important than the result. However, when
            /// this method is called, these errors will be returned as well.
            ///
            /// [`wait`]: #method.wait
            #[inline]
            #[must_use]
            pub fn strict_errors(mut self) -> Self {
                self.strict_errors = true;
                self
            }

            /// Causes the process to be terminated if it exceeds the time
            /// limit.
            ///
            /// Process identifier reuse by the system will be mitigated. There
            /// should never be a scenario that causes an unintended process to
            /// be terminated.
            #[inline]
            #[must_use]
            pub fn terminating(mut self) -> Self {
                self.terminate = true;
                self
            }

            fn run_wait(&mut self) -> IoResult<Option<ExitStatus>> {
                // Check if the exit status was already captured.
                let result = self.process.try_wait();
                if let Ok(Some(exit_status)) = result {
                    return Ok(Some(exit_status.into()));
                }

                let _ = self.process.stdin.take();
                let mut result = self
                    .handle
                    .wait_with_timeout(self.time_limit)
                    .map(|x| x.map(ExitStatus));

                macro_rules! try_run {
                    ( $result:expr ) => {
                        let next_result = $result;
                        if self.strict_errors && result.is_ok() {
                            if let Err(error) = next_result {
                                result = Err(error);
                            }
                        }
                    };
                }

                if self.terminate {
                    // If the process exited normally, identifier reuse might
                    // cause a different process to be terminated.
                    if let Ok(Some(_)) = result {
                    } else {
                        try_run!(self.process.kill().and(self.process.wait()));
                    }
                }
                try_run!(self.process.try_wait());

                result
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
            /// This method cannot guarantee that the same [`ErrorKind`]
            /// variants will be returned in the future for the same type of
            /// failure. Allowing these breakages is required to be compatible
            /// with the [`Error`] type.
            ///
            /// [`Error`]: https://doc.rust-lang.org/std/io/struct.Error.html
            /// [`ErrorKind`]: https://doc.rust-lang.org/std/io/enum.ErrorKind.html
            /// [`terminating`]: #method.terminating
            #[inline]
            pub fn wait(mut self) -> IoResult<Option<$return_type>> {
                self.run_wait()?
                    .map(|x| $create_result_fn(&mut self, x))
                    .transpose()
            }
        }
    };
}

r#impl!(
    _ExitStatusTimeout<'a>,
    &'a mut Child,
    ExitStatus,
    |_, status| Ok(status),
);

r#impl!(
    _OutputTimeout,
    Child,
    Output,
    |timeout: &mut Self, status| {
        let mut output = Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        if let Some(stdout) = &mut timeout.process.stdout {
            let _ = stdout.read_to_end(&mut output.stdout)?;
        }
        if let Some(stderr) = &mut timeout.process.stderr {
            let _ = stderr.read_to_end(&mut output.stderr)?;
        }
        Ok(output)
    },
);

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
    /// let process_terminator = process.terminator()?;
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`ProcessTerminator`]: struct.ProcessTerminator.html
    fn terminator(&self) -> IoResult<ProcessTerminator>;

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
    fn terminator(&self) -> IoResult<ProcessTerminator> {
        imp::Handle::new(self).map(ProcessTerminator)
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
