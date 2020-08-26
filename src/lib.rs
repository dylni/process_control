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
//! # Comparable Crates
//!
//! - [wait-timeout] -
//!   Made for a related purpose but does not provide the same functionality.
//!   Processes cannot be terminated automatically, and there is no counterpart
//!   of [`Child::wait_with_output`] to read output while setting a timeout.
//!   This crate aims to fill in those gaps and simplify the implementation,
//!   now that [`Receiver::recv_timeout`] exists.
//!
//! # Examples
//!
//! ```
//! use std::io;
//! use std::process::Command;
//! use std::process::Stdio;
//! use std::time::Duration;
//!
//! use process_control::ChildExt;
//! use process_control::Timeout;
//!
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
//!         io::Error::new(io::ErrorKind::TimedOut, "Process timed out")
//!     })?;
//! assert_eq!(b"hello", &output.stdout[..5]);
//! #
//! # Ok::<_, io::Error>(())
//! ```
//!
//! [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
//! [`ChildExt`]: trait.ChildExt.html
//! [`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
//! [`Receiver::recv_timeout`]: https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html#method.recv_timeout
//! [sealed]: https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed
//! [wait-timeout]: https://crates.io/crates/wait-timeout

#![doc(html_root_url = "https://docs.rs/process_control/*")]
// Only require a nightly compiler when building documentation for docs.rs.
// This is a private option that should not be used.
// https://github.com/rust-lang/docs.rs/issues/147#issuecomment-389544407
#![cfg_attr(process_control_docs_rs, feature(doc_cfg))]
#![warn(unused_results)]

use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
use std::process;
use std::process::Child;
use std::time::Duration;

#[cfg(unix)]
#[path = "unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

mod timeout;

/// A wrapper that stores enough information to terminate a process.
///
/// Instances can only be constructed using [`ChildExt::terminator`].
///
/// [`ChildExt::terminator`]: trait.ChildExt.html#tymethod.terminator
#[derive(Debug)]
pub struct Terminator(imp::Handle);

