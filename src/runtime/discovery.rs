//! Explicit one-page discovery with bounded, host-retained continuation.
use super::*;

const MAX_RETAINED_CURSORS: usize = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    observation: String,
    page: f::RetainedPageCursor,
}

/// Ordinary adoption consumes supplied observations before navigation. Preserve
/// only pending observations and supplied handles explicitly requested by this
/// response, so abandoned handles cannot occupy the bounded map indefinitely.
/// This is staged with adoption; held, character-fenced and repair responses
/// never commit this cleanup.
pub(super) fn retain_for_response(focus: &mut Focus, prepared: &Prepared, decision: &p::Decision) {
    let requested: BTreeSet<&str> = decision
        .operations
        .iter()
        .filter_map(|operation| match operation {
            p::Operation::Discover {
                cursor: Some(handle),
                ..
            } => Some(handle.as_str()),
            _ => None,
        })
        .collect();
    focus.discovery_cursors.retain(|handle, cursor| {
        focus.pending_observations.contains(&cursor.observation)
            || (requested.contains(handle.as_str())
                && prepared.supplied_observations.contains(&cursor.observation))
    });
}

/// Once ordinary navigation is resolved, only still-pending observations can
/// supply a cursor to a future invocation. This also drops failed/unused handles
/// preserved temporarily for this response, while retaining newly emitted ones.
pub(super) fn retain_pending(focus: &mut Focus) {
    focus
        .discovery_cursors
        .retain(|_, cursor| focus.pending_observations.contains(&cursor.observation));
}

/// True only when an observation was emitted; false means even the fixed
/// fallback could not fit and no corpus view was consulted.
#[allow(clippy::too_many_arguments)] // One admitted operation and its shared navigation allowance.
pub(super) fn execute(
    state: &mut State,
    prepared: &Prepared,
    id: &str,
    request: &f::DiscoveryRequest,
    limit: usize,
    cursor: Option<&str>,
    corpus: &f::Corpus,
    admission: &f::Admission,
    navigation: &mut f::NavigationBudget,
    now: i64,
) -> Result<bool> {
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let observation = composite_id("observation", &[&prepared.request.id, "discovery", id]);
    let next_handle = format!("discovery:{}", digest(&(&prepared.request.id, id))?);
    let retained = cursor.map(|handle| {
        let retained = state.personas[actor].focus[work]
            .discovery_cursors
            .get(handle)?;
        let source = state.observations.get(&retained.observation)?;
        (prepared
            .supplied_observations
            .contains(&retained.observation)
            && source.actor == *actor
            && source.work == *work
            && source
                .exposure
                .as_ref()
                .is_some_and(|e| exposure_permitted(state, e, admission)))
        .then(|| retained.page.restore())
    });

    // Reserve a fixed fallback first. If even that cannot fit, there is no result
    // observation; host accounting still records the attempted navigation call.
    let fallback = json!({"status":"exhausted","coverage_complete":false,"next_cursor":null});
    let fallback_fits =
        navigation.charge_serialized_result(&fallback, prepared.navigation.result_bytes);
    if !fallback_fits {
        let _ = navigation.begin_call();
        return Ok(false);
    }
    // This deliberately over-reserves the wrapper separately from the files
    // page. The nested page is charged by Corpus::discover, including controls.
    let envelope = json!({"request":request,"next_cursor":next_handle,
        "status":"unavailable","error":"cursor_or_discovery_unavailable",
        "coverage_complete":false,"page":null,"continuation_available":false});
    if !navigation.charge_serialized_result(&envelope, prepared.navigation.result_bytes) {
        let _ = navigation.begin_call();
        add_observation(
            state,
            prepared,
            id,
            "discovery",
            fallback,
            Vec::new(),
            Vec::new(),
            now,
        )?;
        return Ok(true);
    }

    let page = match retained {
        Some(None) => navigation.begin_call().and(Err(f::FilesError::StalePage)),
        _ if navigation.remaining().result_bytes < f::DISCOVERY_CONTROL_BYTES => navigation
            .begin_call()
            .and(Err(f::FilesError::NavigationExhausted)),
        retained => corpus.discover(
            request,
            admission,
            f::PageLimits {
                scan_bytes: prepared.navigation.scanned_bytes,
                matches: prepared.navigation.matches,
                result_bytes: navigation.remaining().result_bytes,
                results: limit,
            },
            navigation,
            retained.as_ref().and_then(|r| r.as_ref()),
        ),
    };
    let (payload, consulted) = match page {
        Ok(page) if page.stop == Some(f::DiscoveryStop::StalePage) => (
            json!({"request":request,"status":"unavailable",
            "error":"cursor_or_discovery_unavailable","coverage_complete":false,"next_cursor":null}),
            page.consulted,
        ),
        Ok(page) => {
            let cursors = &mut state
                .personas
                .get_mut(actor)
                .unwrap()
                .focus
                .get_mut(work)
                .unwrap()
                .discovery_cursors;
            if let Some(handle) = cursor {
                cursors.remove(handle);
            }
            let next = page
                .next
                .as_ref()
                .filter(|_| cursors.len() < MAX_RETAINED_CURSORS)
                .map(|next| {
                    cursors.insert(
                        next_handle.clone(),
                        Cursor {
                            observation,
                            page: next.retain(),
                        },
                    );
                    next_handle.clone()
                });
            (
                json!({"request":request, "page":serde_json::from_str::<Value>(&page.rendered_json()?)?,
                "continuation_available":next.is_some(),"next_cursor":next}),
                page.consulted,
            )
        }
        Err(_) => (
            json!({"request":request,"status":"unavailable",
            "error":"cursor_or_discovery_unavailable","coverage_complete":false,"next_cursor":null}),
            Vec::new(),
        ),
    };
    add_observation(
        state,
        prepared,
        id,
        "discovery",
        payload,
        consulted,
        Vec::new(),
        now,
    )?;
    Ok(true)
}
