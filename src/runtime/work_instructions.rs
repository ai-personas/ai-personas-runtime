//! Command-specific instructions supplied with the actual admitted interface.
//! Shared persona, evidence, authorization and response duties stay mandatory.
use super::*;

const PROFILE: &str = "native-work-command-instructions-v1";
const MAX_BYTES: usize = 16_384;

struct Section {
    id: &'static str,
    commands: &'static [&'static str],
    text: &'static str,
}

const SECTIONS: &[Section] = &[
    Section {
        id: "formal_evidence",
        commands: &["assess", "judge"],
        text: "judge.claims.<outcome>.evidence and assess.evidence use formal work IDs: CONTROL.model_evidence/human_evidence keys or an earlier record_model_evidence inner id, never observation/message or outer operation IDs. Every evidence ID in a met claim must support that outcome's exact selected artifact and current criterion. Evidence bound only to another artifact cannot support that claim, even when both documents were reviewed together.",
    },
    Section {
        id: "invitation",
        commands: &["invite"],
        text: "An accepted participant may explicitly author a new invite with different terms; empty outcomes and review_roles allow an offered contribution without acquiring exclusive outcome or reviewer-role ownership. This does not rewrite an existing offer or accept for its recipient.",
    },
    Section {
        id: "artifact_registration",
        commands: &["put_artifact"],
        text: "put_artifact registers a complete UTF-8 supporting asset: its subject must name an assets source, with no range. For a same-response save, copy the FileWrite or FileImport source, use revision=proposal-op:<FileWrite.id>, and include that FileWrite.id in depends_on. Artifact base: null for creation; exact current ArtifactKey for update.",
    },
    Section {
        id: "model_evidence",
        commands: &["record_model_evidence"],
        text: "record_model_evidence: attributed ModelJudgment of the exact current registered artifact/outcome, with reasons and limitations. Requires its full UTF-8 body supplied now; metadata, summaries, planned selections, same-response saves/reads do not qualify. No human testimony, execution, measurement, outside fact, consent, independent approval or engineering assurance is established. Criterion and outside evidence/review requirements remain; attribution does not verify truth. Later operations cite its inner evidence id, not the outer effect id; no forward references. depends_on names local saves only. Per work: 128 records/2097152 serialized bytes, within response/context/Store limits.",
    },
    Section {
        id: "local_delivery",
        commands: &["deliver_release"],
        text: "deliver_release: existing or earlier-in-response release only.",
    },
    Section {
        id: "closure_supply",
        commands: &["close_goal"],
        text: "CloseGoal requires a matching record and the required exact receipts already supplied in this admitted request's CONTROL.closure_readiness.records; an absent key or missing record does not satisfy that gate. Newly created release/delivery receipts become usable for CloseGoal only after acceptance and supply in a fresh admission; ordering these commands earlier in the same Decision does not satisfy that gate.",
    },
    Section {
        id: "closure_decision",
        commands: &["close_goal"],
        text: "CONTROL.closure_readiness.projection_claims and each record's projection_claims describe supplied evidence, persona judgments and adoption validations. release and released_artifacts bind one exact recorded release to outcome IDs and ArtifactKeys; they supply no bodies and assert no currentness. permitted_prerequisites_observed covers only projected permitted deliveries and latest-permitted acceptance. Unestablished prerequisites may be absent or inaccessible; omissions and truncation establish neither nonexistence nor permission. Validated exact-release receipts freshly supplied in these records satisfy the receipt-supply component of require_supplied without a duplicate complete deliveries inventory. Independently assess fulfillment and authority before proposing CloseGoal, explaining that assessment in Decision.work. This does not require a new persisted Judge command: Judge supersedes your previous vote for that exact candidate, even if its conclusion is unchanged, and can invalidate the existing shared decision and release support. Use Judge when you intend that record change, with a new shared decision/release and freshly supplied delivery receipts as required. If independently satisfied and authorized, you may propose CloseGoal; otherwise identify real remaining work or voluntarily yield with a concrete continuation requirement. CONTROL.command_resource_accounting.close_goal.navigation_charge=none means CloseGoal consumes no invocation navigation allowance; zero navigation alone does not prohibit proposing it. This accounting fact grants no authority or resources, refills nothing and makes no claim about other commands' charges. Complete response validation and bounds, current grants, authority and accepted responsibility, current release support, exact permitted supplied delivery receipts and each required stakeholder's globally latest positive exact-release consent still apply at adoption. Empty stakeholders require no human consent and prove none.",
    },
    Section {
        id: "formal_assessment",
        commands: &["assess"],
        text: "Formal assess needs a role in CONTROL.mandate.review_roles, in accepted terms, and required by the outcome. Empty accepted review_roles means no formal assessment role. It requires nonempty available evidence bound to the exact current artifact/criterion; empty evidence cannot establish a passing assessment.",
    },
    Section {
        id: "candidate_delivery_roots",
        commands: &["judge", "decide_shared"],
        text: "In judge and decide_shared, artifacts maps exact mandate outcome ids to ArtifactKeys. This map determines the delivered artifact roots: mentioning other registered files or citing their evidence does not deliver their bodies. If one outcome requires several documents, select a candidate whose complete qualified bundle includes all of them, with current permissions and required accompaniment intact.",
    },
    Section {
        id: "arrangement_proposal",
        commands: &["propose_arrangement"],
        text: "ProposeArrangement.consent_own is required: false records the proposal alone; true explicitly records only the proposer's own consent to those exact authored terms and can activate the proposal if every required consent and authorization is then satisfied.",
    },
    Section {
        id: "arrangement_response",
        commands: &["accept_arrangement", "decline_arrangement"],
        text: "For a separate accept or decline action, copy the proposal id and exact {id,revision,digest} from this admitted CONTROL; digest is lowercase hexadecimal. Work exact identities are distinct from file ExactRefs and ArtifactKeys. Consent is only your own, bound to the exact proposal and current accepted responsibility.",
    },
    Section {
        id: "arrangement_delegation",
        commands: &["propose_arrangement", "accept_arrangement"],
        text: "Copy delegation=CONTROL.arrangements.delegation_exact in every dynamic proposal. The requester's immutable AffectedPartyAssentV1 delegation governs arrangement changes: a result-rule change requires every actual current accepted contributor's exact consent, including later joiners, plus every affected duty or artifact holder. A result-decider role alone grants no amendment authority. The core rechecks current base, prior acceptances, authority and preserved duty coverage at atomic activation. Fixed legacy decision rules permit duty-only arrangements with decision_rule=null.",
    },
    Section {
        id: "arranged_membership",
        commands: &["accept_arranged_offer"],
        text: "Once current_exact is present, joining requires accept_arranged_offer with the exact supplied offer, terms_revision and current arrangement; a Responsibility accept cannot imply assent to an arrangement. Before formation, existing responsibility operations retain their exact offer semantics.",
    },
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuppliedSection {
    id: String,
    applies_to: Vec<String>,
    text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Supplied {
    profile: String,
    sections: Vec<SuppliedSection>,
}

/// A section shared by several commands is supplied once, naming only the
/// commands actually offered by the selected interface.
pub(super) fn for_interface(interface: &WorkInterface) -> Result<Option<Value>> {
    let commands = interface.selected_commands.as_ref()
        .ok_or_else(|| invalid("native work instructions require selected command definitions"))?;
    let mut sections = Vec::new();
    for section in SECTIONS {
        let applies_to: Vec<String> = section
            .commands
            .iter()
            .copied()
            .filter(|command| commands.contains(*command))
            .map(str::to_owned)
            .collect();
        if !applies_to.is_empty() {
            sections.push(SuppliedSection {
                id: section.id.into(),
                applies_to,
                text: section.text.into(),
            });
        }
    }
    if sections.is_empty() {
        return Ok(None);
    }
    let value = serde_json::to_value(Supplied {
        profile: PROFILE.into(),
        sections,
    })?;
    require(
        serde_json::to_vec(&value)?.len() <= MAX_BYTES,
        "native work instruction byte bound",
    )?;
    Ok(Some(value))
}

/// Newly installed ordinary guide files carry the same command-specific rules.
/// Reading that file is still separate from exposing its current definition.
pub(super) fn guide_text(command: &str) -> String {
    let sections: Vec<_> = SECTIONS
        .iter()
        .filter(|section| section.commands.contains(&command))
        .map(|section| section.text)
        .collect();
    if sections.is_empty() {
        String::new()
    } else {
        format!("Additional command rules:\n\n{}\n\n", sections.join("\n\n"))
    }
}

pub(super) fn bytes(capability: &Value) -> Result<usize> {
    capability
        .get("command_instructions")
        .map(|value| {
            serde_json::to_vec(value)
                .map(|bytes| bytes.len())
                .map_err(Into::into)
        })
        .unwrap_or(Ok(0))
}

/// Read the retained input instead of reconstructing historical instructions
/// from today's code. Absence in an old input is not absence of old prose rules.
pub(crate) fn inspect(prepared: &Prepared, control: &Value) -> Result<Value> {
    let Some(value) = control["work_capability"].get("command_instructions") else {
        return Ok(json!({"available":false,
            "reason":"No separately scoped work-command instructions recorded in this input."}));
    };
    let interface = prepared
        .work_interface
        .as_ref()
        .ok_or_else(|| invalid("native work instructions without an admitted interface"))?;
    require(
        control["work_capability"]["binding_revision"] == interface.binding_revision
            && control["work_capability"]["inputs_schema"] == interface.inputs_schema,
        "native work instruction interface mismatch",
    )?;
    let bytes = serde_json::to_vec(value)?.len();
    require(bytes <= MAX_BYTES, "native work instruction byte bound")?;
    let supplied: Supplied = serde_json::from_value(value.clone())?;
    require(
        supplied.profile == PROFILE
            && !supplied.sections.is_empty()
            && supplied.sections.len() <= 32,
        "native work instruction profile",
    )?;
    let available: BTreeSet<_> = interface.inputs_schema["oneOf"]
        .as_array()
        .ok_or_else(|| invalid("native work instruction schema"))?
        .iter()
        .filter_map(|variant| variant["properties"]["kind"]["const"].as_str())
        .collect();
    let mut seen = BTreeSet::new();
    let mut sections = Vec::new();
    for section in supplied.sections {
        require(
            !section.id.is_empty()
                && section.id.len() <= 128
                && seen.insert(section.id.clone())
                && !section.text.is_empty()
                && !section.applies_to.is_empty()
                && section.applies_to.len() <= available.len()
                && section.applies_to.iter().collect::<BTreeSet<_>>().len()
                    == section.applies_to.len()
                && section
                    .applies_to
                    .iter()
                    .all(|command| available.contains(command.as_str())),
            "native work instruction scope mismatch",
        )?;
        sections.push(json!({"id":section.id,"applies_to":section.applies_to,
            "text_utf8_bytes":section.text.len()}));
    }
    Ok(
        json!({"available":true,"profile":PROFILE,"serialized_utf8_bytes":bytes,
        "sections":sections,
        "meaning":"Command-specific explanation actually supplied in this admitted input. It grants no authority and establishes neither execution nor correct application."}),
    )
}
