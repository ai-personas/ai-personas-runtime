//! Shared clarification is attributed work context, not consent or a mandate.
//! Original questions, replies and owner conclusions retain separate lineage.
use super::*;


#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionInput {
    pub id: String,
    /// An existing participant or the local requester. Audience is separate.
    pub addressee: String,
    pub text: String,
    pub why: String,
    pub affected_claims: Vec<String>,
    pub evidence_needed: String,
    /// Explain why this person's consent, private input or observation is needed.
    /// None means peers may offer useful clarification; it never proves consent.
    pub human_input_required: Option<String>,
    /// An explicitly conditional interpretation, never a confirmed outside fact.
    pub provisional_interpretation: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionReplyInput {
    pub id: String,
    pub question: String,
    /// Copy the immutable question_revision from CONTROL.shared_questions.
    pub question_revision: String,
    pub text: String,
    /// Explain the attributed observation, option or inference supporting this reply.
    /// This statement is not independently verified evidence or human consent.
    pub basis: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionDisposition {
    Provisional,
    Answered,
    Blocked,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionReplyDisposition {
    Used,
    Declined,
    Unresolved,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionReplyAccount {
    pub disposition: QuestionReplyDisposition,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionConclusionInput {
    pub question: String,
    /// Copy review_revision from the complete current supplied question view.
    pub review_revision: String,
    pub disposition: QuestionDisposition,
    pub conclusion: String,
    /// Account explicitly for every reply ID in the current supplied view.
    pub replies: BTreeMap<String, QuestionReplyAccount>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Origin {
    author: String,
    source_principal: String,
    invocation: Option<String>,
    at_unix: i64,
    audience: BTreeSet<String>,
    exposure: Exposure,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    input: QuestionReplyInput,
    origin: Origin,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Conclusion {
    input: QuestionConclusionInput,
    origin: Origin,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Question {
    input: QuestionInput,
    origin: Origin,
    replies: Vec<Reply>,
    conclusions: Vec<Conclusion>,
}

fn question_revision(question: &Question) -> Result<String> {
    digest(&(&question.input, &question.origin))
}

fn visible(state: &State, origin: &Origin, viewer: &str, a: &f::Admission) -> bool {
    let Some(author) = state.personas.get(&origin.source_principal) else {
        return false;
    };
    let mut recipient = a.clone();
    if !state.personas.contains_key(viewer) {
        if !state.config.local_recipients.contains(viewer) {
            return false;
        }
        recipient = admission(author, a.now_unix);
        recipient.audience = BTreeSet::from([viewer.into()]);
    }
    exposure_permitted(state, &origin.exposure, &recipient)
}

fn attribution(origin: &Origin) -> Value {
    json!({"author":origin.author,"invocation":origin.invocation,"at_unix":origin.at_unix,
        "origin":if origin.invocation.is_some() {"persona_invocation"} else {"local_requester_input"},
        "independently_authenticated":false})
}

pub(crate) fn view(
    state: &State,
    work: &str,
    viewer: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Value> {
    let questions = &state.work_meta[work].questions;
    if questions.is_empty() {
        return Ok(Value::Null);
    }
    let mut items = Vec::new();
    for question in questions.values() {
        if !visible(state, &question.origin, viewer, a) {
            items.push(json!({"status":"withheld_unavailable"}));
            continue;
        }
        extend_exposure(exposure, &question.origin.exposure);
        let mut complete = true;
        let mut replies = Vec::new();
        for reply in &question.replies {
            if visible(state, &reply.origin, viewer, a) {
                extend_exposure(exposure, &reply.origin.exposure);
                replies.push(json!({"input":reply.input,"attribution":attribution(&reply.origin)}));
            } else {
                complete = false;
                replies.push(json!({"status":"withheld_unavailable"}));
            }
        }
        let mut conclusions = Vec::new();
        for conclusion in &question.conclusions {
            if visible(state, &conclusion.origin, viewer, a) {
                extend_exposure(exposure, &conclusion.origin.exposure);
                conclusions.push(
                    json!({"input":conclusion.input,"attribution":attribution(&conclusion.origin)}),
                );
            } else {
                complete = false;
                conclusions.push(json!({"status":"withheld_unavailable"}));
            }
        }
        let last = question.conclusions.last();
        let pending = question.replies.iter().any(|reply| {
            last.and_then(|c| c.input.replies.get(&reply.input.id))
                .is_none_or(|account| account.disposition == QuestionReplyDisposition::Unresolved)
        });
        items.push(json!({
            "question":question.input,"question_revision":question_revision(question)?,
            "attribution":attribution(&question.origin),"owner":question.origin.author,
            "follow_through":if state.board.work(&w::WorkId(work.into()))?
                .acceptance(&w::PersonaId(question.origin.author.clone())).is_some() {
                    "original_owner_has_current_accepted_participation"
                } else {"ownership_gap_original_owner_has_no_current_acceptance"},
            "replies":replies,"conclusions":conclusions,
            "review_revision":if complete {Some(digest(question)?)} else {None},
            "owner_review_pending":if complete {Some(last.is_none() || pending)} else {None},
            "body_supplied":true
        }));
    }
    let result = json!({"interface":"shared-questions-v1","items":items,
        "rule":"Questions and replies remain current after inbox acknowledgment. The owner assesses every reply's basis with conclude_question and an explicit account per reply. The basis is its author's statement: a source citation there does not prove that its body was actually supplied or read. Inspect actual evidence before accepting consequential claims. A later reply creates a new outstanding owner decision. Asking does not make an answer a blocking dependency or create a wake. Choose useful independent progress, provisional interpretation, waiting or stopping yourself. A human-input requirement cannot be satisfied by peer confidence, silence or an assumption. Conclusions are attributed interpretations, never mandate changes, verified facts, consent, releases or automatic permission. Keep affected claims conditional and reconsider them when new input conflicts. Separate conclusions preserve original question and reply access; current permission withdrawal still applies."});
    Ok(result)
}

fn supplied(prepared: &Prepared, question: &Question) -> Result<Value> {
    let control = request_context::decode(prepared)?.control;
    require(
        s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256
            && control["actor"] == prepared.request.persona
            && control["work_id"] == prepared.request.work
            && control["invocation_id"] == prepared.request.id,
        "shared question admission mismatch",
    )?;
    let revision = question_revision(question)?;
    control["shared_questions"]["items"]
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item["question"]["id"] == question.input.id
                    && item["question_revision"] == revision
                    && item["question"]
                        == serde_json::to_value(&question.input).unwrap_or(Value::Null)
                    && item["body_supplied"] == true
            })
        })
        .cloned()
        .ok_or_else(|| held("exact question body was not supplied in this admission"))
}

fn origin(state: &State, prepared: &Prepared, now: i64) -> Result<Origin> {
    let mut audience = prepared.audience.clone();
    audience.insert(prepared.request.persona.clone());
    Ok(Origin {
        author: prepared.request.persona.clone(),
        source_principal: prepared.request.persona.clone(),
        invocation: Some(prepared.request.id.clone()),
        at_unix: now,
        audience,
        exposure: expanded_exposure(state, prepared_exposure(prepared)?)?,
    })
}

pub(super) fn validate(action: &WorkAction) -> Result<()> {
    match action {
        WorkAction::AskQuestion(input) => {
            p::check_id(&input.id, "question.id")?;
            identity(&input.addressee)?;
            for value in [&input.text, &input.why, &input.evidence_needed] {
                text(value, 4096)?;
            }
            for value in input
                .affected_claims
                .iter()
                .chain(input.human_input_required.iter())
                .chain(input.provisional_interpretation.iter())
            {
                text(value, 4096)?;
            }
        }
        WorkAction::AnswerQuestion(input) => validate_reply(input)?,
        WorkAction::ConcludeQuestion(input) => {
            p::check_id(&input.question, "question.id")?;
            text(&input.review_revision, 64)?;
            text(&input.conclusion, 8192)?;
            for (id, account) in &input.replies {
                p::check_id(id, "question.reply")?;
                text(&account.reason, 4096)?;
            }
            require(
                input.disposition != QuestionDisposition::Answered
                    || input
                        .replies
                        .values()
                        .all(|r| r.disposition != QuestionReplyDisposition::Unresolved),
                "answered question cannot leave a reply unresolved",
            )?;
        }
        _ => (),
    }
    Ok(())
}

pub(crate) fn validate_reply(input: &QuestionReplyInput) -> Result<()> {
    p::check_id(&input.id, "question.reply.id")?;
    p::check_id(&input.question, "question.reply.question")?;
    text(&input.question_revision, 64)?;
    text(&input.text, 8192)?;
    text(&input.basis, 4096)
}

fn recipient_admission(state: &State, work: &str, viewer: &str, now: i64) -> Result<f::Admission> {
    if let Some(persona) = state.personas.get(viewer) {
        return Ok(admission(persona, now));
    }
    require(
        viewer == state.work_meta[work].intake_operator
            && state.config.local_recipients.contains(viewer),
        "question recipient unavailable",
    )?;
    // Local requester inspection uses the same source-principal and audience
    // distinction as local message delivery; it grants no source-body access.
    let owner = state.work_meta[work]
        .questions
        .values()
        .next()
        .map(|q| &q.origin.author)
        .or_else(|| {
            state
                .board
                .work(&w::WorkId(work.into()))
                .ok()
                .and_then(|w| w.participants.keys().next())
                .map(|p| &p.0)
        })
        .ok_or_else(|| held("question work has no source principal"))?;
    let mut a = admission(&state.personas[owner], now);
    a.audience = BTreeSet::from([viewer.into()]);
    Ok(a)
}

fn viewers(state: &State, work: &str) -> Result<BTreeSet<String>> {
    let mut recipients: BTreeSet<String> = state
        .board
        .work(&w::WorkId(work.into()))?
        .participants
        .keys()
        .map(|p| p.0.clone())
        .collect();
    recipients.insert(state.work_meta[work].intake_operator.clone());
    Ok(recipients)
}

fn validate_views(state: &State, work: &str, now: i64) -> Result<()> {
    for viewer in viewers(state, work)? {
        let a = recipient_admission(state, work, &viewer, now)?;
        view(state, work, &viewer, &a, &mut Exposure::default())?;
    }
    Ok(())
}

pub(super) fn apply(
    state: &mut State,
    prepared: &Prepared,
    action: &WorkAction,
    now: i64,
) -> Result<()> {
    validate(action)?;
    let work = &prepared.request.work;
    let actor = &prepared.request.persona;
    require(
        prepared.request.mode == Mode::Ordinary
            && state
                .board
                .work(&w::WorkId(work.clone()))?
                .acceptance(&w::PersonaId(actor.clone()))
                .is_some(),
        "shared clarification requires ordinary accepted participation",
    )?;
    let authored = origin(state, prepared, now)?;
    match action {
        WorkAction::AskQuestion(input) => {
            require(
                !state.work_meta[work].questions.contains_key(&input.id),
                "question identity already retained",
            )?;
            require(
                viewers(state, work)?.contains(&input.addressee),
                "question addressee is outside the current work",
            )?;
            let a = recipient_admission(state, work, &input.addressee, now)?;
            require(
                visible(state, &authored, &input.addressee, &a),
                "question addressee cannot receive this derived question",
            )?;
            state.work_meta.get_mut(work).unwrap().questions.insert(
                input.id.clone(),
                Question {
                    input: input.clone(),
                    origin: authored,
                    replies: Vec::new(),
                    conclusions: Vec::new(),
                },
            );
        }
        WorkAction::AnswerQuestion(input) => {
            let question = state.work_meta[work]
                .questions
                .get(&input.question)
                .ok_or_else(|| held("question unavailable"))?;
            require(
                visible(
                    state,
                    &question.origin,
                    actor,
                    &admission(&state.personas[actor], now),
                ),
                "question is no longer permitted",
            )?;
            supplied(prepared, question)?;
            require(
                input.question_revision == question_revision(question)?
                    && !question.replies.iter().any(|r| r.input.id == input.id),
                "question reply identity or revision mismatch",
            )?;
            state
                .work_meta
                .get_mut(work)
                .unwrap()
                .questions
                .get_mut(&input.question)
                .unwrap()
                .replies
                .push(Reply {
                    input: input.clone(),
                    origin: authored,
                });
        }
        WorkAction::ConcludeQuestion(input) => {
            let question = state.work_meta[work]
                .questions
                .get(&input.question)
                .ok_or_else(|| held("question unavailable"))?;
            let mut current_exposure = Exposure::default();
            let current_view = view(
                state,
                work,
                actor,
                &admission(&state.personas[actor], now),
                &mut current_exposure,
            )?;
            let current = supplied(prepared, question)?;
            require(question.origin.author == *actor && input.review_revision == digest(question)?
                && current["review_revision"] == input.review_revision
                && current_view["items"].as_array().is_some_and(|items| items.contains(&current))
                && input.replies.len() == question.replies.len()
                && question.replies.iter().all(|r| input.replies.contains_key(&r.input.id)),
                "question conclusion requires the owner, exact complete current review and every reply account")?;
            for viewer in viewers(state, work)? {
                let a = recipient_admission(state, work, &viewer, now)?;
                let prior_visible = visible(state, &question.origin, &viewer, &a)
                    || question
                        .replies
                        .iter()
                        .any(|r| visible(state, &r.origin, &viewer, &a));
                require(!prior_visible || visible(state,&authored,&viewer,&a), "question conclusion would narrow previously readable question or reply access; preserve the original records and communicate any restricted conclusion separately")?;
            }
            state
                .work_meta
                .get_mut(work)
                .unwrap()
                .questions
                .get_mut(&input.question)
                .unwrap()
                .conclusions
                .push(Conclusion {
                    input: input.clone(),
                    origin: authored,
                });
        }
        _ => return Err(invalid("not a shared question action")),
    }
    validate_views(state, work, now)
}

/// Called after the board transition so notifications can satisfy existing
/// message waits without invalidating that transition's version guard.
pub(super) fn deliver_changes(
    state: &mut State,
    previous: &State,
    prepared: &Prepared,
    now: i64,
) -> Result<()> {
    let work = &prepared.request.work;
    let mut events = Vec::new();
    for (id, question) in &state.work_meta[work].questions {
        let before = previous.work_meta[work].questions.get(id);
        if before.is_none() {
            events.push((
                bounded_composite_id("shared-question", &[id]),
                json!({"kind":"shared_question","question":question.input}),
                question.origin.clone(),
            ));
        }
        for reply in question
            .replies
            .iter()
            .skip(before.map_or(0, |q| q.replies.len()))
        {
            events.push((
                bounded_composite_id("shared-question-reply", &[id, &reply.input.id]),
                json!({"kind":"shared_question_reply","reply":reply.input}),
                reply.origin.clone(),
            ));
        }
        for (index, conclusion) in question
            .conclusions
            .iter()
            .enumerate()
            .skip(before.map_or(0, |q| q.conclusions.len()))
        {
            events.push((
                bounded_composite_id("shared-question-conclusion", &[id, &index.to_string()]),
                json!({"kind":"shared_question_conclusion","conclusion":conclusion.input}),
                conclusion.origin.clone(),
            ));
        }
    }
    for (suffix, payload, origin) in events {
        require(
            origin.invocation.as_ref() == Some(&prepared.request.id),
            "question notification origin mismatch",
        )?;
        let body = serde_json::to_string(&payload)?;
        for recipient in viewers(state, work)? {
            if recipient == prepared.request.persona {
                continue;
            }
            let a = recipient_admission(state, work, &recipient, now)?;
            if visible(state, &origin, &recipient, &a) {
                deposit_message(
                    state,
                    prepared,
                    &suffix,
                    &recipient,
                    "Shared work clarification",
                    &body,
                    &[],
                    now,
                )?;
            }
        }
    }
    Ok(())
}

pub(crate) fn operator_view(state: &State, operator: &str, work: &str, now: i64) -> Result<Value> {
    let meta = state
        .work_meta
        .get(work)
        .ok_or_else(|| held("question work unavailable"))?;
    require(
        meta.intake_operator == operator,
        "question inspection requires the work requester",
    )?;
    let a = recipient_admission(state, work, operator, now)?;
    let mut inherited = work_exposure(state, work)?;
    require(
        exposure_permitted(state, &inherited, &a),
        "question work unavailable to requester",
    )?;
    view(state, work, operator, &a, &mut inherited)
}

pub(crate) fn operator_reply(
    state: &mut State,
    operator: &str,
    work: &str,
    input: &QuestionReplyInput,
    now: i64,
) -> Result<bool> {
    validate_reply(input)?;
    operator_view(state, operator, work, now)?;
    let question = state.work_meta[work]
        .questions
        .get(&input.question)
        .ok_or_else(|| held("question unavailable"))?;
    let a = recipient_admission(state, work, operator, now)?;
    require(
        visible(state, &question.origin, operator, &a)
            && question_revision(question)? == input.question_revision,
        "requester reply requires its exact currently permitted question",
    )?;
    if let Some(old) = question.replies.iter().find(|r| r.input.id == input.id) {
        require(
            old.origin.author == operator
                && old.origin.invocation.is_none()
                && digest(&old.input)? == digest(input)?,
            "question reply identity conflict",
        )?;
        return Ok(true);
    }
    let mut inherited = work_exposure(state, work)?;
    extend_exposure(&mut inherited, &question.origin.exposure);
    let origin = Origin {
        author: operator.into(),
        source_principal: question.origin.source_principal.clone(),
        invocation: None,
        at_unix: now,
        audience: question.origin.audience.clone(),
        exposure: inherited,
    };
    state
        .work_meta
        .get_mut(work)
        .unwrap()
        .questions
        .get_mut(&input.question)
        .unwrap()
        .replies
        .push(Reply {
            input: input.clone(),
            origin: origin.clone(),
        });
    validate_views(state, work, now)?;
    for recipient in viewers(state, work)? {
        let Some(persona) = state.personas.get(&recipient) else {
            continue;
        };
        if !visible(state, &origin, &recipient, &admission(persona, now)) {
            continue;
        }
        let id = bounded_composite_id(
            "question-requester-reply",
            &[work, &input.question, &input.id, &recipient],
        );
        require(
            !state.observations.contains_key(&id),
            "question reply observation identity conflict",
        )?;
        state.observations.insert(
            id.clone(),
            Observation {
                id: id.clone(),
                actor: recipient.clone(),
                work: work.into(),
                invocation: None,
                kind: "shared_question_requester_reply".into(),
                at_unix: now,
                payload: json!({"reply":input,"attribution":attribution(&origin)}),
                sources: Vec::new(),
                exposure: Some(origin.exposure.clone()),
                qualified_roots: Vec::new(),
                qualification_sources: None,
            },
        );
        state
            .personas
            .get_mut(&recipient)
            .unwrap()
            .focus
            .get_mut(work)
            .ok_or_else(|| held("question participant focus unavailable"))?
            .pending_observations
            .push(id.clone());
        requester_wait::satisfy(state, &id, operator, now)?;
    }
    Ok(false)
}
