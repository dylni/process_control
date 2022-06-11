use std::io;
use std::thread;

use process_control::ChildExt;
use process_control::Control;
use process_control::ExitStatus;

#[macro_use]
mod common;
use common::LONG_TIME_LIMIT;
use common::SHORT_TIME_LIMIT;

#[test]
fn test_time_limit() -> io::Result<()> {
    test!(
        command: common::create_time_limit_command(SHORT_TIME_LIMIT),
        time_limit: LONG_TIME_LIMIT,
        terminating: false,
        expected_result: Some(Some(0)),
        running: false,
    )
}

#[test]
fn test_time_limit_expired() -> io::Result<()> {
    test!(
        command: common::create_time_limit_command(LONG_TIME_LIMIT),
        time_limit: SHORT_TIME_LIMIT,
        terminating: false,
        expected_result: None,
        running: true,
    )
}

#[test]
fn test_terminating_time_limit() -> io::Result<()> {
    test!(
        command: common::create_time_limit_command(SHORT_TIME_LIMIT),
        time_limit: LONG_TIME_LIMIT,
        terminating: true,
        expected_result: Some(Some(0)),
        running: false,
    )
}

#[test]
fn test_terminating_time_limit_expired() -> io::Result<()> {
    test!(
        command: common::create_time_limit_command(LONG_TIME_LIMIT),
        time_limit: SHORT_TIME_LIMIT,
        terminating: true,
        expected_result: None,
        running: false,
    )
}
