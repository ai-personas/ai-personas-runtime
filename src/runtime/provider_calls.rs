//! Transport futures carry actual receipts, never a mutable Runtime connection.
use super::*;
use futures_util::future::BoxFuture;

pub(in crate::runtime) struct Pending {
    pub request: PrepareRequest,
    pub completion: BoxFuture<'static, Completion>,
}

pub(in crate::runtime) struct Completion {
    pub prepared: Prepared,
    pub audit: provider::RequestAudit,
    pub intent: provider::ProviderIntent,
    pub cancellation: provider::Cancellation,
    pub response_bytes: usize,
    pub outcome: std::result::Result<provider::ProviderOutput, provider::ProviderError>,
}

struct CancelOnDrop(Option<provider::Cancellation>);
impl CancelOnDrop {
    fn disarm(mut self) {
        self.0.take();
    }
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(cancellation) = &self.0 {
            cancellation.cancel();
        }
    }
}

pub(super) fn pending(
    prepared: Prepared,
    audit: provider::RequestAudit,
    intent: provider::ProviderIntent,
    cancellation: provider::Cancellation,
    response_bytes: usize,
    transport: BoxFuture<
        'static,
        std::result::Result<provider::ProviderOutput, provider::ProviderError>,
    >,
) -> Pending {
    let request = prepared.request.clone();
    // Construct outside the async body so dropping an unpolled future also
    // cancels its local transport. No cancellation proves remote termination.
    let guard = CancelOnDrop(Some(cancellation.clone()));
    Pending {
        request,
        completion: Box::pin(async move {
            let outcome = transport.await;
            guard.disarm();
            Completion {
                prepared,
                audit,
                intent,
                cancellation,
                response_bytes,
                outcome,
            }
        }),
    }
}
