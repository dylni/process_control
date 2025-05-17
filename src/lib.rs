//! This crate allows running a process with resource limits, such as a running
//! time, and the option to terminate it automatically afterward. The latter is
//! surprisingly difficult to achieve on Unix, since process identifiers can be
//! arbitrarily reassigned when no longer used. Thus, it would be extremely
//! easy to inadvertently terminate an unexpected process. This crate protects
//! against that possibility.
//!
//! Methods for setting limits are available on [`ChildExt`], which is
//! implemented for [`Child`]. They each return a builder of options to
//! configure how the limit should be applied.
//!
//! <div class="warning">
//!
//! This crate should not be used for security. There are many ways that a
//! process can bypass resource limits. The limits are only intended for simple
//! restriction of harmless processes.
//!
//! </div>
//!
//! # Features
//!
//! These features are optional and can be enabled or disabled in a
//! "Cargo.toml" file.
//!
//! ### Optional Features
//!
//! - **parking\_lot** -
//!   Changes the implementation to use crate [parking\_lot] on targets missing
//!   some syscalls. This feature will reduce the likelihood of resource
//!   starvation for those targets.
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
//!   of [`ChildExt::controlled_with_output`] to read output while setting a
//!   timeout. This crate aims to fill in those gaps and simplify the
//!   implementation, now that [`Receiver::recv_timeout`] exists.
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
//! use process_control::Control;
//!
//! let message = "hello world";
//! let process = Command::new("echo")
//!     .arg(message)
//!     .stdout(Stdio::piped())
//!     .spawn()?;
//!
//! let output = process
//!     .controlled_with_output()
//!     .time_limit(Duration::from_secs(1))
//!     .terminate_for_timeout()
//!     .wait()?
//!     .ok_or_else(|| {
//!         io::Error::new(io::ErrorKind::TimedOut, "Process timed out")
//!     })?;
//! assert!(output.status.success());
//! assert_eq!(message.as_bytes(), &output.stdout[..message.len()]);
//! #
//! # Ok::<_, io::Error>(())
//! ```
//!
//! [parking\_lot]: https://crates.io/crates/parking_lot
//! [`Receiver::recv_timeout`]: ::std::sync::mpsc::Receiver::recv_timeout
//! [sealed]: https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed
//! [wait-timeout]: https://crates.io/crates/wait-timeout

// Only require a nightly compiler when building documentation for docs.rs.
// This is a private option that should not be used.
// https://github.com/rust-lang/docs.rs/issues/147#issuecomment-389544407
#![cfg_attr(process_control_docs_rs, feature(doc_cfg))]
#![warn(unused_results)]

use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
#[cfg(any(doc, unix))]
use std::os::raw::c_int;
use std::process;
use std::process::Child;
use std::str;
use std::time::Duration;

mod control;

#[cfg_attr(unix, path = "unix/mod.rs")]
#[cfg_attr(windows, path = "windows/mod.rs")]
mod imp;

type WaitResult<T> = io::Result<Option<T>>;

#[rustfmt::skip]
macro_rules! unix_method {
    ( $method:ident , $return_type:ty ) => {
        #[doc = concat!(
            "Equivalent to [`ExitStatusExt::",
            stringify!($method),
            "`][method].

[method]: ::std::os::unix::process::ExitStatusExt::",
            stringify!($method),
        )]
        #[cfg(any(doc, unix))]
        #[cfg_attr(process_control_docs_rs, doc(cfg(unix)))]
        #[inline]
        #[must_use]
        pub fn $method(&self) -> $return_type {
            self.inner.$method()
        }
    };
}

/// Equivalent to [`process::ExitStatus`] but allows for greater accuracy.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct ExitStatus {
    inner: imp::ExitStatus,
    std: process::ExitStatus,
}

impl ExitStatus {
    fn new(inner: imp::ExitStatus, std: process::ExitStatus) -> Self {
        debug_assert_eq!(inner, std.into());
        Self { inner, std }
    }

    /// Equivalent to [`process::ExitStatus::success`].
    #[inline]
    #[must_use]
    pub fn success(self) -> bool {
        self.inner.success()
    }

