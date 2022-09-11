use std::io;
use std::process::Command;
use std::process::Stdio;
use std::thread;

use process_control::ChildExt;
use process_control::Control;
use process_control::ExitStatus;

#[macro_use]
mod common;
use common::LONG_TIME_LIMIT;
use common::SHORT_TIME_LIMIT;

#[test]
fn test_stdin() -> io::Result<()> {
    let mut command = Command::new("perl");
    let _ = command.stdin(Stdio::piped());

    test!(
        command: command,
        time_limit: LONG_TIME_LIMIT,
        terminating: false,
        expected_result: Some(Some(0)),
        running: false,
    )
}

#[test]
fn test_large_output() -> io::Result<()> {
    const BUFFER_COUNT: usize = 1024;
    const BUFFER_LENGTH: usize = 1024;
    const OUTPUT_LENGTH: usize = BUFFER_COUNT * BUFFER_LENGTH;

    #[track_caller]
    fn test_output(output: Vec<u8>, byte: u8) {
        assert_eq!(OUTPUT_LENGTH, output.len());
        assert!(output.into_iter().all(|x| x == byte));
    }

    let process = Command::new("perl")
        .arg("-e")
        .arg(
            r"for (my $i = 0; $i < $ARGV[0]; $i++) {
                print 'a' x $ARGV[1];
                print STDERR 'b' x $ARGV[1];
            }",
        )
        .arg("--")
        .arg(BUFFER_COUNT.to_string())
        .arg(BUFFER_LENGTH.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = process
        .controlled_with_output()
        .time_limit(LONG_TIME_LIMIT)
        .strict_errors()
        .wait()?
        .unwrap();

    assert_eq!(Some(0), output.status.code());

    test_output(output.stdout, b'a');
    test_output(output.stderr, b'b');

    Ok(())
}

#[test]
fn test_terminate_if_running() -> io::Result<()> {
    let mut process =
        common::create_time_limit_command(LONG_TIME_LIMIT).spawn()?;

    process.terminate_if_running()?;
    process.terminate_if_running()?;

    thread::sleep(SHORT_TIME_LIMIT);

    process.terminate_if_running()?;
    if cfg!(windows) {
        assert!(process.kill().is_err());
    } else {
        process.kill()?;
    }

    Ok(())
}
