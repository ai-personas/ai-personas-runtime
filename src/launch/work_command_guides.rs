//! Explicit installation of native supporting guides into an existing launch.
//! Fresh native setup installs the same bounded files before any persona work.
use super::*;

const KIND: &str = "native-work-guides-installation-v1";

fn deserialize_exact_reference<'de, D>(
    deserializer: D,
) -> std::result::Result<s::ObjectRef, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactReference {
        id: String,
        revision: u64,
        sha256: String,
    }
    let reference = ExactReference::deserialize(deserializer)?;
    Ok(s::ObjectRef {
        id: reference.id,
        revision: reference.revision,
        sha256: reference.sha256,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub operator: String,
    #[serde(deserialize_with = "deserialize_exact_reference")]
    pub expected_runtime: s::ObjectRef,
    #[serde(deserialize_with = "deserialize_exact_reference")]
    pub expected_launch: s::ObjectRef,
    pub authorization_reference: String,
    #[serde(default)]
    pub authorize_installation: bool,
}
impl Request {
    pub fn validate(&self, operator: &str) -> Result<()> {
        identity(&self.id)?;
        identity(&self.operator)?;
        require(
            self.operator == operator,
            "native-guide installation operator mismatch",
        )?;
        text(&self.authorization_reference, 4096)?;
        require(
            !self.authorization_reference.trim().is_empty(),
            "installation authorization reference required",
        )?;
        for (reference, id) in [
            (&self.expected_runtime, STATE_ID),
            (&self.expected_launch, ID),
        ] {
            require(
                reference.id == id
                    && reference.revision > 0
                    && reference.sha256.len() == 64
                    && reference
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "native-guide installation requires exact complete state references",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    request_sha256: String,
    request: Request,
    installed_runtime: s::ObjectRef,
    installed_launch: s::ObjectRef,
    catalog_sha256: String,
    at_unix: i64,
}

fn authorize(state: &State, saved: &Saved, operator: &str) -> Result<()> {
    admission_extension::authorize(state, saved, operator)?;
    admission_extension::validate_saved(state, saved)
}

pub(super) fn fresh_summary(config: &Config, saved: &Saved) -> Result<Value> {
    work_guides::installation_summary(
        &initial_state(config.clone(), &saved.operator),
        &people(saved.setup.cohort)[0],
        POLICY,
    )
}

pub(super) fn install_fresh(app: &mut Runtime, saved: &Saved) -> Result<()> {
    let (head, mut state) = app.load()?;
    require(
        state.work_guides.is_none() && saved.steps == 0 && saved.works.is_empty(),
        "fresh native-guide installation cannot replace continuing work",
    )?;
    work_guides::install(
        &mut state,
        &saved.operator,
        &people(saved.setup.cohort)[0],
        POLICY,
        unix_now()?,
    )?;
    app.save(&head, &state)?;
    Ok(())
}

pub fn snapshot_at(root: &Path, operator: &str) -> Result<Value> {
    identity(operator)?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let (head, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    authorize(&state, &saved, operator)?;
    Ok(
        json!({"operator":operator,"expected_runtime":head.exact_ref()?,
        "expected_launch":launch.exact_ref()?,
        "current_profile":if state.work_guides.is_some() {"selected_native_work_guides_v1"} else {"native_work_guides_not_installed"},
        "proposed":work_guides::installation_summary(&state, &people(saved.setup.cohort)[0], POLICY)?,
        "inference_performed":false,"capacity_reserved":false,
        "scope":"Installation changes future ordinary command-definition supply. It adds native supporting files within existing storage, preserves current selections and all grants/limits/spending, and starts no work or model call."}),
    )
}

fn outcome(receipt: &Receipt, applied: bool, replayed: bool) -> Value {
    json!({"receipt":receipt,"applied":applied,"replayed":replayed,
    "receipt_retained":applied || replayed,
    "inference_performed":false,"resources_added":false,"budget_reset":false,
    "selections_changed":false,"work_created":false,
    "scope":if applied || replayed {
        "Historical native-guide installation receipt. Replay restores no source, permission, model call or resource; inspect current availability separately."
    } else {
        "Installation preview with prospective receipt references; nothing is retained or reserved. Apply still requires the unchanged runtime and launch fences."
    }})
}

pub fn install_at(root: &Path, request: &Request, apply: bool, now: i64) -> Result<Value> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative native-guide installation time")?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let _owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (head, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    authorize(&state, &saved, &request.operator)?;
    let id = format!("{KIND}:{}", s::sha256_bytes(request.id.as_bytes()));
    let hash = digest(request)?;
    if let Some(record) = app.store.read(&id)? {
        require(
            record.kind == KIND && record.revision == 1,
            "installation receipt binding unavailable",
        )?;
        let receipt: Receipt = serde_json::from_value(record.body)?;
        require(
            receipt.version == 1
                && receipt.request_sha256 == hash
                && digest(&receipt.request)? == hash,
            "installation identity conflicts with its retained request",
        )?;
        return Ok(outcome(&receipt, false, true));
    }
    require(
        head.exact_ref()? == request.expected_runtime
            && launch.exact_ref()? == request.expected_launch,
        "native-guide installation snapshot changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks native-guide installation",
    )?;
    app.store.require_quiescent_invocations()?;
    let mut next = state.clone();
    let owner = &people(saved.setup.cohort)[0];
    work_guides::install(&mut next, &request.operator, owner, POLICY, now)?;
    let runtime = state_object(&head, &next)?;
    let next_launch = s::StoredObject {
        revision: increment(launch.revision)?,
        ..launch.clone()
    };
    let receipt = Receipt {
        version: 1,
        request_sha256: hash,
        request: request.clone(),
        installed_runtime: runtime.exact_ref()?,
        installed_launch: next_launch.exact_ref()?,
        catalog_sha256: digest(
            next.work_guides
                .as_ref()
                .ok_or_else(|| held("native guide catalog absent"))?,
        )?,
        at_unix: now,
    };
    let receipt_object = s::StoredObject {
        id,
        revision: 1,
        kind: KIND.into(),
        body: serde_json::to_value(&receipt)?,
    };
    if apply {
        app.store
            .compare_and_swap_batch_requiring_quiescent_invocations(vec![
                s::Change {
                    expected_revision: Some(head.revision),
                    object: runtime,
                    depends_on: Vec::new(),
                },
                s::Change {
                    expected_revision: Some(launch.revision),
                    object: next_launch,
                    depends_on: Vec::new(),
                },
                s::Change {
                    expected_revision: None,
                    object: receipt_object,
                    depends_on: Vec::new(),
                },
            ])?;
    }
    let mut result = outcome(&receipt, apply, false);
    result["installation"] = work_guides::installation_summary(&next, owner, POLICY)?;
    if !apply {
        result["preview_only"] = json!(true);
        result["receipt_retained"] = json!(false);
        result["qualified_guides"] = json!(next
            .work_guides
            .as_ref()
            .unwrap()
            .guides
            .iter()
            .map(|(command, reference)| -> Result<Value> {
                let file = &record(&next, &file_reference(reference)?)?.record;
                Ok(
                    json!({"command":command,"reference":reference,"author":file.author,
                    "path":file.path,"policy":file.policy,"text":std::str::from_utf8(&file.bytes)
                        .map_err(|_|invalid("native guide UTF-8 unavailable"))?}),
                )
            })
            .collect::<Result<Vec<_>>>()?);
    }
    Ok(result)
}
