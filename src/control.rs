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

fn read_to_end<R>(mut reader: R) -> io::Result<Vec<u8>>
where
    R: Read,
{
    let mut output = Vec::new();
    let _ = reader.read_to_end(&mut output)?;
    Ok(output)
}

struct Reader(Option<JoinHandle<io::Result<Vec<u8>>>>);

impl Reader {
    fn spawn<R>(source: Option<R>) -> io::Result<Self>
    where
        R: 'static + Read + Send,
    {
        source
            .map(|x| thread::Builder::new().spawn(move || read_to_end(x)))
            .transpose()
            .map(Self)
    }

    fn join(self) -> io::Result<Vec<u8>> {
        self.0
            .map(|x| x.join().unwrap_or_else(|x| panic::resume_unwind(x)))
            .unwrap_or_else(|| Ok(Vec::new()))
    }
}

#[derive(Clone, Copy, Debug)]
struct Limits {
    #[cfg(process_control_memory_limit)]
    memory: Option<usize>,
    time: Option<Duration>,
}

pub trait Process {
    type Result;

    fn get(&mut self) -> &mut Child;

    #[allow(private_interfaces)]
    fn run_wait(&mut self, limits: Limits) -> WaitResult<Self::Result>;
}

impl Process for &mut Child {
    type Result = ExitStatus;

    fn get(&mut self) -> &mut Child {
        self
    }

    #[allow(private_interfaces)]
    fn run_wait(&mut self, limits: Limits) -> WaitResult<Self::Result> {
        let result = self.try_wait();
        if let Ok(Some(exit_status)) = result {
            return Ok(Some(exit_status.into()));
        }

        let mut handle = imp::Process::new(self);
        #[cfg(process_control_memory_limit)]
        if let Some(memory_limit) = limits.memory {
            handle.set_memory_limit(memory_limit)?;
        }
        handle.wait(limits.time).map(|x| x.map(ExitStatus))
    }
}

impl Process for Child {
    type Result = Output;

    fn get(&mut self) -> &mut Child {
        self
    }

    #[allow(private_interfaces)]
    fn run_wait(&mut self, limits: Limits) -> WaitResult<Self::Result> {
        macro_rules! reader {
            ( $stream:ident ) => {
                Reader::spawn(self.$stream.take())
            };
        }

        let stdout_reader = reader!(stdout)?;
        let stderr_reader = reader!(stderr)?;

        (&mut &mut *self)
            .run_wait(limits)?
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

#[derive(Debug)]
pub struct Buffer<P>
where
    P: Process,
{
    process: P,
    limits: Limits,
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
            limits: Limits {
                #[cfg(process_control_memory_limit)]
                memory: None,
                time: None,
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
        self.limits.memory = Some(limit);
        self
    }

    #[inline]
    fn time_limit(mut self, limit: Duration) -> Self {
        self.limits.time = Some(limit);
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
        let _ = self.process.get().stdin.take();
        let mut result = self.process.run_wait(self.limits);

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

        let process = self.process.get();
        // If the process exited normally, identifier reuse might cause a
        // different process to be terminated.
        if self.terminate_for_timeout && !matches!(result, Ok(Some(_))) {
            let next_result = process.kill().and_then(|()| process.wait());
            if self.strict_errors {
                run_if_ok!(|| next_result);
            }
        }
        run_if_ok!(|| process.try_wait());

        result
    }
}
