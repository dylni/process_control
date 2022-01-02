use std::io;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use process_control::ChildExt;
use process_control::Control;
use process_control::ExitStatus;
use process_control::Terminator;

const ONE_SECOND: Duration = Duration::from_secs(2);

const FIVE_SECONDS: Duration = Duration::from_secs(5);

fn create_command(running_time: Duration) -> Command {
    let mut command = Command::new("perl");
    let _ = command
        .arg("-e")
        .arg("sleep $ARGV[0]")
        .arg("--")
        .arg(running_time.as_secs().to_string());
    command
}

#[track_caller]
fn assert_terminated(process: &mut Child) -> io::Result<()> {
    let exit_status = process.wait()?;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        use libc::SIGKILL;

        assert_eq!(Some(SIGKILL), exit_status.signal());
    }
    if cfg!(not(unix)) {
        assert_eq!(Some(1), exit_status.code());
    }
    Ok(())
}

#[track_caller]
unsafe fn assert_not_found(terminator: &Terminator) {
    assert_eq!(
        Err(io::ErrorKind::NotFound),
        terminator.terminate().map_err(|x| x.kind()),
    );
}

macro_rules! test {
    ( command: $command:expr , $($token:tt)* ) => {{
        test!(@output $command, controlled, $($token)*);
        test!(@output $command, controlled_with_output, $($token)*);
        Ok(())
    }};
    (
        @output
        $command:expr ,
        $method:ident ,
        $type:ident : $limit:expr ,
        $($token:tt)*
    ) => {{
        let mut terminator;
        test!(
            @$type
            {
                let process = $command.spawn()?;
                terminator = process.terminator()?;
                process
            }.$method(),
            $limit,
            terminator,
            $($token)*
        );
    }};
    ( @time_limit $control:expr , $limit:expr , $($token:tt)* ) => {{
        test!(@strict_errors $control.time_limit($limit), $($token)*);
    }};
    ( @strict_errors $control:expr , $($token:tt)* ) => {{
        test!($control, $($token)*);
        test!($control.strict_errors(), $($token)*);
    }};
    ( $control:expr , $terminator:expr , terminating: true, $($token:tt)* ) =>
    {{
        test!(
            $control.terminate_for_timeout(),
            $terminator,
            terminating: false,
            $($token)*
        );
    }};
    (
        $control:expr ,
        $terminator:expr ,
        terminating: false ,
        expected_result: $expected_result:expr ,
        run: | $terminator_var:ident | $body:expr ,
    ) => {{
        assert_eq!(
            $expected_result,
            $control.wait()?.map(|x| ExitStatus::from(x).code()),
        );

        let $terminator_var = &$terminator;
        let _: () = $body;
    }};
}

#[test]
fn test_terminate() -> io::Result<()> {
    let mut process = create_command(FIVE_SECONDS).spawn()?;
    let terminator = process.terminator()?;

    unsafe {
        terminator.terminate()?;
    }

    assert_eq!(None, process.try_wait()?.and_then(|x| x.code()));
    assert_terminated(&mut process)?;

    unsafe {
        assert_not_found(&terminator);
    }

    Ok(())
}

#[test]
fn test_time_limit() -> io::Result<()> {
    test!(
        command: create_command(ONE_SECOND),
        time_limit: FIVE_SECONDS,
        terminating: false,
        expected_result: Some(Some(0)),
        run: |terminator| unsafe { assert_not_found(terminator) },
    )
}

#[test]
fn test_time_limit_expired() -> io::Result<()> {
    test!(
        command: create_command(FIVE_SECONDS),
        time_limit: ONE_SECOND,
        terminating: false,
        expected_result: None,
        run: |terminator| {
            thread::sleep(ONE_SECOND);
            unsafe { terminator.terminate() }?;
        },
    )
}

#[test]
fn test_terminating_time_limit() -> io::Result<()> {
    test!(
        command: create_command(ONE_SECOND),
        time_limit: FIVE_SECONDS,
        terminating: true,
        expected_result: Some(Some(0)),
        run: |terminator| unsafe { assert_not_found(terminator) },
    )
}

#[test]
fn test_terminating_time_limit_expired() -> io::Result<()> {
    test!(
        command: create_command(FIVE_SECONDS),
        time_limit: ONE_SECOND,
        terminating: true,
        expected_result: None,
        run: |terminator| unsafe { assert_not_found(terminator) },
    )
}

#[test]
fn test_stdin() -> io::Result<()> {
    let mut command = Command::new("perl");
    let _ = command.stdin(Stdio::piped());

    test!(
        command: command,
        time_limit: FIVE_SECONDS,
        terminating: false,
        expected_result: Some(Some(0)),
        run: |terminator| unsafe { assert_not_found(terminator) },
    )
}

#[test]
fn test_large_output() -> io::Result<()> {
    const BUFFER_COUNT: usize = 1024;
    const BUFFER_LENGTH: usize = 1024;
    const OUTPUT_LENGTH: usize = BUFFER_COUNT * BUFFER_LENGTH;

    let process = Command::new("perl")
        .arg("-e")
        .arg(
            r"for (my $i = 0; $i < $ARGV[0]; $i++) {
                print 'a' x $ARGV[1];
                print STDERR 'a' x $ARGV[1];
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
        .time_limit(FIVE_SECONDS)
        .strict_errors()
        .wait()?
        .unwrap();

    assert_eq!(Some(0), output.status.code());

    assert_eq!(OUTPUT_LENGTH, output.stdout.len());
    assert_eq!(OUTPUT_LENGTH, output.stderr.len());

    assert!(output
        .stdout
        .into_iter()
        .chain(output.stderr)
        .all(|x| x == b'a'));

    Ok(())
}
