use std::io::Result as IoResult;
use std::sync::mpsc;
use std::thread::Builder as ThreadBuilder;
use std::time::Duration;

pub(crate) fn run_with_timeout<TReturn>(
    get_result_fn: impl 'static + FnOnce() -> TReturn + Send,
    time_limit: Duration,
) -> IoResult<Option<TReturn>>
where
    TReturn: 'static + Send,
{
    let (result_sender, result_receiver) = mpsc::channel();
    let _ = ThreadBuilder::new()
        .spawn(move || result_sender.send(get_result_fn()))?;

    Ok(result_receiver.recv_timeout(time_limit).ok())
}