    /// Equivalent to [`process::ExitStatus::code`], but a more accurate value
    /// will be returned if possible.
    #[inline]
    #[must_use]
    pub fn code(self) -> Option<i64> {
        self.inner.code().map(Into::into)
    }

    unix_method!(continued, bool);
    unix_method!(core_dumped, bool);
    unix_method!(signal, Option<c_int>);
    unix_method!(stopped_signal, Option<c_int>);

    /// Converts this structure to a corresponding [`process::ExitStatus`]
    /// instance.
    ///
    /// Since this type can represent more exit codes, it will attempt to
    /// provide an equivalent representation using the standard library type.
    /// However, if converted back to this structure, detailed information may
    /// have been lost.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    /// use process_control::Control;
    ///
    /// let exit_status = Command::new("echo")
    ///     .spawn()?
    ///     .controlled()
    ///     .time_limit(Duration::from_secs(1))
    ///     .terminate_for_timeout()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(exit_status.success());
    /// assert!(exit_status.into_std_lossy().success());
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn into_std_lossy(self) -> process::ExitStatus {
        self.std
    }
}

impl AsMut<Self> for ExitStatus {
    #[inline]
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl AsRef<Self> for ExitStatus {
    #[inline]
    fn as_ref(&self) -> &Self {
        self
    }
}

impl Display for ExitStatus {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

impl From<process::ExitStatus> for ExitStatus {
    #[inline]
    fn from(value: process::ExitStatus) -> Self {
        Self::new(value.into(), value)
    }
}

/// Equivalent to [`process::Output`] but holds an instance of [`ExitStatus`]
/// from this crate.
#[derive(Clone, Eq, PartialEq)]
#[must_use]
pub struct Output {
    /// Equivalent to [`process::Output::status`].
    pub status: ExitStatus,

    /// Equivalent to [`process::Output::stdout`].
    pub stdout: Vec<u8>,

    /// Equivalent to [`process::Output::stderr`].
    pub stderr: Vec<u8>,
}

impl Output {
    /// Converts this structure to a corresponding [`process::Output`]
    /// instance.
    ///
    /// For more information, see [`ExitStatus::into_std_lossy`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io;
    /// use std::process::Command;
    /// use std::process::Stdio;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    /// use process_control::Control;
    ///
    /// let message = "foobar";
    /// let output = Command::new("echo")
    ///     .arg(message)
    ///     .stdout(Stdio::piped())
    ///     .spawn()?
    ///     .controlled_with_output()
    ///     .time_limit(Duration::from_secs(1))
    ///     .terminate_for_timeout()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(output.status.success());
    /// assert_eq!(message.as_bytes(), &output.stdout[..message.len()]);
    ///
    /// let output = output.into_std_lossy();
    /// assert!(output.status.success());
    /// assert_eq!(message.as_bytes(), &output.stdout[..message.len()]);
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn into_std_lossy(self) -> process::Output {
        process::Output {
            status: self.status.into_std_lossy(),
            stdout: self.stdout,
            stderr: self.stderr,
        }
    }
}

impl AsMut<ExitStatus> for Output {
    #[inline]
    fn as_mut(&mut self) -> &mut ExitStatus {
        &mut self.status
    }
}

impl AsRef<ExitStatus> for Output {
    #[inline]
    fn as_ref(&self) -> &ExitStatus {
        &self.status
    }
}

struct DebugBuffer<'a>(&'a [u8]);

impl Debug for DebugBuffer<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;

        let mut string = self.0;
        while !string.is_empty() {
            let mut invalid = &b""[..];
            let valid = str::from_utf8(string).unwrap_or_else(|error| {
                let (valid, string) = string.split_at(error.valid_up_to());

                let invalid_length =
                    error.error_len().unwrap_or_else(|| string.len());
                invalid = &string[..invalid_length];

                // SAFETY: This slice was validated to be UTF-8.
                unsafe { str::from_utf8_unchecked(valid) }
            });

            Display::fmt(&valid.escape_debug(), f)?;
            string = &string[valid.len()..];

            for byte in invalid {
                write!(f, "\\x{:02X}", byte)?;
            }
            string = &string[invalid.len()..];
        }

