use std::mem::MaybeUninit;
use std::time::Duration;

use libc::P_PID;
use libc::WEXITED;
use libc::WNOWAIT;
use libc::WSTOPPED;

use crate::WaitResult;

use super::super::check_syscall;
use super::super::ExitStatus;
use super::super::Handle;

#[allow(clippy::needless_pass_by_ref_mut)]
pub(in super::super) fn wait(
    handle: &mut Handle<'_>,
    time_limit: Option<Duration>,
) -> WaitResult<ExitStatus> {
    let pid = handle.pid.as_id();
    super::run_with_time_limit(
        move || loop {
            let mut process_info = MaybeUninit::uninit();
            check_result!(check_syscall(unsafe {
                libc::waitid(
                    P_PID,
                    pid,
                    process_info.as_mut_ptr(),
                    WEXITED | WNOWAIT | WSTOPPED,
                )
            }));
            break Ok(unsafe { ExitStatus::new(process_info.assume_init()) });
        },
        time_limit,
    )?
    .transpose()
}
