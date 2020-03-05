use std::io::ErrorKind as IoErrorKind;
use std::io::Result as IoResult;
use std::process::Child;
use std::process::Command;
use std::thread;
use std::time::Duration;

use process_control::ProcessTerminator;
use process_control::Terminator;

const ONE_SECOND: Duration = Duration::from_secs(1);

const FIVE_SECONDS: Duration = Duration::from_secs(5);

fn assert_terminated(mut process: Child) -> IoResult<()> {
    assert_eq!(None, process.try_wait()?.and_then(|x| x.code()));

    let exit_status = process.wait()?;
    #[cfg(unix)]
    assert_eq!(
        Some(::libc::SIGKILL),
        ::std::os::unix::process::ExitStatusExt::signal(&exit_status),
    );
    if cfg!(windows) {
        assert_eq!(Some(1), exit_status.code());
    }

    Ok(())
}

fn assert_not_found(process_terminator: &ProcessTerminator) {
    assert_eq!(
        Err(IoErrorKind::NotFound),
        process_terminator.terminate().map_err(|x| x.kind()),
    );
}

fn create_process(running_time: Option<Duration>) -> IoResult<Child> {
    Command::new("perl")
        .arg("-e")
        .arg("sleep $ARGV[0]")
        .arg("--")
        .arg(running_time.unwrap_or(FIVE_SECONDS).as_secs().to_string())
        .spawn()
}

#[test]
fn test_terminate() -> IoResult<()> {
    let process = create_process(None)?;
    let process_terminator = process.terminator();

    process_terminator.terminate()?;
    assert_terminated(process)?;

    assert_not_found(&process_terminator);

    Ok(())
}

#[test]
fn test_terminate_if_necessary() -> IoResult<()> {
    let process = create_process(None)?;
    let process_terminator = process.terminator();

    process_terminator.terminate_if_necessary()?;
    assert_terminated(process)?;

    process_terminator.terminate_if_necessary()?;

    Ok(())
}

#[test]
fn test_wait_with_timeout() -> IoResult<()> {
    let process = create_process(Some(ONE_SECOND))?;
    assert_eq!(
        Some(Some(0)),
        process
            .wait_with_timeout(FIVE_SECONDS)?
            .map(|(x, _)| x.code()),
    );
    Ok(())
}

#[test]
fn test_wait_with_timeout_expired() -> IoResult<()> {
    let process = create_process(None)?;
    let process_terminator = process.terminator();

    assert_eq!(None, process.wait_with_timeout(ONE_SECOND)?.map(|(x, _)| x));
    thread::sleep(ONE_SECOND);
    assert_not_found(&process_terminator);

    Ok(())
}

#[test]
fn test_wait_for_output_with_timeout() -> IoResult<()> {
    let process = create_process(Some(ONE_SECOND))?;
    assert_eq!(
        Some(Some(0)),
        process
            .wait_for_output_with_timeout(FIVE_SECONDS)?
            .map(|x| x.status.code()),
    );
    Ok(())
}

#[test]
fn test_wait_for_output_with_timeout_expired() -> IoResult<()> {
    let process = create_process(None)?;
    let process_terminator = process.terminator();

    assert_eq!(None, process.wait_for_output_with_timeout(ONE_SECOND)?);
    thread::sleep(ONE_SECOND);
    assert_not_found(&process_terminator);

    Ok(())
}
