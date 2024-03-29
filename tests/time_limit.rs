use std::io;
use std::process::Child;
use std::process::Command;
use std::time::Duration;

#[macro_use]
mod common;
use common::Limit;
use common::Spawn;
use common::LONG_TIME_LIMIT;
use common::SHORT_TIME_LIMIT;

struct FinishedCommand(Command);

impl FinishedCommand {
    fn new() -> Self {
        Self(common::create_time_limit_command(Duration::ZERO))
    }
}

impl Spawn for FinishedCommand {
    fn spawn(&mut self) -> io::Result<Child> {
        let mut process = self.0.spawn()?;
        let _ = process.wait()?;
        Ok(process)
    }
}

macro_rules! test {
    (
        command: $command:expr ,
        limit: $limit:expr ,
        terminating: $terminating:expr ,
        expected_result: $expected_result:pat ,
        running: $running:expr ,
    ) => {
        test_common!(
            command: common::create_time_limit_command($command),
            limit: Limit::Time($limit),
            terminating: $terminating,
            expected_result: $expected_result,
            running: $running,
        );
    };
}

#[test]
fn test_accept() {
    test!(
        command: SHORT_TIME_LIMIT,
        limit: LONG_TIME_LIMIT,
        terminating: false,
        expected_result: Some(Some(0)),
        running: false,
    );
}

#[test]
fn test_reject() {
    test!(
        command: LONG_TIME_LIMIT,
        limit: SHORT_TIME_LIMIT,
        terminating: false,
        expected_result: None,
        running: true,
    );
}

#[test]
fn test_terminating_accept() {
    test!(
        command: SHORT_TIME_LIMIT,
        limit: LONG_TIME_LIMIT,
        terminating: true,
        expected_result: Some(Some(0)),
        running: false,
    );
}

#[test]
fn test_terminating_reject() {
    test!(
        command: LONG_TIME_LIMIT,
        limit: SHORT_TIME_LIMIT,
        terminating: true,
        expected_result: None,
        running: false,
    );
}

#[test]
fn test_0() {
    test_common!(
        command: FinishedCommand::new(),
        limit: Limit::Time(Duration::ZERO),
        terminating: false,
        expected_result: Some(Some(0)),
        running: false,
    );
}

#[test]
fn test_1() {
    test_common!(
        command: FinishedCommand::new(),
        limit: Limit::Time(Duration::from_millis(1)),
        terminating: false,
        expected_result: Some(Some(0)),
        running: false,
    );
}
