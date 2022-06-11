//! This crate allows running a process with resource limits, such as a time,
//! and the option to terminate it automatically afterward. The latter is
//! surprisingly difficult to achieve on Unix, since process identifiers can be
//! arbitrarily reassigned when no longer used. Thus, it would be extremely
//! easy to inadvertently terminate an unexpected process. This crate protects
//! against that possibility.
//!
//! Methods for setting limits are available on [`ChildExt`], which is
//! implemented for [`Child`]. They each return a builder of options to
//! configure how the limit should be applied.
//!
//! <div style="background:rgba(255,181,77,0.16); padding:0.75em;">
//! <strong>Warning</strong>: This crate should not be used for security. There
//! are many ways that a process can bypass resource limits. The limits are
//! only intended for simple restriction of harmless processes.
//! </div>
//!
//! # Features
//!
//! These features are optional and can be enabled or disabled in a
//! "Cargo.toml" file.
//!
//! ### Optional Features
//!
//! - **crossbeam-channel** -
//!   Changes the implementation to use crate [crossbeam-channel] for better
//!   performance.
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
//! let process = Command::new("echo")
//!     .arg("hello")
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
//! assert_eq!(b"hello", &output.stdout[..5]);
//! #
//! # Ok::<_, io::Error>(())
//! ```
//!
//! [crossbeam-channel]: https://crates.io/crates/crossbeam-channel
//! [`Receiver::recv_timeout`]: ::std::sync::mpsc::Receiver::recv_timeout
//! [sealed]: https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed
//! [wait-timeout]: https://crates.io/crates/wait-timeout

#![allow(deprecated)]
// Only require a nightly compiler when building documentation for docs.rs.
// This is a private option that should not be used.
// https://github.com/rust-lang/docs.rs/issues/147#issuecomment-389544407
#![cfg_attr(process_control_docs_rs, feature(doc_cfg))]
#![warn(unsafe_op_in_unsafe_fn)]
#![warn(unused_results)]

use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
use std::process;
use std::process::Child;
use std::time::Duration;

macro_rules! if_memory_limit {
    ( $($item:item)+ ) => {
        $(
            #[cfg(any(
                target_os = "android",
                all(
                    target_os = "linux",
                    any(target_env = "gnu", target_env = "musl"),
                ),
                windows,
            ))]
            $item
        )+
    };
}

mod control;

#[cfg_attr(unix, path = "unix/mod.rs")]
#[cfg_attr(windows, path = "windows/mod.rs")]
mod imp;

#[cfg(all(
    feature = "signal-hook",
    not(feature = "__unstable-force-missing-waitid"),
))]
const _: &str = env! {
    "__UNSTABLE_PROCESS_CONTROL_ALLOW_SIGNAL_HOOK_FEATURE",
    "The 'signal-hook' feature is private and will be removed.",
};

type WaitResult<T> = io::Result<Option<T>>;

/// A wrapper that stores enough information to terminate a process.
///
/// Instances can only be constructed using [`ChildExt::terminator`].
#[deprecated = "cannot be used safely and should be unnecessary"]
#[derive(Debug)]
pub struct Terminator(imp::DuplicatedHandle);

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
    #[deprecated = "cannot be used safely and should be unnecessary"]
    #[inline]
    pub unsafe fn terminate(&self) -> io::Result<()> {
        // SAFETY: The safety requirements are documented.
        unsafe { self.0.terminate() }
    }
}

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
        #[cfg(any(unix, doc))]
        #[cfg_attr(process_control_docs_rs, doc(cfg(unix)))]
        #[inline]
        #[must_use]
        pub fn $method(&self) -> $return_type {
            self.0.$method()
        }
    };
}

/// Equivalent to [`process::ExitStatus`] but allows for greater accuracy.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ExitStatus(imp::ExitStatus);

impl ExitStatus {
    /// Equivalent to [`process::ExitStatus::success`].
    #[inline]
    #[must_use]
    pub fn success(self) -> bool {
        self.0.success()
    }

    /// Equivalent to [`process::ExitStatus::code`], but a more accurate value
    /// will be returned if possible.
    #[inline]
    #[must_use]
    pub fn code(self) -> Option<i64> {
        self.0.code().map(Into::into)
    }

    unix_method!(continued, bool);
    unix_method!(core_dumped, bool);
    unix_method!(signal, Option<::std::os::raw::c_int>);
    unix_method!(stopped_signal, Option<::std::os::raw::c_int>);
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
        Display::fmt(&self.0, f)
    }
}

impl From<process::ExitStatus> for ExitStatus {
    #[inline]
    fn from(value: process::ExitStatus) -> Self {
        #[cfg_attr(windows, allow(clippy::useless_conversion))]
        Self(value.into())
    }
}

/// Equivalent to [`process::Output`] but holds an instance of [`ExitStatus`]
/// from this crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Output {
    /// Equivalent to [`process::Output::status`].
    pub status: ExitStatus,

    /// Equivalent to [`process::Output::stdout`].
    pub stdout: Vec<u8>,

    /// Equivalent to [`process::Output::stderr`].
    pub stderr: Vec<u8>,
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

/// A temporary wrapper for process limits.
#[must_use]
pub trait Control: private::Sealed {
    /// The type returned by [`wait`].
    ///
    /// [`wait`]: Self::wait
    type Result;

