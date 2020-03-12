use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use process_control::ChildExt;
use process_control::Terminator;

const ONE_SECOND: Duration = Duration::from_secs(1);

const FIVE_SECONDS: Duration = Duration::from_secs(5);

fn create_process(running_time: Option<Duration>) -> IoResult<Child> {
    Command::new("perl")
        .arg("-e")
        .arg("sleep $ARGV[0]")
        .arg("--")
        .arg(running_time.unwrap_or(FIVE_SECONDS).as_secs().to_string())
        .spawn()
}

fn create_stdin_process() -> IoResult<Child> {
    Command::new("perl").stdin(Stdio::piped()).spawn()
}

fn assert_terminated(mut process: Child) -> IoResult<()> {
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

fn assert_not_found(terminator: &Terminator) {
    assert_eq!(Err(IoErrorKind::NotFound), unsafe {
        terminator.terminate().map_err(|x| x.kind())
    });
}

#[test]
fn test_terminate() -> IoResult<()> {
    let mut process = create_process(None)?;
    let terminator = process.terminator()?;

    unsafe { terminator.terminate()? }

    assert_eq!(None, process.try_wait()?.and_then(|x| x.code()));
    assert_terminated(process)?;

    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_with_timeout() -> IoResult<()> {
    let mut process = create_process(Some(ONE_SECOND))?;
    let terminator = process.terminator()?;

    assert_eq!(
        Some(Some(0)),
        process
            .with_timeout(FIVE_SECONDS)
            .strict_errors()
            .wait()?
            .map(|x| x.code()),
    );
    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_with_timeout_expired() -> IoResult<()> {
    let mut process = create_process(None)?;
    let terminator = process.terminator()?;

    assert_eq!(
        None,
        process.with_timeout(ONE_SECOND).strict_errors().wait()?,
    );
    thread::sleep(ONE_SECOND);
    unsafe { terminator.terminate() }
}

#[test]
fn test_wait_for_output_with_timeout() -> IoResult<()> {
    let process = create_process(Some(ONE_SECOND))?;
    let terminator = process.terminator()?;

    assert_eq!(
        Some(Some(0)),
        process
            .with_output_timeout(FIVE_SECONDS)
            .strict_errors()
            .wait()?
            .map(|x| x.status.code()),
    );
    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_for_output_with_timeout_expired() -> IoResult<()> {
    let process = create_process(None)?;
    let terminator = process.terminator()?;

    assert_eq!(
        None,
        process
            .with_output_timeout(ONE_SECOND)
            .strict_errors()
            .wait()?,
    );
    thread::sleep(ONE_SECOND);
    unsafe { terminator.terminate() }
}

#[test]
fn test_wait_with_terminating_timeout() -> IoResult<()> {
    let mut process = create_process(Some(ONE_SECOND))?;
    let terminator = process.terminator()?;

    assert_eq!(
        Some(Some(0)),
        process
            .with_timeout(FIVE_SECONDS)
            .strict_errors()
            .terminating()
            .wait()?
            .map(|x| x.code()),
    );
    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_with_terminating_timeout_expired() -> IoResult<()> {
    let mut process = create_process(None)?;
    let terminator = process.terminator()?;

    assert_eq!(
        None,
        process
            .with_timeout(ONE_SECOND)
            .strict_errors()
            .terminating()
            .wait()?,
    );
    thread::sleep(ONE_SECOND);
    assert_terminated(process)?;

    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_for_output_with_terminating_timeout() -> IoResult<()> {
    let process = create_process(Some(ONE_SECOND))?;
    let terminator = process.terminator()?;

    assert_eq!(
        Some(Some(0)),
        process
            .with_output_timeout(FIVE_SECONDS)
            .strict_errors()
            .terminating()
            .wait()?
            .map(|x| x.status.code()),
    );
    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_for_output_with_terminating_timeout_expired() -> IoResult<()> {
    let process = create_process(None)?;
    let terminator = process.terminator()?;

    assert_eq!(
        None,
        process
            .with_output_timeout(ONE_SECOND)
            .strict_errors()
            .terminating()
            .wait()?,
    );
    thread::sleep(ONE_SECOND);
    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_with_stdin() -> IoResult<()> {
    let mut process = create_stdin_process()?;
    let terminator = process.terminator()?;

    assert_eq!(
        Some(Some(0)),
        process
            .with_timeout(FIVE_SECONDS)
            .strict_errors()
            .wait()?
            .map(|x| x.code()),
    );
    assert_not_found(&terminator);

    Ok(())
}

#[test]
fn test_wait_for_output_with_stdin() -> IoResult<()> {
    let process = create_stdin_process()?;
    let terminator = process.terminator()?;

    assert_eq!(
        Some(Some(0)),
        process
            .with_output_timeout(FIVE_SECONDS)
            .strict_errors()
            .wait()?
            .map(|x| x.status.code()),
    );
    assert_not_found(&terminator);

    Ok(())
}
