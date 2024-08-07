use std::fmt;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::io;
use std::process::ChildStdout;

use crate::imp;
use crate::PipeFilter as Filter;

pub(super) struct FilterWrapper(Box<dyn Filter>);

impl Debug for FilterWrapper {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilterWrapper").finish_non_exhaustive()
    }
}

impl<T> From<T> for FilterWrapper
where
    T: Filter,
{
    #[inline]
    fn from(value: T) -> Self {
        Self(Box::new(value))
    }
}

pub(crate) struct Pipe {
    pub(crate) inner: ChildStdout,
    filter: FilterWrapper,
}

impl Pipe {
    pub(super) fn new(
        pipe: imp::OwnedFd,
        filter: Option<FilterWrapper>,
    ) -> Self {
        Self {
            inner: pipe.into(),
            filter: filter.unwrap_or_else(|| (|_: &_| Ok(true)).into()),
        }
    }

    pub(crate) fn run_filter(
        &mut self,
        buffer: &mut Vec<u8>,
        index: usize,
    ) -> io::Result<()> {
        debug_assert_ne!(index, buffer.len());
        if !(self.filter.0)(&buffer[index..])? {
            buffer.truncate(index);
        }
        Ok(())
    }
}