    if_memory_limit! {
        /// Sets the total virtual memory limit for the process in bytes.
        ///
        /// If the process attempts to allocate memory in excess of this limit,
        /// the allocation will fail. The type of failure will depend on the
        /// platform, and the process might terminate if it cannot handle it.
        ///
        /// Small memory limits are safe, but they might prevent the operating
        /// system from starting the process.
        #[cfg_attr(
            process_control_docs_rs,
            doc(cfg(any(
                target_os = "android",
                all(
                    target_os = "linux",
                    any(target_env = "gnu", target_env = "musl"),
                ),
                windows,
            )))
        )]
        #[must_use]
        fn memory_limit(self, limit: usize) -> Self;
    }

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

/// A temporary wrapper for a process timeout.
#[deprecated = "use `Control` instead"]
pub trait Timeout: private::Sealed {
    /// The type returned by [`wait`].
    ///
    /// [`wait`]: Self::wait
    #[deprecated = "use `Control::Result` instead"]
    type Result;

    /// Causes [`wait`] to never suppress an error.
    ///
    /// Typically, errors terminating the process will be ignored, as they are
    /// often less important than the result. However, when this method is
    /// called, those errors will be returned as well.
    ///
    /// [`wait`]: Self::wait
    #[must_use]
    #[deprecated = "use `Control::strict_errors` instead"]
    fn strict_errors(self) -> Self;

    /// Causes the process to be terminated if it exceeds the time limit.
    ///
    /// Process identifier reuse by the system will be mitigated. There should
    /// never be a scenario that causes an unintended process to be terminated.
    #[must_use]
    #[deprecated = "use `Control::terminate_for_timeout` instead"]
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
    /// This method cannot guarantee that the same [`io::ErrorKind`] variants
    /// will be returned in the future for the same types of failures. Allowing
    /// these breakages is required to enable calling [`Child::kill`]
    /// internally.
    ///
    /// [`terminating`]: Self::terminating
    #[deprecated = "use `Control::wait` instead"]
    fn wait(self) -> WaitResult<Self::Result>;
}

/// Extensions to [`Child`] for easily terminating processes.
///
/// For more information, see [the module-level documentation][crate].
pub trait ChildExt<'a>: private::Sealed {
    /// The type returned by [`controlled`].
    ///
    /// [`controlled`]: Self::controlled
    type ExitStatusControl: 'a + Control<Result = ExitStatus> + Debug;

    /// The type returned by [`controlled_with_output`].
    ///
    /// [`controlled_with_output`]: Self::controlled_with_output
    type OutputControl: Control<Result = Output> + Debug;

    /// The type returned by [`with_timeout`].
    ///
    /// [`with_timeout`]: Self::with_timeout
    #[deprecated = "use `ExitStatusControl` instead"]
    type ExitStatusTimeout: 'a + Timeout<Result = ExitStatus>;

    /// The type returned by [`with_output_timeout`].
    ///
    /// [`with_output_timeout`]: Self::with_output_timeout
    #[deprecated = "use `OutputControl` instead"]
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
    /// let terminator = process.terminator()?;
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    #[deprecated = "cannot be used safely and should be unnecessary"]
    fn terminator(&self) -> io::Result<Terminator>;

    /// Equivalent to [`Child::kill`] but ignores errors when the process is no
    /// longer running.
    ///
    /// Windows and Unix errors are inconsistent when terminating processes.
    /// This method unifies them by simulating Unix behavior on Windows.
    #[allow(clippy::missing_errors_doc)]
    fn terminate_if_running(&mut self) -> io::Result<()>;

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
    fn controlled(&'a mut self) -> Self::ExitStatusControl;

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
    /// use std::time::Duration;
    ///
    /// use process_control::ChildExt;
    /// use process_control::Control;
    ///
    /// let output = Command::new("echo")
    ///     .spawn()?
    ///     .controlled_with_output()
    ///     .time_limit(Duration::from_secs(1))
    ///     .terminate_for_timeout()
    ///     .wait()?
    ///     .expect("process timed out");
    /// assert!(output.status.success());
    /// #
    /// # Ok::<_, io::Error>(())
    /// ```
    #[must_use]
    fn controlled_with_output(self) -> Self::OutputControl;

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
    #[deprecated = "use `controlled` and `Control::time_limit` instead"]
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
    #[deprecated = "use `controlled_with_output` and `Control::time_limit` \
                    instead"]
    #[must_use]
    fn with_output_timeout(self, time_limit: Duration) -> Self::OutputTimeout;
}

impl<'a> ChildExt<'a> for Child {
    type ExitStatusControl = control::ExitStatusControl<'a>;

    type OutputControl = control::OutputControl;

    type ExitStatusTimeout = control::ExitStatusTimeout<'a>;

    type OutputTimeout = control::OutputTimeout;

    #[inline]
    fn terminator(&self) -> io::Result<Terminator> {
        imp::DuplicatedHandle::new(self).map(Terminator)
    }

    #[inline]
    fn terminate_if_running(&mut self) -> io::Result<()> {
        imp::terminate_if_running(self)
    }

    #[inline]
    fn controlled(&'a mut self) -> Self::ExitStatusControl {
        Self::ExitStatusControl::new(self)
    }

    #[inline]
    fn controlled_with_output(self) -> Self::OutputControl {
        Self::OutputControl::new(self)
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

    use super::control;

    pub trait Sealed {}
    impl Sealed for Child {}
    impl Sealed for control::ExitStatusControl<'_> {}
    impl Sealed for control::ExitStatusTimeout<'_> {}
    impl Sealed for control::OutputControl {}
    impl Sealed for control::OutputTimeout {}
}
