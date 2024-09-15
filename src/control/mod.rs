use std::panic;
use std::process::Child;
use std::thread;
use std::time::Duration;

use super::imp;
use super::Control;
use super::ExitStatus;
use super::Output;
use super::PipeFilter;
use super::WaitResult;

mod pipe;
pub(super) use pipe::Pipe;

#[derive(Debug)]
struct Options {
    #[cfg(process_control_memory_limit)]
    memory_limit: Option<usize>,
    time_limit: Option<Duration>,
    stdout_filter: Option<pipe::FilterWrapper>,
    stderr_filter: Option<pipe::FilterWrapper>,
}

pub(super) trait Process {
    type Result: AsRef<ExitStatus>;

    fn get(&mut self) -> &mut Child;

    #[allow(private_interfaces)]
    fn run_wait(&mut self, options: Options) -> WaitResult<Self::Result>;
}

impl Process for &mut Child {
    type Result = ExitStatus;

    fn get(&mut self) -> &mut Child {
        self
    }

    #[allow(private_interfaces)]
    fn run_wait(&mut self, options: Options) -> WaitResult<Self::Result> {
        let result = self.try_wait();
        if let Ok(Some(exit_status)) = result {
            return Ok(Some(exit_status.into()));
        }

        let mut handle = imp::Process::new(self);
        #[cfg(process_control_memory_limit)]
        if let Some(memory_limit) = options.memory_limit {
            handle.set_memory_limit(memory_limit)?;
        }
        let result = handle.wait(options.time_limit)?;
        result
            .map(|result| {
                self.try_wait().map(|std_result| {
                    ExitStatus::new(
                        result,
                        std_result.expect("missing exit status"),
                    )
                })
            })
            .transpose()
    }
}

impl Process for Child {
    type Result = Output;

    fn get(&mut self) -> &mut Child {
        self
    }

    #[allow(private_interfaces)]
    fn run_wait(&mut self, mut options: Options) -> WaitResult<Self::Result> {
        macro_rules! pipe {
            ( $pipe:ident , $filter:ident ) => {{
                let filter = options.$filter.take();
                self.$pipe.take().map(|x| Pipe::new(x.into(), filter))
            }};
        }

        let pipes =
            [pipe!(stdout, stdout_filter), pipe!(stderr, stderr_filter)];
        let reader =
            thread::Builder::new().spawn(move || imp::read2(pipes))?;

        (&mut &mut *self)
            .run_wait(options)?
            .map(|status| {
                reader
                    .join()
                    .unwrap_or_else(|x| panic::resume_unwind(x))
                    .map(|[stdout, stderr]| Output {
                        status,
                        stdout,
                        stderr,
                    })
            })
            .transpose()
    }
}

#[derive(Debug)]
pub(super) struct Buffer<P>
where
    P: Process,
{
    process: P,
    options: Options,
    strict_errors: bool,
    terminate_for_timeout: bool,
}

impl<P> Buffer<P>
where
    P: Process,
{
    pub(super) const fn new(process: P) -> Self {
        Self {
            process,
            options: Options {
                #[cfg(process_control_memory_limit)]
                memory_limit: None,
                time_limit: None,
                stdout_filter: None,
                stderr_filter: None,
            },
            strict_errors: false,
            terminate_for_timeout: false,
        }
    }
}

impl<P> Control for Buffer<P>
where
    P: Process,
{
    type Result = P::Result;

    #[cfg(any(doc, process_control_memory_limit))]
    #[inline]
    fn memory_limit(mut self, limit: usize) -> Self {
        self.options.memory_limit = Some(limit);
        self
    }

    #[inline]
    fn time_limit(mut self, limit: Duration) -> Self {
        self.options.time_limit = Some(limit);
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
    fn stdout_filter<T>(mut self, filter: T) -> Self
    where
        Self: Control<Result = Output>,
        T: PipeFilter,
    {
        assert!(self.process.get().stdout.is_some(), "stdout is not piped");

        self.options.stdout_filter = Some(filter.into());
        self
    }

    #[inline]
    fn stderr_filter<T>(mut self, filter: T) -> Self
    where
        Self: Control<Result = Output>,
        T: PipeFilter,
    {
        assert!(self.process.get().stderr.is_some(), "stderr is not piped");

        self.options.stderr_filter = Some(filter.into());
        self
    }

    #[inline]
    fn wait(mut self) -> WaitResult<Self::Result> {
        let _ = self.process.get().stdin.take();
        let mut result = self.process.run_wait(self.options);

        let process = self.process.get();
        // If the process exited normally, identifier reuse might cause a
        // different process to be terminated.
        if self.terminate_for_timeout && !matches!(result, Ok(Some(_))) {
            let next_result = process.kill().and_then(|()| process.wait());
            if self.strict_errors && result.is_ok() {
                if let Err(error) = next_result {
                    result = Err(error);
                }
            }
        }

        result
    }
}
