//! This crate allows terminating a process without a mutable reference. Thus,
//! it becomes possible to abort early from waiting for output or an exit code
//! – primarily through [`ProcessTerminator::terminate`]. That method is
//! intentionally designed to not require a reference of any kind to the
//! [`Child`] instance, to allow for maximal flexibility.
//!
//! Typically, it is not possible to terminate a process during a call to
//! [`Child::wait`] or [`Child::wait_with_output`] in another thread, since
//! [`Child::kill`] takes a mutable reference. However, since this crate
//! creates its own termination method, there is no issue, and useful methods
//! such as [`Terminator::wait_for_output_with_timeout`] can exist.
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
    /// This method powers the entire crate. All other methods are possible to
    /// implement using this method, since it does not require a reference to
    /// the [`Child`] instance for the process.
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
    /// [`Child`]: https://doc.rust-lang.org/std/process/struct.Child.html
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
    process: Child,
    time_limit: Duration,
    get_result_fn: TGetResultFn,
) -> IoResult<Option<TResult>>
where
    TGetResultFn: 'static + FnOnce(Child) -> TResult + Send,
    TResult: 'static + Send,
{
    let process_terminator = process.terminator();

    let (result_sender, result_receiver) = mpsc::channel();
    ThreadBuilder::new()
        .spawn(move || result_sender.send(get_result_fn(process)))?;

    let result = result_receiver.recv_timeout(time_limit).ok();
    // Errors terminating a process are less important than the result.
    let _ = process_terminator.terminate();

    Ok(result)
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
    /// If the time limit expires before that method returns,
    /// [`ProcessTerminator::terminate`] will be called in another thread,
    /// without waiting for the process to finish. `Ok(None)` will be returned
    /// in that case.
    ///
    /// As the `Child` must be consumed by this method, it is returned if the
    /// process finishes. The instance would be required to subsequently access
    /// [`Child::stdout`] or other fields. That pattern is unique to this
    /// method; [`wait_for_output_with_timeout`] captures all output, so the
    /// instance should be unnecessary afterward.
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
    /// match process.wait_with_timeout(Duration::from_secs(1))? {
    ///     Some((exit_status, _)) => assert!(exit_status.success()),
    ///     None => panic!("process timed out"),
    /// }
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// [`Child::stdout`]: https://doc.rust-lang.org/std/process/struct.Child.html#structfield.stdout
    /// [`Child::wait`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait
    /// [`ProcessTerminator::terminate`]: struct.ProcessTerminator.html#method.terminate
    /// [`wait_for_output_with_timeout`]: #tymethod.wait_for_output_with_timeout
    fn wait_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>>;

    /// A convenience method for calling [`Child::wait_with_output`] with a
    /// timeout.
    ///
    /// For more information, see [`wait_with_timeout`].
    ///
    /// [`Child::wait_with_output`]: https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output
    /// [`wait_with_timeout`]: #tymethod.wait_with_timeout
    fn wait_for_output_with_timeout(
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
        self,
        time_limit: Duration,
    ) -> IoResult<Option<(ExitStatus, Self)>> {
        run_with_timeout(self, time_limit, |mut x| (x.wait(), x))?
            .map(|(exit_status, process)| exit_status.map(|x| (x, process)))
            .transpose()
    }

    #[inline]
    fn wait_for_output_with_timeout(
        self,
        time_limit: Duration,
    ) -> IoResult<Option<Output>> {
        run_with_timeout(self, time_limit, Self::wait_with_output)?.transpose()
    }
}

mod private {
    use std::process::Child;

    pub trait Sealed {}
    impl Sealed for Child {}
}
