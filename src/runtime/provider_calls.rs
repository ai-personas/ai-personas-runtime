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
    pub runtime_control_observation: Option<Value>,
    pub outcome: std::result::Result<provider::ProviderOutput, provider::ProviderError>,
}

fn observe_stop(
    monitor: &mut s::RuntimeControlMonitor,
    event_cursor: &mut usize,
    request: &PrepareRequest,
) -> Result<Option<Value>> {
    let Some(object) = monitor.changed()? else {
        return Ok(None);
    };
    require(
        object.kind == "runtime-state" && object.body["format"] == 1,
        "transport control state unavailable",
    )?;
    let phase: w::Phase =
        serde_json::from_value(object.body["board"]["works"][&request.work]["phase"].clone())?;
    let events = object.body["operator_events"]
        .as_array()
        .ok_or_else(|| held("transport control history unavailable"))?;
    require(
        *event_cursor <= events.len(),
        "transport control history was shortened",
    )?;
    let explicit_stop = events[*event_cursor..].iter().any(|event| {
        (event["work"] == request.work
            && matches!(event["kind"].as_str(), Some("pause" | "closeout")))
            || (event["persona"] == request.persona
                && event["kind"] == "persona_lifecycle_control"
                && matches!(
                    event["notice"]["action"].as_str(),
                    Some("pause" | "quarantine")
                ))
    });
    *event_cursor = events.len();
    if explicit_stop || !matches!(phase, w::Phase::Open) {
        Ok(Some(
            json!({"kind":"runtime_stop_observed", "work":request.work,
            "persona":request.persona, "runtime_revision":object.revision,
            "explicit_stop_observed":explicit_stop, "phase":phase,
            "local_cancellation_requested":true, "remote_stop_confirmed":false}),
        ))
    } else {
        Ok(None)
    }
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
    mut monitor: s::RuntimeControlMonitor,
    mut event_cursor: usize,
    mut transport: BoxFuture<
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
            let mut runtime_control_observation = None;
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut check = || {
                let observation = observe_stop(&mut monitor, &mut event_cursor, &prepared.request)
                    .unwrap_or_else(|_| {
                        Some(json!({"kind":"runtime_control_monitor_unavailable",
                        "local_cancellation_requested":true,"remote_stop_confirmed":false}))
                    });
                if observation.is_some() {
                    cancellation.cancel();
                }
                observation
            };
            let outcome = loop {
                tokio::select! {
                    outcome = &mut transport => {
                        if runtime_control_observation.is_none() {
                            runtime_control_observation = check();
                        }
                        break outcome;
                    }
                    _ = interval.tick(), if runtime_control_observation.is_none() => {
                        runtime_control_observation = check();
                    }
                }
            };
            guard.disarm();
            Completion {
                prepared,
                audit,
                intent,
                cancellation,
                runtime_control_observation,
                outcome,
            }
        }),
    }
}
