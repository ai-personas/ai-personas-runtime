//! Primary-authored current affect, with exact receipts and declared expiry.
//! No host-authored mood, decay, new character, permission, or scheduled wake.
use super::*;

const PROFILE: &str = "persona-transient-affect-v1";
const MAX_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;
const KIND: &str = "affect_update";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    revision: String,
    character_revision: String,
    change: p::AffectChange,
}

pub(super) fn interface(now: i64) -> Result<Value> {
    let maximum = now
        .checked_add(MAX_LIFETIME_SECONDS)
        .ok_or_else(|| invalid("affect expiry overflow"))?;
    Ok(json!({
        "profile":PROFILE,
        "maximum_expires_at_unix":maximum,
        "rule":"Optional affect_update replaces or clears your own current modeled state. Set exactly valence, activation (arousal), and control (perceived control), a stated interpretation, actual supplied observation IDs or [], and an explicit expiry after adoption and no later than maximum_expires_at_unix. Use ordinary words; this is neither felt emotion nor a measurement. Current text is the exact receipt named by transient_affect.observation_id in actual_observations. Expired, unavailable or character-changed state supplies no current affect body. This does not revise stable character or baseline VAD, grant authority, choose a role, or schedule a wake. Do not use contextual state to substitute a new character. Prior accepted receipts remain historical evidence. Fragment and asset maintenance are separate."
    }))
}

pub(crate) struct Projection {
    pub control: Value,
    pub observation: Option<Value>,
    pub exposure: Exposure,
    pub references: Vec<f::Reference>,
    pub qualified_roots: Vec<f::Reference>,
}

impl Projection {
    fn status(status: &str) -> Self {
        Self {
            control: json!({"status":status,"body_supplied":false}),
            observation: None,
            exposure: Exposure::default(),
            references: Vec::new(),
            qualified_roots: Vec::new(),
        }
    }
}

/// The current state's body is an exact immutable observation. Its source and
/// qualification checks are the same as other protected observations, including
/// when this persona later acts in a different work or for a different audience.
pub(crate) fn project(
    state: &State,
    actor: &str,
    a: &f::Admission,
    corpus: &f::Corpus,
) -> Result<Projection> {
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("affect actor absent"))?;
    let Some(id) = &persona.transient_affect else {
        return Ok(Projection::status("none_retained"));
    };
    let observation = state
        .observations
        .get(id)
        .ok_or_else(|| held("affect receipt absent"))?;
    require(
        observation.kind == KIND && observation.actor == actor && observation.invocation.is_some(),
        "affect receipt attribution changed",
    )?;
    let inherited = observation
        .exposure
        .as_ref()
        .ok_or_else(|| held("affect lineage absent"))?;
    if !exposure_permitted(state, inherited, a) {
        return Ok(Projection::status("withheld_unavailable"));
    }
    let record: Record = serde_json::from_value(observation.payload.clone())?;
    let status = if persona.character.as_ref().map(|c| c.revision.as_str())
        != Some(record.character_revision.as_str())
    {
        "character_changed"
    } else {
        match &record.change {
            p::AffectChange::Clear { .. } => "cleared",
            p::AffectChange::Set {
                expires_at_unix, ..
            } if *expires_at_unix <= a.now_unix => "expired",
            p::AffectChange::Set { .. } => "current",
        }
    };
    let mut projected = Projection::status(status);
    if status == "current" {
        let Ok((value, exposure, references)) =
            observation_projection::project(state, observation, a, corpus)
        else {
            return Ok(Projection::status("withheld_unavailable"));
        };
        projected.observation = Some(value);
        projected.exposure = exposure;
        projected.references = references;
        projected.qualified_roots = observation.qualified_roots.clone();
        projected.control["body_supplied"] = json!(true);
    } else {
        // These are permitted historical metadata, never a replacement state.
        projected.exposure = inherited.clone();
    }
    projected.control["observation_id"] = json!(id);
    projected.control["revision"] = json!(record.revision);
    projected.control["accepted_at_unix"] = json!(observation.at_unix);
    if let p::AffectChange::Set {
        expires_at_unix, ..
    } = record.change
    {
        projected.control["expires_at_unix"] = json!(expires_at_unix);
    }
    Ok(projected)
}

pub(super) fn apply(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    now: i64,
) -> Result<()> {
    let Some((id, change)) = decision
        .operations
        .iter()
        .find_map(|operation| match operation {
            p::Operation::AffectUpdate { id, change } => Some((id, change)),
            _ => None,
        })
    else {
        return Ok(());
    };
    require(
        prepared.request.mode == Mode::Ordinary && prepared.character_revision.is_some(),
        "transient affect requires a continuing ordinary persona",
    )?;
    let control = request_context::decode(prepared)?.control;
    require(
        control["affect_interface"] == interface(prepared.at_unix)?,
        "transient affect interface was not supplied",
    )?;
    let actor = &prepared.request.persona;
    let character = state.personas[actor]
        .character
        .as_ref()
        .ok_or_else(|| held("affect requires accepted character"))?;
    require(
        prepared.character_revision.as_deref() == Some(character.revision.as_str()),
        "affect cannot accompany a changed effective character",
    )?;
    if let p::AffectChange::Set {
        observations,
        expires_at_unix,
        ..
    } = change
    {
        require(
            *expires_at_unix > now
                && *expires_at_unix
                    <= control["affect_interface"]["maximum_expires_at_unix"]
                        .as_i64()
                        .unwrap(),
            "affect expiry must be future and within the supplied lifetime bound",
        )?;
        let a = admission(&state.personas[actor], now);
        for id in observations {
            require(
                prepared.supplied_observations.contains(id)
                    && control["actual_observations"]
                        .as_array()
                        .is_some_and(|items| {
                            items.iter().any(|item| {
                                item["id"].as_str() == Some(id) && item.get("payload").is_some()
                            })
                        }),
                "affect basis observation was not actually supplied",
            )?;
            let original = state
                .observations
                .get(id)
                .ok_or_else(|| held("affect basis absent"))?;
            require(
                original.actor == *actor
                    && original
                        .exposure
                        .as_ref()
                        .is_some_and(|exposure| exposure_permitted(state, exposure, &a)),
                "affect basis currently unavailable",
            )?;
        }
    }
    let record = Record {
        revision: digest(&(
            PROFILE,
            &prepared.request,
            prepared.owner_epoch,
            now,
            id,
            change,
        ))?,
        character_revision: character.revision.clone(),
        change: change.clone(),
    };
    // Whole-response adoption retains this receipt and current pointer together.
    // The existing owner/decision fences serialize all changes to this persona.
    let qualifications = tools::qualifications(state, &prepared.sources);
    add_observation(
        state,
        prepared,
        id,
        KIND,
        serde_json::to_value(record)?,
        Vec::new(),
        qualifications,
        now,
    )?;
    let receipt_id = observation_id(&prepared.request.id, KIND, id);
    state
        .observations
        .get_mut(&receipt_id)
        .unwrap()
        .qualification_sources = Some(prepared.sources.clone());
    state.personas.get_mut(actor).unwrap().transient_affect = Some(receipt_id);
    Ok(())
}
