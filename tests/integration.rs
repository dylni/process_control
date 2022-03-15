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

macro_rules! assert_matches {
    ( $result:expr , $expected_result:pat $(,)? ) => {{
        let result = $result;
        if !matches!(result, $expected_result) {
            panic!(
                "assertion failed: `(left matches right)`
  left: `{:?}`
 right: `{:?}`",
                result,
                stringify!($expected_result),
            );
        }
    }};
}

macro_rules! if_memory_limit {
    ( $($item:item)+ ) => {
        $(
            #[cfg(any(
                target_os = "android",
                all(
                    target_os = "linux",
                    any(target_env = "gnu", target_env = "musl"),
                ),
                windows,
            ))]
            $item
        )+
    };
}

const SHORT_TIME_LIMIT: Duration = Duration::from_secs(2);

const LONG_TIME_LIMIT: Duration = Duration::from_secs(5);

if_memory_limit! {
    const MEMORY_LIMIT: usize = 104_857_600;
}

fn create_time_limit_command(seconds: Duration) -> Command {
    assert_eq!(0, seconds.subsec_millis());

    let mut command = Command::new("perl");
    let _ = command
        .arg("-e")
        .arg("sleep $ARGV[0]")
        .arg("--")
        .arg(seconds.as_secs().to_string());
    command
}

if_memory_limit! {
    fn create_memory_limit_command(bytes: usize) -> Command {
        let mut command = Command::new("perl");
        let _ = command
            .arg("-e")
            .arg("my $bytes = 'a' x $ARGV[0]; print $bytes; sleep $ARGV[1]")
            .arg("--")
            .arg(bytes.to_string())
            .arg(SHORT_TIME_LIMIT.as_secs().to_string())
            .stderr(Stdio::null())
            .stdout(Stdio::null());
        command
    }
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
    ( @memory_limit $control:expr , $limit:expr , $($token:tt)* ) => {{
        test!(@strict_errors $control.memory_limit($limit), $($token)*);
        test!(
            @strict_errors
            $control.memory_limit($limit).time_limit(LONG_TIME_LIMIT),
            $($token)*
        );
    }};
    ( @time_limit $control:expr , $limit:expr , $($token:tt)* ) => {{
        test!(@strict_errors $control.time_limit($limit), $($token)*);
        #[cfg(any(
            target_os = "android",
            all(
                target_os = "linux",
                any(target_env = "gnu", target_env = "musl"),
            ),
            windows,
        ))]
        test!(
            @strict_errors
            $control.time_limit($limit).memory_limit(MEMORY_LIMIT),
            $($token)*
        );
    }};
    ( @strict_errors $control:expr , $($token:tt)* ) => {{
        test!($control, $($token)*);
        test!($control.strict_errors(), $($token)*);
    }};
    ( $control:expr , $terminator:expr , terminating: true, $($token:tt)* ) =>
    {
        test!(
            $control.terminate_for_timeout(),
            $terminator,
            terminating: false,
            $($token)*
        )
    };
    (
        $control:expr ,
        $terminator:expr ,
        terminating: false ,
        expected_result: $expected_result:pat ,
        run: | $terminator_var:ident | $body:expr ,
    ) => {{
        assert_matches!(
            $control.wait()?.map(|x| ExitStatus::from(x).code()),
            $expected_result,
        );

        let $terminator_var = &$terminator;
        let _: () = $body;
    }};
}

#[test]
fn test_terminate() -> io::Result<()> {
    let mut process = create_time_limit_command(LONG_TIME_LIMIT).spawn()?;
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
        command: create_time_limit_command(SHORT_TIME_LIMIT),
        time_limit: LONG_TIME_LIMIT,
        terminating: false,
        expected_result: Some(Some(0)),
        run: |terminator| unsafe { assert_not_found(terminator) },
    )
}

#[test]
fn test_time_limit_expired() -> io::Result<()> {
    test!(
        command: create_time_limit_command(LONG_TIME_LIMIT),
        time_limit: SHORT_TIME_LIMIT,
        terminating: false,
        expected_result: None,
        run: |terminator| {
            thread::sleep(SHORT_TIME_LIMIT);
            unsafe { terminator.terminate() }?;
        },
    )
}

#[test]
fn test_terminating_time_limit() -> io::Result<()> {
    test!(
        command: create_time_limit_command(SHORT_TIME_LIMIT),
        time_limit: LONG_TIME_LIMIT,
        terminating: true,
        expected_result: Some(Some(0)),
        run: |terminator| unsafe { assert_not_found(terminator) },
    )
}

#[test]
fn test_terminating_time_limit_expired() -> io::Result<()> {
    test!(
        command: create_time_limit_command(LONG_TIME_LIMIT),
        time_limit: SHORT_TIME_LIMIT,
        terminating: true,
        expected_result: None,
        run: |terminator| unsafe { assert_not_found(terminator) },
    )
}

if_memory_limit! {
    #[test]
    fn test_memory_limit() -> io::Result<()> {
        test!(
            command: create_memory_limit_command(MEMORY_LIMIT),
            memory_limit: 2 * MEMORY_LIMIT,
            terminating: false,
            expected_result: Some(Some(0)),
            run: |terminator| unsafe { assert_not_found(terminator) },
        )
    }

    #[test]
    fn test_memory_limit_exceeded() -> io::Result<()> {
        test!(
            command: create_memory_limit_command(MEMORY_LIMIT),
            memory_limit: MEMORY_LIMIT,
            terminating: false,
            expected_result: Some(Some(1)),
            run: |terminator| unsafe { assert_not_found(terminator) },
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
            Some(127) | None
        };
    }

    #[test]
    fn test_memory_limit_0() -> io::Result<()> {
        test!(
            command: create_memory_limit_command(MEMORY_LIMIT),
            memory_limit: 0,
            terminating: false,
            expected_result: Some(memory_limit_0_result!()),
            run: |terminator| unsafe { assert_not_found(terminator) },
        )
    }

    #[test]
    fn test_memory_limit_1() -> io::Result<()> {
        test!(
            command: create_memory_limit_command(MEMORY_LIMIT),
            memory_limit: 1,
            terminating: false,
            expected_result: Some(memory_limit_0_result!()),
            run: |terminator| unsafe { assert_not_found(terminator) },
        )
    }
}

#[test]
fn test_stdin() -> io::Result<()> {
    let mut command = Command::new("perl");
    let _ = command.stdin(Stdio::piped());

    test!(
        command: command,
        time_limit: LONG_TIME_LIMIT,
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

    return Ok(());

    #[track_caller]
    fn test_output(output: Vec<u8>, byte: u8) {
        assert_eq!(OUTPUT_LENGTH, output.len());
        assert!(output.into_iter().all(|x| x == byte));
    }
}