        f.write_str("\"")
    }
}

impl Debug for Output {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output")
            .field("status", &self.status)
            .field("stdout", &DebugBuffer(&self.stdout))
            .field("stderr", &DebugBuffer(&self.stderr))
            .finish()
    }
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

impl From<Output> for ExitStatus {
    #[inline]
    fn from(value: Output) -> Self {
        value.status
    }
}

/// A function to be called for reads from a specific process pipe ([stdout] or
/// [stderr]).
///
/// When additional bytes are read from the pipe, they are passed to this
/// function, which determines whether to include them in [`Output`]. The
/// number of bytes is not guaranteed to be consistent and may not match the
/// number written at any time by the command on the other side of the stream.
///
/// If this function returns `Ok(false)`, the passed output will be discarded
/// and not included in [`Output`]. Errors will be propagated to
/// [`Control::wait`]. For more complex cases, where specific portions of read
/// bytes should be included, this function can return `false` and maintain the
/// output buffer itself.
///
/// # Examples
///
/// ```
/// use std::io;
/// use std::io::Write;
/// use std::process::Command;
/// use std::process::Stdio;
///
/// use process_control::ChildExt;
/// use process_control::Control;
///
/// let message = "foobar";
/// let output = Command::new("echo")
///     .arg(message)
///     .stdout(Stdio::piped())
///     .spawn()?
///     .controlled_with_output()
///     // Stream output while collecting it.
///     .stdout_filter(|x| io::stdout().write_all(x).map(|()| true))
///     .wait()?
///     .expect("process timed out");
/// assert!(output.status.success());
/// assert_eq!(message.as_bytes(), &output.stdout[..message.len()]);
/// #
/// # Ok::<_, io::Error>(())
/// ```
///
/// [stderr]: Control::stderr_filter
/// [stdout]: Control::stdout_filter
pub trait PipeFilter:
    'static + FnMut(&[u8]) -> io::Result<bool> + Send
{
}

impl<T> PipeFilter for T where
    T: 'static + FnMut(&[u8]) -> io::Result<bool> + Send
{
}

/// A temporary wrapper for process limits.
#[attr_alias::eval]
#[must_use]
pub trait Control: private::Sealed {
    /// The type returned by [`wait`].
    ///
    /// [`wait`]: Self::wait
    type Result;

    /// Sets the total virtual memory limit for the process in bytes.
    ///
    /// If the process attempts to allocate memory in excess of this limit, the
    /// allocation will fail. The type of failure will depend on the platform,
    /// and the process might terminate if it cannot handle it.
    ///
    /// Small memory limits are safe, but they might prevent the operating
    /// system from starting the process.
    #[attr_alias(memory_limit, cfg(any(doc, *)))]
    #[attr_alias(memory_limit, cfg_attr(process_control_docs_rs, doc(cfg(*))))]
    #[must_use]
    fn memory_limit(self, limit: usize) -> Self;

    /// Sets the total time limit for the process in milliseconds.
    ///
    /// A process that exceeds this limit will not be terminated unless
    /// [`terminate_for_timeout`] is called.
    ///
    /// [`terminate_for_timeout`]: Self::terminate_for_timeout
    #[must_use]
    fn time_limit(self, limit: Duration) -> Self;

    /// Causes [`wait`] to never suppress an error.
    ///
    /// Typically, errors terminating the process will be ignored, as they are
    /// often less important than the result. However, when this method is
    /// called, those errors will be returned as well.
    ///
    /// [`wait`]: Self::wait
    #[must_use]
    fn strict_errors(self) -> Self;

    /// Causes the process to be terminated if it exceeds the time limit.
    ///
    /// Process identifier reuse by the system will be mitigated. There should
    /// never be a scenario that causes an unintended process to be terminated.
    #[must_use]
    fn terminate_for_timeout(self) -> Self;

