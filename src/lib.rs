//! This crate allows terminating a process without a mutable reference.
//! [`ProcessTerminator::terminate`] is designed to operate in this manner and
//! is the reason this crate exists. It intentionally does not require a
//! reference of any kind to the [`Child`] instance, allowing for maximal
//! flexibility in working with processes.
//!
//! Typically, it is not possible to terminate a process during a call to
//! [`Child::wait`] or [`Child::wait_with_output`] in another thread, since
//! [`Child::kill`] takes a mutable reference. However, since this crate
//! creates its own termination method, there is no issue, allowing cleanup
//! after calling methods such as [`Terminator::wait_for_output_with_timeout`].
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
//! use process_control::Terminator;
//!
//! # fn main() -> IoResult<()> {
//! let process = Command::new("echo")
//!     .arg("hello")
//!     .stdout(Stdio::piped())
//!     .spawn()?;
//!
//! let output = process
//!     .wait_for_output_with_timeout(Duration::from_secs(1))?
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
//! [`ProcessTerminator::terminate`]: struct.ProcessTerminator.html#method.terminate
//! [`Receiver::recv_timeout`]: https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html#method.recv_timeout
//! [sealed]: https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed
//! [`Terminator::wait_for_output_with_timeout`]: trait.Terminator.html#tymethod.wait_for_output_with_timeout
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
use std::sync::mpsc;
use std::thread::Builder as ThreadBuilder;
use std::time::Duration;

#[cfg(unix)]
#[path = "unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

/// A wrapper that stores enough information to terminate a process.
///
/// Instances can only be constructed using [`Terminator::terminator`].
///
/// [`Terminator::terminator`]: trait.Terminator.html#tymethod.terminator
#[derive(Debug)]
pub struct ProcessTerminator(imp::Process);

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
    /// use process_control::Terminator;
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

fn run_with_timeout<TGetResultFn, TResult>(
    get_result_fn: TGetResultFn,
    time_limit: Duration,
) -> IoResult<Option<TResult>>
where
    TGetResultFn: 'static + FnOnce() -> TResult + Send,
    TResult: 'static + Send,
{
    let (result_sender, result_receiver) = mpsc::channel();
    let _ = ThreadBuilder::new()
        .spawn(move || result_sender.send(get_result_fn()))?;

    Ok(result_receiver.recv_timeout(time_limit).ok())
}

macro_rules! wait_and_terminate {
    ( $process:ident , $wait_fn:expr , $time_limit:ident $(,)? ) => {{
        let process_terminator = $process.terminator();
        let result = $wait_fn($process, $time_limit);
        // Errors terminating a process are less important than the result.
        let _ = process_terminator.terminate();
        result
    }};
}

/// Extensions to [`Child`] for easily killing processes.
///
/// For more information, see [the module-level documentation][module].
///
/// [module]: index.html
/// [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
pub trait Terminator: private::Sealed + Sized {
    /// Creates an instance of [`ProcessTerminator`] for this process.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io::Result as IoResult;
    /// use std::process::Command;
    ///
    /// use process_control::Terminator;
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

    /// A convenience method for calling [`Child::wait`] with a timeout.
    ///
    /// As the `Child` must be consumed by this method, it is returned if the
    /// process finishes. The instance would be required to subsequently access
    /// [`Child::stdout`] or other fields.
    ///
    /// For more information, see [`wait_for_output_with_timeout`].
    ///
    /// [`Child::stdout`]: https://doc.rust-lang.org/std/process/struct.Child.html#structfield.stdout
    /// [`Child::wait`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait
    /// [`wait_for_output_with_timeout`]: #tymethod.wait_for_output_with_timeout
    fn wait_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>>;

    /// A convenience method for calling [`Child::wait_with_output`] with a
    /// timeout.
    ///
    /// If the time limit expires before that method finishes, `Ok(None)` will
    /// be returned. The process will not be terminated, so it may be desirable
    /// to call [`ProcessTerminator::terminate_if_necessary`] afterward to free
    /// system resources. [`wait_for_output_with_terminating_timeout`] can be
    /// used to call that method automatically.
    ///
    /// This method will create a separate thread to run the method without
    /// blocking the current thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io::Result as IoResult;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::Terminator;
    ///
    /// # fn main() -> IoResult<()> {
    /// let process = Command::new("echo").spawn()?;
    /// let process_terminator = process.terminator();
    ///
    /// let result =
    ///     process.wait_for_output_with_timeout(Duration::from_secs(1))?;
    /// process_terminator.terminate_if_necessary()?;
    ///
    /// match result {
    ///     Some(output) => assert!(output.status.success()),
    ///     None => panic!("process timed out"),
    /// }
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
    /// [`ProcessTerminator::terminate_if_necessary`]: struct.ProcessTerminator.html#method.terminate_if_necessary
    /// [`wait_for_output_with_terminating_timeout`]: #tymethod.wait_for_output_with_terminating_timeout
    fn wait_for_output_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>>;

    /// A convenience method for calling [`wait_with_timeout`] and terminating
    /// the process if it exceeds the time limit.
    ///
    /// For more information, see [`wait_for_output_with_terminating_timeout`].
    ///
    /// [`wait_with_timeout`]: #tymethod.wait_with_timeout
    /// [`wait_for_output_with_terminating_timeout`]: #tymethod.wait_for_output_with_terminating_timeout
    fn wait_with_terminating_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>>;

    /// A convenience method for calling [`wait_for_output_with_timeout`] and
    /// terminating the process if it exceeds the time limit.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::io::Result as IoResult;
    /// use std::process::Command;
    /// use std::time::Duration;
    ///
    /// use process_control::Terminator;
    ///
    /// # fn main() -> IoResult<()> {
    /// let process = Command::new("echo").spawn()?;
    /// match process
    ///     .wait_for_output_with_terminating_timeout(Duration::from_secs(1))?
    /// {
    ///     Some(output) => assert!(output.status.success()),
    ///     None => panic!("process timed out"),
    /// }
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`wait_for_output_with_timeout`]: #tymethod.wait_for_output_with_timeout
    fn wait_for_output_with_terminating_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>>;
}

impl Terminator for Child {
    #[inline]
    fn terminator(&self) -> ProcessTerminator {
        ProcessTerminator(imp::Process::new(self))
    }

    #[inline]
    fn wait_with_timeout(
        mut self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>> {
        run_with_timeout(|| (self.wait(), self), time_limit)?
            .map(|(exit_status, process)| exit_status.map(|x| (x, process)))
            .transpose()
    }

    #[inline]
    fn wait_for_output_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>> {
        run_with_timeout(|| self.wait_with_output(), time_limit)?.transpose()
    }

    #[inline]
    fn wait_with_terminating_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>> {
        wait_and_terminate!(self, Self::wait_with_timeout, time_limit)
    }

    #[inline]
    fn wait_for_output_with_terminating_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>> {
        wait_and_terminate!(
            self,
            Self::wait_for_output_with_timeout,
            time_limit,
        )
    }
}

mod private {
    use std::process::Child;

    pub trait Sealed {}
    impl Sealed for Child {}
}
