use super::*;

fn editions(state: &str, holder: Value) -> (Value, Value, Value) {
    let prior = json!({"work":"activity","requester":"persona-1","title":"Assemble",
        "action":"Assemble the issued files","state":state,"claimed_by":holder});
    let mut current = prior.clone();
    current["last_report"] = json!({"action":"contribute"});
    let notice = json!({"action":"contribute","state":state,"claimed_by":holder,
        "text":"Observed the digital drawing, not physical completion","attachments":[]});
    (prior, current, notice)
}

#[test]
fn contribution_preserves_open_or_held_responsibility() {
    for (state, holder) in [("open", Value::Null), ("in_progress", json!("Holder"))] {
        let (a,b,c) = editions(state, holder);
        assert!(contribution_matches(&a,&b,&c));
    }
}

#[test]
fn contribution_cannot_change_holder_or_finish_the_step() {
    let (a,b,c) = editions("in_progress", json!("Holder"));
    for change in [json!({"claimed_by":"Observer"}),json!({"state":"completed"}),json!({"action":"Changed instructions"})] {
        let mut altered = b.clone();
        for (key,value) in change.as_object().unwrap() { altered[key] = value.clone(); }
        assert!(!contribution_matches(&a,&altered,&c));
    }
    let mut forged = c.clone();
    forged["claimed_by"] = json!("Observer");
    assert!(!contribution_matches(&a,&b,&forged));
}

#[test]
fn closed_and_malformed_responsibilities_reject_contributions() {
    for (state, holder) in [("completed",json!("Holder")),("unable",json!("Holder")),
        ("withdrawn",Value::Null),("open",json!("Holder")),("in_progress",Value::Null),
        ("in_progress",json!("  "))] {
        let (a,b,c) = editions(state, holder);
        assert!(!contribution_matches(&a,&b,&c));
    }
}

#[test]
fn attachment_only_observation_is_not_an_empty_report() {
    let (a,b,mut c) = editions("open", Value::Null);
    c["text"] = json!("");
    assert!(!contribution_matches(&a,&b,&c));
    c["attachments"] = json!(["object:evidence"]);
    assert!(contribution_matches(&a,&b,&c));
}

#[test]
fn ordinary_owner_update_is_not_a_contributor_report() {
    let (a,mut b,mut c) = editions("in_progress", json!("Holder"));
    b["last_report"].as_object_mut().unwrap().remove("action");
    c.as_object_mut().unwrap().remove("action");
    assert!(!contribution_matches(&a,&b,&c));
}
