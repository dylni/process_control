#![cfg(process_control_memory_limit)]

use std::process::Command;
use std::process::Stdio;

#[macro_use]
mod common;
use common::Limit;
use common::MEMORY_LIMIT;
use common::SHORT_TIME_LIMIT;

fn create_command(bytes: usize) -> Command {
    let mut command = Command::new("perl");
    let _ = command
        .arg("-e")
        .arg("sleep $ARGV[1]; my $bytes = 'a' x $ARGV[0]; print $bytes")
        .arg("--")
        .arg(bytes.to_string())
        .arg(SHORT_TIME_LIMIT.as_secs().to_string())
        .stderr(Stdio::null())
        .stdout(Stdio::null());
    command
}

macro_rules! test {
    (
        limit: $limit:expr ,
        expected_result: $expected_result:pat ,
    ) => {
        test_common!(
            command: create_command(MEMORY_LIMIT),
            limit: Limit::Memory($limit),
            terminating: false,
            expected_result: $expected_result,
            running: false,
        );
    };
}

#[test]
fn test_accept() {
    test!(
        limit: 2 * MEMORY_LIMIT,
        expected_result: Some(Some(0)),
    );
}

#[test]
fn test_reject() {
    test!(
        limit: MEMORY_LIMIT,
        expected_result: Some(Some(1)),
    );
}

#[cfg(windows)]
macro_rules! memory_limit_0_result {
    () => {
        Some(1)
    };
}
#[cfg(not(windows))]
macro_rules! memory_limit_0_result {
    () => {
        _
    };
}

#[test]
fn test_0() {
    test!(
        limit: 0,
        expected_result: Some(memory_limit_0_result!()),
    );
}

#[test]
fn test_1() {
    test!(
        limit: 1,
        expected_result: Some(memory_limit_0_result!()),
    );
}
