use std::io;
use std::mem;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use signal_hook::consts::SIGCHLD;
use signal_hook::iterator::Signals;

use crate::WaitResult;

use super::super::ExitStatus;
use super::super::Handle;

unsafe fn transmute_lifetime_mut<'a, T>(value: &mut T) -> &'a mut T
where
    T: ?Sized,
{
    unsafe { mem::transmute(value) }
}

fn run_on_drop<F>(drop_fn: F) -> impl Drop
where
    F: FnOnce(),
{
    struct Dropper<F>(ManuallyDrop<F>)
    where
        F: FnOnce();

    impl<F> Drop for Dropper<F>
    where
        F: FnOnce(),
    {
        fn drop(&mut self) {
            (unsafe { ManuallyDrop::take(&mut self.0) })();
        }
    }

    Dropper(ManuallyDrop::new(drop_fn))
}

pub(in super::super) fn wait(
    handle: &mut Handle<'_>,
    time_limit: Option<Duration>,
) -> WaitResult<ExitStatus> {
    // SAFETY: The process is removed by [_guard] before this function returns.
    let process = Arc::new(Mutex::new(Some(unsafe {
        transmute_lifetime_mut(handle.process)
    })));
    let _guard = run_on_drop(|| {
        let _ = process.lock().unwrap().take();
    });

    let process = Arc::clone(&process);
    super::run_with_time_limit(
        move || {
            let mut signals = Signals::new([SIGCHLD])?;
            loop {
                if let Some(process) = &mut *process.lock().unwrap() {
                    let result = check_result!(process.try_wait());
                    if let Some(result) = result {
                        break Ok(result.into());
                    }
                } else {
                    break Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Process timed out",
                    ));
                };
                while signals.wait().count() == 0 {}
            }
        },
        time_limit,
    )?
    .transpose()
}
