#![allow(deprecated)]

use std::io;
use std::process::Child;

use process_control::ChildExt;
use process_control::Terminator;

mod common;
use common::LONG_TIME_LIMIT;

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

#[test]
fn test_terminate() -> io::Result<()> {
    let mut process =
        common::create_time_limit_command(LONG_TIME_LIMIT).spawn()?;
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
