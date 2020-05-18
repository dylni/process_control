use std::io;
use std::io::Read;
use std::panic;
use std::process::Child;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use super::imp;
use super::ExitStatus;
use super::Output;
use super::Timeout;

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
            handle: imp::Handle,
            time_limit: Duration,
            strict_errors: bool,
            terminate: bool,
        }

        impl$(<$lifetime>)? $struct$(<$lifetime>)? {
            pub(crate) fn new(
                process: $process_type,
                time_limit: Duration,
            ) -> Self {
                Self {
                    handle: imp::Handle::inherited(&process),
                    process,
                    time_limit,
                    strict_errors: false,
                    terminate: false,
                }
            }

            fn run_wait(&mut self) -> io::Result<Option<ExitStatus>> {
                // Check if the exit status was already captured.
                let result = self.process.try_wait();
                if let Ok(Some(exit_status)) = result {
                    return Ok(Some(exit_status.into()));
                }

                self
                    .handle
                    .wait_with_timeout(self.time_limit)
                    .map(|x| x.map(ExitStatus))
            }
        }

        #[allow(single_use_lifetimes)]
        impl$(<$lifetime>)? Timeout for $struct$(<$lifetime>)? {
            type Result = $return_type;

            #[inline]
            fn strict_errors(mut self) -> Self {
                self.strict_errors = true;
                self
            }

            #[inline]
            fn terminating(mut self) -> Self {
                self.terminate = true;
                self
            }

            #[inline]
            fn wait(mut self) -> io::Result<Option<Self::Result>> {
                let _ = self.process.stdin.take();
                let mut result = $wait_fn(&mut self);

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
        }
    };
}

r#impl!(
    ExitStatusTimeout<'a>,
    &'a mut Child,
    ExitStatus,
    Self::run_wait,
);

r#impl!(OutputTimeout, Child, Output, |timeout: &mut Self| {
    let stdout_reader = spawn_reader(&mut timeout.process.stdout)?;
    let stderr_reader = spawn_reader(&mut timeout.process.stderr)?;

    return timeout
        .run_wait()?
        .map(|x| {
            Ok(Output {
                status: x,
                stdout: join_reader(stdout_reader)?,
                stderr: join_reader(stderr_reader)?,
            })
        })
        .transpose();

    fn spawn_reader<TSource>(
        source: &mut Option<TSource>,
    ) -> io::Result<Option<JoinHandle<io::Result<Vec<u8>>>>>
    where
        TSource: 'static + Read + Send,
    {
        source
            .take()
            .map(|mut x| {
                thread::Builder::new().spawn(move || {
                    let mut buffer = Vec::new();
                    let _ = x.read_to_end(&mut buffer)?;
                    Ok(buffer)
                })
            })
            .transpose()
    }

    fn join_reader(
        reader: Option<JoinHandle<io::Result<Vec<u8>>>>,
    ) -> io::Result<Vec<u8>> {
        reader
            .map(|x| x.join().unwrap_or_else(|x| panic::resume_unwind(x)))
            .transpose()
            .map(|x| x.unwrap_or_else(Vec::new))
    }
});
