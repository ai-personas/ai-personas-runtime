//! Explicit local executable fixture. No subprocess, network, model or export.
use ai_personas::{mock_effects as m, protocol as p, store as s};
use serde_json::json;

pub fn bounds() -> m::Bounds {
    m::Bounds {
        response_bytes: 16 * 1024,
        input_bytes: 2048,
        result_bytes: 1024,
        evidence_bytes: 8192,
    }
}

pub fn limits() -> s::Limits {
    s::Limits {
        total: s::Resources {
            attempts: 32,
            input_bytes: 1_000_000,
            output_bytes: 1_000_000,
            navigation_bytes: 16_000_000,
            retained_bytes: 32 * 1024 * 1024,
        },
        protected_closeout: s::Resources::default(),
        max_body_bytes: 64 * 1024,
        max_record_bytes: 128 * 1024,
        max_read_bytes: 128 * 1024,
        max_page_bytes: 512 * 1024,
        max_page_items: 32,
        max_batch_items: 32,
        max_id_bytes: 128,
        max_identity_records: 512,
        max_history_events: 2048,
        busy_timeout_ms: 5000,
    }
}

pub fn rights() -> m::Rights {
    m::Rights {
        submit_fixture: true,
        dispatch: true,
        cancel: true,
        observe: true,
    }
}

pub fn request(id: &str) -> m::MockRequest {
    m::MockRequest {
        invocation_id: format!("fixture-decision-{id}"),
        id: id.into(),
        actor: "fixture-actor".into(),
        work: "fixture-work".into(),
        interface_version: m::INTERFACE_VERSION.into(),
        connection: "fixture-mock-connection".into(),
        request_text: "An explicitly synthetic opaque request.".into(),
        inputs: "Exact fixture inputs: \\n and λ.\n".into(),
    }
}

pub fn response(spec: &m::IntentSpec, invocation: &str, epoch: u64) -> Vec<u8> {
    // Explicit fixture authorship; this is neither an LLM call nor a host-inserted
    // maintenance outcome for an absent/invalid persona response.
    serde_json::to_vec(&json!({
        "version":1, "invocation_id":invocation, "epoch":epoch,
        "work":{"kind":"ongoing","reason":"The fixture records a mock request, not a work-quality judgment.","evidence":[]},
        "fragment_maintenance":{"kind":"no_change","reason":"This fixture proposes no fragment changes."},
        "asset_maintenance":{"kind":"no_change","reason":"This fixture proposes no supporting-resource changes."},
        "next_fragments":{"kind":"preserve","reason":"No persona context change is proposed by this fixture."},
        "next_assets":{"kind":"preserve","reason":"No persona context change is proposed by this fixture."},
        "continuation":{"kind":"stop","reason":"No automatic continuation is requested."},
        "reply":{"kind":"none","reason":"No persona communication is requested."},
        "operations":[spec.operation()]
    })).unwrap()
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = directory.path().join("mock.sqlite3");
    let mut store = s::Store::open(&path, limits())?;
    let authority = s::StoredObject {
        id: "fixture-authority".into(),
        revision: 1,
        kind: "explicit-mock-fixture-authority".into(),
        body: json!({"mock_only":true,"scope":"fixture-actor/fixture-work"}),
    };
    let pins = vec![authority.exact_ref()?];
    store.compare_and_swap(None, authority)?;
    let terminal = {
        let mut mock =
            m::MockHarness::attach(&mut store, "fixture-mock-connection".into(), bounds())?;
        let admission = |epoch| {
            m::HostAdmission::from_trusted_host(
                epoch,
                "fixture-actor".into(),
                "fixture-work".into(),
                pins.clone(),
                rights(),
            )
        };
        let request = request("example");
        let invocation = request.invocation_id.clone();
        let spec = m::IntentSpec::new(request, pins.clone(), 128);
        let bytes = response(&spec, &invocation, mock.epoch()?);
        p::parse_decision(&bytes, bounds().response_bytes)?;
        mock.submit_fixture(&admission(mock.epoch()?), &spec, &bytes)?;
        mock.dispatch(&admission(mock.epoch()?), "example")?;
        mock.simulate_terminal(
            &admission(mock.epoch()?),
            "example",
            m::MockOutcome::Completed,
            "Exact locally simulated terminal bytes.",
        )?
        .terminal
        .ok_or("missing observed mock terminal")?
    };
    drop(store);
    let mut store = s::Store::open(&path, limits())?;
    let retained = store.read_exact(terminal.evidence())?;
    assert_eq!(retained.body["mock_only"], true);
    println!(
        "{}",
        serde_json::to_string(&json!({
            "mock_only":true, "receipt":terminal.receipt(),
            "event_observation":terminal.event_observation()?,
            "native_capability_ready":false, "external_effects":false
        }))?
    );
    Ok(())
}
