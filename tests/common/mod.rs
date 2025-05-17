#![allow(dead_code)]

use std::io;
use std::process::Child;
use std::process::Command;
use std::thread;
use std::time::Duration;

use process_control::ChildExt;
use process_control::Control;
use process_control::ExitStatus;

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
mod imp;
pub(super) use imp::Handle;

pub(super) const SHORT_TIME_LIMIT: Duration = Duration::from_secs(2);

pub(super) const LONG_TIME_LIMIT: Duration = Duration::from_secs(5);

#[attr_alias::eval]
#[attr_alias(memory_limit)]
pub(super) const MEMORY_LIMIT: usize = 104_857_600;

macro_rules! assert_matches {
    ( $result:expr , $($token:tt)* ) => {{
        let result = $result;
        if !matches!(result, $($token)*) {
            panic!(
                "assertion failed: `(left matches right)`
  left: `{:?}`
 right: `{:?}`",
                result,
                stringify!($($token)*),
            );
        }
    }};
}

pub(super) trait Spawn {
    fn spawn(&mut self) -> io::Result<Child>;
}

impl Spawn for Command {
    fn spawn(&mut self) -> io::Result<Child> {
        self.spawn()
    }
}

#[attr_alias::eval]
#[derive(Clone, Copy)]
pub(super) enum Limit {
    #[attr_alias(memory_limit)]
    Memory(usize),
    Time(Duration),
}

#[derive(Clone, Copy)]
pub(super) struct __Test {
    pub(super) is_expected_fn: fn(Option<Option<i64>>) -> bool,
    pub(super) running: bool,
}

#[attr_alias::eval]
impl __Test {
    fn run_one<F>(self, mut wait_fn: F)
    where
        F: FnMut() -> io::Result<Result<ExitStatus, Handle>>,
    {
        let exit_code =
            wait_fn()
                .expect("failed to run process")
                .map(|exit_status| {
                    assert_eq!(
                        exit_status.code(),
                        exit_status.into_std_lossy().code().map(Into::into),
                    );
                    exit_status.code()
                });
        assert!((self.is_expected_fn)(exit_code.as_ref().ok().copied()));

        if self.running {
            thread::sleep(SHORT_TIME_LIMIT);
        }
        match exit_code {
            Ok(_) => assert!(!self.running),
            Err(handle) => {
                assert_matches!(
                    handle.is_possibly_running(),
                    Ok(x) if x == self.running,
                );
            }
        }
    }

    fn run_many<T>(self, options: &mut __Options<T>)
    where
        T: Spawn,
    {
        macro_rules! run_one {
            ( $method:ident ) => {
                self.run_one(|| {
                    #[allow(unused_mut)]
                    let mut process = options.command.spawn()?;
                    let handle = Handle::new(&process)?;
                    options.wait(process.$method()).map(|x| x.ok_or(handle))
                });
            };
        }

        for strict_errors in [false, true] {
            options.strict_errors = strict_errors;
            run_one!(controlled);
            run_one!(controlled_with_output);
        }
    }

    pub(super) fn run<T>(self, mut options: __Options<T>, limit: Limit)
    where
        T: Spawn,
    {
        match limit {
            #[attr_alias(memory_limit)]
            Limit::Memory(limit) => {
                options.memory_limit = limit;
                self.run_many(&mut options);

                options.time_limit = Some(LONG_TIME_LIMIT);
                self.run_many(&mut options);
            }
            Limit::Time(limit) => {
                options.time_limit = Some(limit);
                self.run_many(&mut options);
            }
        }
    }
}

#[attr_alias::eval]
pub(super) struct __Options<T>
where
    T: Spawn,
{
    command: T,
    #[attr_alias(memory_limit)]
    memory_limit: usize,
    strict_errors: bool,
    terminating: bool,
    time_limit: Option<Duration>,
}

#[attr_alias::eval]
impl<T> __Options<T>
where
    T: Spawn,
{
    pub(super) const fn new(command: T, terminating: bool) -> Self {
        Self {
            command,
            #[attr_alias(memory_limit)]
            memory_limit: MEMORY_LIMIT,
            strict_errors: false,
            terminating,
            time_limit: None,
        }
    }

    fn wait<C>(&mut self, mut control: C) -> io::Result<Option<ExitStatus>>
    where
        C: Control,
        C::Result: Into<ExitStatus>,
    {
        #[attr_alias(memory_limit)]
        {
            control = control.memory_limit(self.memory_limit);
        }
        if self.strict_errors {
            control = control.strict_errors();
        }
        if self.terminating {
            control = control.terminate_for_timeout();
        }
        if let Some(time_limit) = self.time_limit {
            control = control.time_limit(time_limit);
        }
        control.wait().map(|x| x.map(Into::into))
    }
}

macro_rules! test_common {
    (
        command: $command:expr ,
        limit: $limit:expr ,
        terminating: $terminating:expr ,
        expected_result: $expected_result:pat ,
        running: $running:expr ,
    ) => {{
        use $crate::common::__Options;
        use $crate::common::__Test;

        __Test {
            #[allow(clippy::redundant_pattern_matching)]
            is_expected_fn: |x| matches!(x, $expected_result),
            running: $running,
        }
        .run(__Options::new($command, $terminating), $limit);
    }};
}

pub(super) fn create_time_limit_command(seconds: Duration) -> Command {
    let whole_seconds = seconds.as_secs();
    assert_eq!(seconds, Duration::from_secs(whole_seconds));

    let mut command = Command::new("perl");
    let _ = command
        .arg("-e")
        .arg("sleep $ARGV[0]")
        .arg("--")
        .arg(whole_seconds.to_string());
    command
}
