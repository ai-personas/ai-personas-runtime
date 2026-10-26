//! Keep admitted transports progressing while the coordinator does synchronous work.
use futures_util::future::BoxFuture;

struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The waiting future owns the task even before its first poll. Dropping it
/// aborts that task and drops the transport's local cancellation guard. Normal
/// coordination still awaits every completion before releasing execution ownership.
pub(super) fn spawn<T: Send + 'static>(future: BoxFuture<'static, T>) -> BoxFuture<'static, T> {
    let task = tokio::spawn(future);
    let owned = AbortOnDrop(task.abort_handle());
    Box::pin(async move {
        let result = task.await;
        drop(owned);
        match result {
            Ok(value) => value,
            // A failed task supplies no transport receipt. Preserve panic/unknown
            // semantics instead of inventing a provider failure or completion.
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(_) => panic!("owned transport task canceled before completion"),
        }
    })
}
