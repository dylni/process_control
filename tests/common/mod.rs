#![allow(dead_code)]
#![allow(unused_macros)]
#![warn(unsafe_op_in_unsafe_fn)]

use std::process::Command;
use std::time::Duration;

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
pub(crate) mod imp;

pub(crate) const SHORT_TIME_LIMIT: Duration = Duration::from_secs(2);

pub(crate) const LONG_TIME_LIMIT: Duration = Duration::from_secs(5);

#[cfg(process_control_memory_limit)]
pub(crate) const MEMORY_LIMIT: usize = 104_857_600;

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

#[cfg_attr(not(process_control_memory_limit), allow(unused_macro_rules))]
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
        use $crate::common::imp::Handle;

        let mut handle;
        test!(
            @$type
            {
                let process = $command.spawn()?;
                handle = Handle::new(&process)?;
                process
            }.$method(),
            $limit,
            handle,
            $($token)*
        );
    }};
    ( @memory_limit $control:expr , $limit:expr , $($token:tt)* ) => {{
        use $crate::common::LONG_TIME_LIMIT;

        test!(@strict_errors $control.memory_limit($limit), $($token)*);
        test!(
            @strict_errors
            $control.memory_limit($limit).time_limit(LONG_TIME_LIMIT),
            $($token)*
        );
    }};
    ( @time_limit $control:expr , $limit:expr , $($token:tt)* ) => {{
        test!(@strict_errors $control.time_limit($limit), $($token)*);
        #[cfg(process_control_memory_limit)]
        {
            use $crate::common::MEMORY_LIMIT;

            test!(
                @strict_errors
                $control.time_limit($limit).memory_limit(MEMORY_LIMIT),
                $($token)*
            );
        }
    }};
    ( @strict_errors $control:expr , $($token:tt)* ) => {{
        test!($control, $($token)*);
        test!($control.strict_errors(), $($token)*);
    }};
    ( $control:expr , $handle:expr , terminating: true, $($token:tt)* ) => {
        test!(
            $control.terminate_for_timeout(),
            $handle,
            terminating: false,
            $($token)*
        )
    };
    (
        $control:expr ,
        $handle:expr ,
        terminating: false ,
        expected_result: $expected_result:pat ,
        running: $running:expr ,
    ) => {{
        assert_matches!(
            $control.wait()?.map(|x| ExitStatus::from(x).code()),
            $expected_result,
        );

        let running = $running;
        if running {
            thread::sleep(SHORT_TIME_LIMIT);
        }
        assert_eq!(running, unsafe { $handle.is_running()? });
    }};
}

pub(crate) fn create_time_limit_command(seconds: Duration) -> Command {
    let whole_seconds = seconds.as_secs();
    assert_eq!(seconds, Duration::from_secs(whole_seconds));

    let mut command = Command::new("perl");
    let _ = command
        .arg("-e")
        .arg("sleep $ARGV[0]")
        .arg("--")
        .arg(whole_seconds.to_string());
    command
}
