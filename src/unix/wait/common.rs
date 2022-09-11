use std::io;
use std::mem;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "parking_lot")]
use parking_lot as sync;
#[cfg(not(feature = "parking_lot"))]
use std::sync;
use sync::Mutex;

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

struct MutexGuard<'a, T> {
    guard: ManuallyDrop<sync::MutexGuard<'a, T>>,
    #[cfg(feature = "parking_lot")]
    fair: bool,
}

impl<'a, T> MutexGuard<'a, T> {
    #[cfg_attr(not(feature = "parking_lot"), allow(unused_variables))]
    fn lock(mutex: &'a Mutex<T>, fair: bool) -> Self {
        let guard = mutex.lock();
        #[cfg(not(feature = "parking_lot"))]
        let guard = guard.unwrap();
        Self {
            guard: ManuallyDrop::new(guard),
            #[cfg(feature = "parking_lot")]
            fair,
        }
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        #[cfg_attr(not(feature = "parking_lot"), allow(unused_variables))]
        let guard = unsafe { ManuallyDrop::take(&mut self.guard) };
        #[cfg(feature = "parking_lot")]
        if self.fair {
            sync::MutexGuard::unlock_fair(guard);
        }
    }
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
        let _ = MutexGuard::lock(&process, false).take();
    });

    let process = Arc::clone(&process);
    super::run_with_time_limit(
        move || {
            let mut signals = Signals::new([SIGCHLD])?;
            loop {
                if let Some(process) = &mut *MutexGuard::lock(&process, true) {
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
