use std::io;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use process_control::ChildExt;
use process_control::Control;

#[macro_use]
mod common;
use common::Limit;
use common::LONG_TIME_LIMIT;

#[test]
fn test_stdin() {
    let mut command = Command::new("perl");
    let _ = command.stdin(Stdio::piped());

    test_common!(
        command: command,
        limit: Limit::Time(LONG_TIME_LIMIT),
        terminating: false,
        expected_result: Some(Some(0)),
        running: false,
    );
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

    #[track_caller]
    fn test_stream_output(stream: &[(bool, Vec<u8>)], byte: u8) {
        let mut buffer = Vec::new();
        for (_, output) in stream {
            buffer.extend(output);
        }
        test_output(buffer, byte);
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

    let streams = Arc::new(Mutex::new(Vec::new()));
    let create_filter_fn = |stderr| {
        let streams = Arc::clone(&streams);
        move |buffer: &[_]| {
            streams.lock().unwrap().push((stderr, buffer.to_owned()));
            Ok(true)
        }
    };
    let output = process
        .controlled_with_output()
        .time_limit(LONG_TIME_LIMIT)
        .strict_errors()
        .stdout_filter(create_filter_fn(false))
        .stderr_filter(create_filter_fn(true))
        .wait()?
        .expect("process timed out");

    assert_eq!(Some(0), output.status.code());

    test_output(output.stdout, b'a');
    test_output(output.stderr, b'b');

    let mut streams = streams.lock().unwrap();
    let unordered_streams = streams.clone();
    streams.sort();
    assert_ne!(*streams, unordered_streams);
    assert!(unordered_streams.iter().rev().ne(&*streams));

    let (stdout, stderr) = streams
        .iter()
        .position(|(x, _)| x == &true)
        .map(|x| streams.split_at(x))
        .expect("missing stderr");
    assert!(stdout.len() >= 2);
    assert!(stderr.len() >= 2);

    test_stream_output(stdout, b'a');
    test_stream_output(stderr, b'b');

    Ok(())
}
