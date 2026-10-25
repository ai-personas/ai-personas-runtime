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

fn stop_event(event: &Value, request: &PrepareRequest) -> bool {
    (event["work"] == request.work
        && matches!(event["kind"].as_str(), Some("pause" | "closeout")))
        || (event["persona"] == request.persona
            && event["kind"] == "persona_lifecycle_control"
            && matches!(event["notice"]["action"].as_str(), Some("pause" | "quarantine")))
}

fn stop_receipt(request: &PrepareRequest, revision: u64, phase: &w::Phase, explicit: bool) -> Value {
    json!({"kind":"runtime_stop_observed", "work":request.work,
        "persona":request.persona, "runtime_revision":revision,
        "explicit_stop_observed":explicit, "phase":phase,
        "local_cancellation_requested":true, "remote_stop_confirmed":false})
}

/// Observe the same controls before dispatch, including a stop followed by
/// resumption while this request was being prepared. It grants no new route.
pub(super) fn state_stop(
    state: &State, revision: u64, event_cursor: usize, request: &PrepareRequest,
) -> Result<Option<Value>> {
    require(event_cursor <= state.operator_events.len(), "transport control history was shortened")?;
    let phase = &state.board.work(&w::WorkId(request.work.clone()))?.phase;
    let explicit = lifecycle::reason(state, &request.persona).is_some()
        || state.operator_events[event_cursor..].iter().any(|event| stop_event(event, request));
    Ok((explicit || !matches!(phase, w::Phase::Open))
        .then(|| stop_receipt(request, revision, phase, explicit)))
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
    let explicit_stop = events[*event_cursor..].iter().any(|event| stop_event(event, request));
    *event_cursor = events.len();
    if explicit_stop || !matches!(phase, w::Phase::Open) {
        Ok(Some(stop_receipt(request, object.revision, &phase, explicit_stop)))
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
