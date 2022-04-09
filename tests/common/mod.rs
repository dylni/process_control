#![warn(unsafe_op_in_unsafe_fn)]

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
mod imp;

pub(crate) use imp::Handle;