impl Terminator {
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
    /// # use std::io;
    /// use std::path::Path;
    /// use std::process::Command;
    /// use std::thread;
    ///
    /// use process_control::ChildExt;
    ///
    /// let dir = Path::new("hello");
    /// let mut process = Command::new("mkdir").arg(dir).spawn()?;
    /// let terminator = process.terminator()?;
    ///
    /// let thread = thread::spawn(move || process.wait());
    /// if !dir.exists() {
    ///     // [process.kill] requires a mutable reference.
    ///     unsafe { terminator.terminate()? }
    /// }
    ///
    /// let exit_status = thread.join().expect("thread panicked")?;
    /// println!("exited {}", exit_status);
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    ///
    /// [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
    /// [`Child::kill`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.kill
    #[inline]
    pub unsafe fn terminate(&self) -> io::Result<()> {
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

    /// Equivalent to [`ExitStatus::code`], but a more accurate value will be
    /// returned if possible.
    ///
    /// [`ExitStatus::code`]: https://doc.rust-lang.org/std/process/struct.ExitStatus.html#method.code
    #[inline]
    #[must_use]
    pub fn code(self) -> Option<i64> {
        self.0.code().map(Into::into)
    }

    /// Equivalent to [`ExitStatusExt::signal`].
    ///
    /// [`ExitStatusExt::signal`]: https://doc.rust-lang.org/std/os/unix/process/trait.ExitStatusExt.html#tymethod.signal
    #[cfg(any(unix, doc))]
    #[cfg_attr(process_control_docs_rs, doc(cfg(unix)))]
    #[inline]
    #[must_use]
    pub fn signal(self) -> Option<::std::os::raw::c_int> {
        self.0.signal()
    }
}

impl Display for ExitStatus {
    #[inline]
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<process::ExitStatus> for ExitStatus {
    #[inline]
    fn from(value: process::ExitStatus) -> Self {
        #[cfg_attr(windows, allow(clippy::useless_conversion))]
        Self(value.into())
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

impl From<process::Output> for Output {
    #[inline]
    fn from(value: process::Output) -> Self {
        Self {
            status: value.status.into(),
            stdout: value.stdout,
            stderr: value.stderr,
        }
    }
}

/// A temporary wrapper for a process timeout.
pub trait Timeout: private::Sealed {
    /// The type returned by [`wait`].
    ///
    /// [`wait`]: #tymethod.wait
    type Result;

    /// Causes [`wait`] to never suppress an error.
    ///
    /// Typically, errors terminating the process will be ignored, as they are
    /// often less important than the result. However, when this method is
    /// called, those errors will be returned as well.
    ///
    /// [`wait`]: #tymethod.wait
    #[must_use]
    fn strict_errors(self) -> Self;

    /// Causes the process to be terminated if it exceeds the time limit.
    ///
    /// Process identifier reuse by the system will be mitigated. There should
    /// never be a scenario that causes an unintended process to be terminated.
    #[must_use]
    fn terminating(self) -> Self;

    /// Runs the process to completion, aborting if it exceeds the time limit.
    ///
    /// At least one thread will be created to wait on the process without
    /// blocking the current thread.
    ///
    /// If the time limit is exceeded before the process finishes, `Ok(None)`
    /// will be returned. However, the process will not be terminated in that
    /// case unless [`terminating`] is called beforehand. It is recommended to
    /// always call that method to allow system resources to be freed.
    ///
    /// The stdin handle to the process, if it exists, will be closed before
    /// waiting. Otherwise, the process would assuredly time out when reading
    /// from that pipe.
    ///
    /// This method cannot guarantee that the same [`ErrorKind`] variants will
    /// be returned in the future for the same types of failures. Allowing
    /// these breakages is required to be compatible with the [`Error`] type.
    ///
    /// [`Error`]: https://doc.rust-lang.org/std/io/struct.Error.html
    /// [`ErrorKind`]: https://doc.rust-lang.org/std/io/enum.ErrorKind.html
    /// [`terminating`]: #tymethod.terminating
    fn wait(self) -> io::Result<Option<Self::Result>>;
}

/// Extensions to [`Child`] for easily terminating processes.
///
/// For more information, see [the module-level documentation][module].
///
/// [module]: index.html
/// [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
pub trait ChildExt<'a>: private::Sealed {
    /// The type returned by [`with_timeout`].
    ///
    /// [`with_timeout`]: #tymethod.with_timeout
    type ExitStatusTimeout: 'a + Timeout<Result = ExitStatus>;

    /// The type returned by [`with_output_timeout`].
    ///
    /// [`with_output_timeout`]: #tymethod.with_output_timeout
    type OutputTimeout: Timeout<Result = Output>;

    /// Creates an instance of [`Terminator`] for this process.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io;
    /// use std::process::Command;
    ///
    /// use process_control::ChildExt;
    ///
    /// let process = Command::new("echo").spawn()?;
    /// # #[allow(unused_variables)]
    /// let terminator = process.terminator()?;
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    ///
    /// [`Terminator`]: struct.Terminator.html
    fn terminator(&self) -> io::Result<Terminator>;

    /// Creates an instance of [`Timeout`] that yields [`ExitStatus`] for this
    /// process.
    ///
    /// This method parallels [`Child::wait`] when the process must finish
    /// within a time limit.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    /// use process_control::Timeout;
    ///
    /// let exit_status = Command::new("echo")
    ///     .spawn()?
    ///     .with_timeout(Duration::from_secs(1))
    ///     .terminating()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(exit_status.success());
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    ///
    /// [`Child::wait`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait
    /// [`ExitStatus`]: struct.ExitStatus.html
    /// [`Timeout`]: trait.Timeout.html
    #[must_use]
    fn with_timeout(
        &'a mut self,
        time_limit: Duration,
    ) -> Self::ExitStatusTimeout;

    /// Creates an instance of [`Timeout`] that yields [`Output`] for this
    /// process.
    ///
    /// This method parallels [`Child::wait_with_output`] when the process must
    /// finish within a time limit.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    /// use process_control::Timeout;
    ///
    /// let output = Command::new("echo")
    ///     .spawn()?
    ///     .with_output_timeout(Duration::from_secs(1))
    ///     .terminating()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(output.status.success());
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    ///
    /// [`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
    /// [`Output`]: struct.Output.html
    /// [`Timeout`]: trait.Timeout.html
    #[must_use]
    fn with_output_timeout(self, time_limit: Duration) -> Self::OutputTimeout;
}

impl<'a> ChildExt<'a> for Child {
    type ExitStatusTimeout = timeout::ExitStatusTimeout<'a>;
    type OutputTimeout = timeout::OutputTimeout;

    #[inline]
    fn terminator(&self) -> io::Result<Terminator> {
        imp::Handle::new(self).map(Terminator)
    }

    #[inline]
    fn with_timeout(
        &'a mut self,
        time_limit: Duration,
    ) -> Self::ExitStatusTimeout {
        Self::ExitStatusTimeout::new(self, time_limit)
    }

    #[inline]
    fn with_output_timeout(self, time_limit: Duration) -> Self::OutputTimeout {
        Self::OutputTimeout::new(self, time_limit)
    }
}

mod private {
    use std::process::Child;

    use super::timeout;

    pub trait Sealed {}
    impl Sealed for Child {}
    impl Sealed for timeout::ExitStatusTimeout<'_> {}
    impl Sealed for timeout::OutputTimeout {}
}
