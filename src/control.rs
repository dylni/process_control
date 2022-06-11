use std::io;
use std::io::Read;
use std::panic;
use std::process::Child;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use super::imp;
use super::Control;
use super::ExitStatus;
use super::Output;
use super::Timeout;
use super::WaitResult;

macro_rules! r#impl {
    (
        $struct:ident $(< $lifetime:lifetime >)? ,
        $deprecated_struct:ident,
        $process_type:ty ,
        $return_type:ty ,
        $wait_fn:expr $(,)?
    ) => {
        #[derive(Debug)]
        pub struct $struct$(<$lifetime>)? {
            handle: imp::SharedHandle,
            process: $process_type,
            #[cfg(any(
                target_os = "android",
                all(
                    target_os = "linux",
                    any(target_env = "gnu", target_env = "musl"),
                ),
                windows,
            ))]
            memory_limit: Option<usize>,
            strict_errors: bool,
            terminate_for_timeout: bool,
        }

        impl$(<$lifetime>)? $struct$(<$lifetime>)? {
            pub(super) fn new(process: $process_type) -> Self {
                // SAFETY: The process is stored in this struct.
                Self {
                    handle: unsafe { imp::SharedHandle::new(&process) },
                    process,
                    #[cfg(any(
                        target_os = "android",
                        all(
                            target_os = "linux",
                            any(target_env = "gnu", target_env = "musl"),
                        ),
                        windows,
                    ))]
                    memory_limit: None,
                    strict_errors: false,
                    terminate_for_timeout: false,
                }
            }

            fn run_wait(&mut self) -> WaitResult<ExitStatus> {
                let result = self.process.try_wait();
                if let Ok(Some(exit_status)) = result {
                    return Ok(Some(exit_status.into()));
                }

                #[cfg(any(
                    target_os = "android",
                    all(
                        target_os = "linux",
                        any(target_env = "gnu", target_env = "musl"),
                    ),
                    windows,
                ))]
                if let Some(memory_limit) = self.memory_limit {
                    self.handle.set_memory_limit(memory_limit)?;
                }
                self.handle.wait().map(|x| x.map(ExitStatus))
            }
        }

        impl$(<$lifetime>)? Control for $struct$(<$lifetime>)? {
            type Result = $return_type;

            if_memory_limit! {
                #[inline]
                fn memory_limit(mut self, limit: usize) -> Self {
                    self.memory_limit = Some(limit);
                    self
                }
            }

            #[inline]
            fn time_limit(mut self, limit: Duration) -> Self {
                self.handle.time_limit = Some(limit);
                self
            }

            #[inline]
            fn strict_errors(mut self) -> Self {
                self.strict_errors = true;
                self
            }

            #[inline]
            fn terminate_for_timeout(mut self) -> Self {
                self.terminate_for_timeout = true;
                self
            }

            #[inline]
            fn wait(mut self) -> WaitResult<Self::Result> {
                let _ = self.process.stdin.take();
                let mut result = $wait_fn(&mut self);

                macro_rules! try_run {
                    ( $get_result_fn:expr ) => {
                        if result.is_ok() {
                            if let Err(error) = $get_result_fn() {
                                result = Err(error);
                            }
                        }
                    };
                }

                // If the process exited normally, identifier reuse might cause
                // a different process to be terminated.
                if self.terminate_for_timeout && !matches!(result, Ok(Some(_)))
                {
                    let next_result =
                        imp::terminate_if_running(&mut self.process)
                            .and_then(|()| self.process.wait());
                    if self.strict_errors {
                        try_run!(|| next_result);
                    }
                }
                try_run!(|| self.process.try_wait());

                result
            }
        }

        #[deprecated = concat!("use `", stringify!($struct), "` instead")]
        #[derive(Debug)]
        pub struct $deprecated_struct$(<$lifetime>)?($struct$(<$lifetime>)?);

        impl$(<$lifetime>)? $deprecated_struct$(<$lifetime>)? {
            #[deprecated = concat!(
                "use `",
                stringify!($struct),
                "::new` and `",
                stringify!($struct),
                "::time_limit` instead",
            )]
            pub(super) fn new(
                process: $process_type,
                time_limit: Duration,
            ) -> Self {
                Self($struct::new(process).time_limit(time_limit))
            }
        }

        impl$(<$lifetime>)? Timeout for $deprecated_struct$(<$lifetime>)? {
            type Result = $return_type;

            #[inline]
            fn strict_errors(self) -> Self {
                Self(self.0.strict_errors())
            }

            #[inline]
            fn terminating(self) -> Self {
                Self(self.0.terminate_for_timeout())
            }

            #[inline]
            fn wait(self) -> WaitResult<Self::Result> {
                self.0.wait()
            }
        }
    };
}

r#impl!(
    ExitStatusControl<'a>,
    ExitStatusTimeout,
    &'a mut Child,
    ExitStatus,
    Self::run_wait,
);

impl OutputControl {
    fn run_wait_with_output(&mut self) -> WaitResult<Output> {
        let stdout_reader = spawn_reader(&mut self.process.stdout)?;
        let stderr_reader = spawn_reader(&mut self.process.stderr)?;

        return self
            .run_wait()?
            .map(|status| {
                Ok(Output {
                    status,
                    stdout: join_reader(stdout_reader)?,
                    stderr: join_reader(stderr_reader)?,
                })
            })
            .transpose();

        type Reader = Option<JoinHandle<io::Result<Vec<u8>>>>;

        fn spawn_reader<R>(source: &mut Option<R>) -> io::Result<Reader>
        where
            R: 'static + Read + Send,
        {
            source
                .take()
                .map(|mut source| {
                    thread::Builder::new().spawn(move || {
                        let mut buffer = Vec::new();
                        let _ = source.read_to_end(&mut buffer)?;
                        Ok(buffer)
                    })
                })
                .transpose()
        }

        fn join_reader(reader: Reader) -> io::Result<Vec<u8>> {
            reader
                .map(|x| x.join().unwrap_or_else(|x| panic::resume_unwind(x)))
                .transpose()
                .map(|x| x.unwrap_or_else(Vec::new))
        }
    }
}

r#impl!(
    OutputControl,
    OutputTimeout,
    Child,
    Output,
    Self::run_wait_with_output,
);
