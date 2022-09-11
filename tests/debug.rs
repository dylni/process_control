use std::process;

#[cfg(unix)]
use std::os::unix as os;
#[cfg(windows)]
use std::os::windows as os;

use os::process::ExitStatusExt;

use process_control::ExitStatus;
use process_control::Output;

fn test(result: &str, string: &[u8]) {
    let exit_status: ExitStatus = process::ExitStatus::from_raw(0).into();
    assert_eq!(
        format!(
            "Output {{ status: {:?}, stdout: {}, stderr: {} }}",
            exit_status, result, result,
        ),
        format!(
            "{:?}",
            Output {
                status: exit_status,
                stdout: string.to_owned(),
                stderr: string.to_owned(),
            },
        ),
    );
}

#[test]
fn test_empty() {
    test("\"\"", b"");
}

#[test]
fn test_invalid() {
    test(
        "\"\\xF1foo\\xF1\\x80bar\\xF1\\x80\\x80baz\"",
        b"\xF1foo\xF1\x80bar\xF1\x80\x80baz",
    );
}

#[test]
fn test_quote() {
    test("\"foo\\\"bar\"", b"foo\"bar");
}