    /// Calls a filter function for each write to [stdout].
    ///
    /// For more information, see [`PipeFilter`].
    ///
    /// # Panics
    ///
    /// Panics if [`Command::stdout`] has not been set to [`Stdio::piped`].
    ///
    /// [`Command::stdout`]: ::std::process::Command::stdout
    /// [`Stdio::piped`]: ::std::process::Stdio::piped
    /// [stdout]: Output::stdout
    #[must_use]
    fn stdout_filter<T>(self, listener: T) -> Self
    where
        Self: Control<Result = Output>,
        T: PipeFilter;

    /// Calls a filter function for each write to [stderr].
    ///
    /// For more information, see [`PipeFilter`].
    ///
    /// # Panics
    ///
    /// Panics if [`Command::stderr`] has not been set to [`Stdio::piped`].
    ///
    /// [`Command::stderr`]: ::std::process::Command::stderr
    /// [`Stdio::piped`]: ::std::process::Stdio::piped
    /// [stderr]: Output::stderr
    #[must_use]
    fn stderr_filter<T>(self, listener: T) -> Self
    where
        Self: Control<Result = Output>,
        T: PipeFilter;

    /// Runs the process to completion, aborting if it exceeds the time limit.
    ///
    /// At least one additional thread might be created to wait on the process
    /// without blocking the current thread.
    ///
    /// If the time limit is exceeded before the process finishes, `Ok(None)`
    /// will be returned. However, the process will not be terminated in that
    /// case unless [`terminate_for_timeout`] is called beforehand. It is
    /// recommended to always call that method to allow system resources to be
    /// freed.
    ///
    /// The stdin handle to the process, if it exists, will be closed before
    /// waiting. Otherwise, the process would assuredly time out when reading
    /// from that pipe.
    ///
    /// This method cannot guarantee that the same [`io::ErrorKind`] variants
    /// will be returned in the future for the same types of failures. Allowing
    /// these breakages is required to enable calling [`Child::kill`]
    /// internally.
    ///
    /// [`terminate_for_timeout`]: Self::terminate_for_timeout
    fn wait(self) -> WaitResult<Self::Result>;
}

/// Extensions to [`Child`] for easily terminating processes.
///
/// For more information, see [the module-level documentation][module].
///
/// [module]: self
pub trait ChildExt: private::Sealed {
    /// Creates an instance of [`Control`] that yields [`ExitStatus`] for this
    /// process.
    ///
    /// This method parallels [`Child::wait`] but allows setting limits on the
    /// process.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    /// use process_control::Control;
    ///
    /// let exit_status = Command::new("echo")
    ///     .spawn()?
    ///     .controlled()
    ///     .time_limit(Duration::from_secs(1))
    ///     .terminate_for_timeout()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(exit_status.success());
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    #[must_use]
    fn controlled(&mut self) -> impl Control<Result = ExitStatus> + Debug;

    /// Creates an instance of [`Control`] that yields [`Output`] for this
    /// process.
    ///
    /// This method parallels [`Child::wait_with_output`] but allows setting
    /// limits on the process.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io;
    /// use std::process::Command;
    /// use std::process::Stdio;
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    /// use process_control::Control;
    ///
    /// let message = "foobar";
    /// let output = Command::new("echo")
    ///     .arg(message)
    ///     .stdout(Stdio::piped())
    ///     .spawn()?
    ///     .controlled_with_output()
    ///     .time_limit(Duration::from_secs(1))
    ///     .terminate_for_timeout()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(output.status.success());
    /// assert_eq!(message.as_bytes(), &output.stdout[..message.len()]);
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    #[must_use]
    fn controlled_with_output(self) -> impl Control<Result = Output> + Debug;
}

impl ChildExt for Child {
    #[inline]
    fn controlled(&mut self) -> impl Control<Result = ExitStatus> + Debug {
        control::Buffer::new(self)
    }

    #[inline]
    fn controlled_with_output(self) -> impl Control<Result = Output> + Debug {
        control::Buffer::new(self)
    }
}

mod private {
    use std::process::Child;

    use super::control;

    pub trait Sealed {}
    impl Sealed for Child {}
    impl<P> Sealed for control::Buffer<P> where P: control::Process {}
}
