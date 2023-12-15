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
use super::WaitResult;

macro_rules! r#impl {
    (
        $struct:ident $(< $lifetime:lifetime >)? ,
        $process_type:ty ,
        $return_type:ty ,
        $wait_fn:expr $(,)?
    ) => {
        #[derive(Debug)]
        pub struct $struct$(<$lifetime>)? {
            process: $process_type,
            #[cfg(process_control_memory_limit)]
            memory_limit: Option<usize>,
            time_limit: Option<Duration>,
            strict_errors: bool,
            terminate_for_timeout: bool,
        }

        impl$(<$lifetime>)? $struct$(<$lifetime>)? {
            pub(super) fn new(process: $process_type) -> Self {
                Self {
                    process,
                    #[cfg(process_control_memory_limit)]
                    memory_limit: None,
                    time_limit: None,
                    strict_errors: false,
                    terminate_for_timeout: false,
                }
            }

            fn run_wait(&mut self) -> WaitResult<ExitStatus> {
                let result = self.process.try_wait();
                if let Ok(Some(exit_status)) = result {
                    return Ok(Some(exit_status.into()));
                }

                let mut handle = imp::Process::new(&mut self.process);
                #[cfg(process_control_memory_limit)]
                if let Some(memory_limit) = self.memory_limit {
                    handle.set_memory_limit(memory_limit)?;
                }
                handle.wait(self.time_limit).map(|x| x.map(ExitStatus))
            }
        }

        impl$(<$lifetime>)? Control for $struct$(<$lifetime>)? {
            type Result = $return_type;

            #[cfg(any(doc, process_control_memory_limit))]
            #[inline]
            fn memory_limit(mut self, limit: usize) -> Self {
                self.memory_limit = Some(limit);
                self
            }

            #[inline]
            fn time_limit(mut self, limit: Duration) -> Self {
                self.time_limit = Some(limit);
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

                macro_rules! run_if_ok {
                    ( $get_result_fn:expr ) => {
                        if result.is_ok() {
                            #[allow(clippy::redundant_closure_call)]
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
                        run_if_ok!(|| next_result);
                    }
                }
                run_if_ok!(|| self.process.try_wait());

                result
            }
        }
    };
}

r#impl!(
    ExitStatusControl<'a>,
    &'a mut Child,
    ExitStatus,
    Self::run_wait,
);

struct Reader(Option<JoinHandle<io::Result<Vec<u8>>>>);

impl Reader {
    fn spawn<R>(source: Option<R>) -> io::Result<Self>
    where
        R: 'static + Read + Send,
    {
        source
            .map(|mut source| {
                thread::Builder::new().spawn(move || {
                    let mut buffer = Vec::new();
                    let _ = source.read_to_end(&mut buffer)?;
                    Ok(buffer)
                })
            })
            .transpose()
            .map(Self)
    }

    fn join(self) -> io::Result<Vec<u8>> {
        self.0
            .map(|x| x.join().unwrap_or_else(|x| panic::resume_unwind(x)))
            .transpose()
            .map(|x| x.unwrap_or_else(Vec::new))
    }
}

impl OutputControl {
    fn run_wait_with_output(&mut self) -> WaitResult<Output> {
        macro_rules! reader {
            ( $stream:ident ) => {
                Reader::spawn(self.process.$stream.take())
            };
        }

        let stdout_reader = reader!(stdout)?;
        let stderr_reader = reader!(stderr)?;

        self.run_wait()?
            .map(|status| {
                Ok(Output {
                    status,
                    stdout: stdout_reader.join()?,
                    stderr: stderr_reader.join()?,
                })
            })
            .transpose()
    }
}

r#impl!(OutputControl, Child, Output, Self::run_wait_with_output);
