#![cfg(process_control_memory_limit)]

use std::io;
use std::process::Command;
use std::process::Stdio;
use std::thread;

use process_control::ChildExt;
use process_control::Control;
use process_control::ExitStatus;

#[macro_use]
mod common;
use common::MEMORY_LIMIT;
use common::SHORT_TIME_LIMIT;

fn create_memory_limit_command(bytes: usize) -> Command {
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

#[test]
fn test_memory_limit() -> io::Result<()> {
    test!(
        command: create_memory_limit_command(MEMORY_LIMIT),
        memory_limit: 2 * MEMORY_LIMIT,
        terminating: false,
        expected_result: Some(Some(0)),
        running: false,
    )
}

#[test]
fn test_memory_limit_exceeded() -> io::Result<()> {
    test!(
        command: create_memory_limit_command(MEMORY_LIMIT),
        memory_limit: MEMORY_LIMIT,
        terminating: false,
        expected_result: Some(Some(1)),
        running: false,
    )
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
fn test_memory_limit_0() -> io::Result<()> {
    test!(
        command: create_memory_limit_command(MEMORY_LIMIT),
        memory_limit: 0,
        terminating: false,
        expected_result: Some(memory_limit_0_result!()),
        running: false,
    )
}

#[test]
fn test_memory_limit_1() -> io::Result<()> {
    test!(
        command: create_memory_limit_command(MEMORY_LIMIT),
        memory_limit: 1,
        terminating: false,
        expected_result: Some(memory_limit_0_result!()),
        running: false,
    )
}
