use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::os::windows::process::ExitStatusExt;
use std::process;

use super::EXIT_SUCCESS;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExitStatus(u32);

impl ExitStatus {
    pub(super) const fn new(exit_code: u32) -> Self {
        Self(exit_code)
    }

    pub(crate) fn success(self) -> bool {
        self.0 == EXIT_SUCCESS
    }

    pub(crate) fn code(self) -> Option<u32> {
        Some(self.0)
    }
}

impl Display for ExitStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        process::ExitStatus::from_raw(self.0).fmt(f)
    }
}

impl From<process::ExitStatus> for ExitStatus {
    fn from(value: process::ExitStatus) -> Self {
        Self(value.code().expect("process has no exit code") as u32)
    }
}
