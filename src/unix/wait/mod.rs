use std::thread;
use std::time::Duration;

use crate::WaitResult;

macro_rules! check_result {
    ( $result:expr ) => {{
        use libc::EINTR;

        // https://github.com/rust-lang/rust/blob/49c68bd53f90e375bfb3cbba8c1c67a9e0adb9c0/src/libstd/sys/unix/process/process_unix.rs#L432-L441
        match $result {
            Ok(result) => result,
            Err(error) => {
                if error.raw_os_error() != Some(EINTR) {
                    break Err(error);
                }
                continue;
            }
        }
    }};
}

#[cfg_attr(process_control_unix_waitid, path = "waitid.rs")]
#[cfg_attr(not(process_control_unix_waitid), path = "common.rs")]
mod imp;
pub(super) use imp::wait;

pub(super) fn run_with_time_limit<F, R>(
    run_fn: F,
    time_limit: Option<Duration>,
) -> WaitResult<R>
where
    F: 'static + FnOnce() -> R + Send,
    R: 'static + Send,
{
    let time_limit = if let Some(time_limit) = time_limit {
        time_limit
    } else {
        return Ok(Some(run_fn()));
    };

    let (result_sender, result_receiver) = {
        #[cfg(feature = "crossbeam-channel")]
        {
            crossbeam_channel::bounded(0)
        }
        #[cfg(not(feature = "crossbeam-channel"))]
        {
            use std::sync::mpsc;

            mpsc::channel()
        }
    };

    thread::Builder::new()
        .spawn(move || result_sender.send(run_fn()))
        .map(|_| result_receiver.recv_timeout(time_limit).ok())
}
