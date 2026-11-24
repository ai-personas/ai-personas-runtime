use super::*;

fn fixture() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE objects(digest TEXT PRIMARY KEY,media TEXT,bytes BLOB);
        CREATE TABLE events(seq INTEGER PRIMARY KEY,work TEXT,actor TEXT,target TEXT,
        kind TEXT,digest TEXT,invocation TEXT);").unwrap();
    db
}
fn event(db: &Connection, work: &str, actor: &str, target: &str, kind: &str,
         invocation: Option<&str>, body: Value) -> i64 {
    let digest = put(db,&body).unwrap();
    db.execute("INSERT INTO events(work,actor,target,kind,digest,invocation) VALUES(?1,?2,?3,?4,?5,?6)",
        params![work,actor,target,kind,digest,invocation]).unwrap();
    db.last_insert_rowid()
}
fn waiting(db: &Connection, after: i64, kinds: &[&str]) -> bool {
    has_arrival(db,"task","persona-1",after,&kinds.iter().map(|s|s.to_string()).collect::<Vec<_>>()).unwrap()
}

#[test]
fn peer_return_after_admission_remains_available_to_a_new_wait() {
    let db = fixture();
    let cursor = event(&db,"task","user","persona-1","invitation",None,json!({}));
    event(&db,"task","persona-2","persona-1","message",Some("peer-call"),json!({}));
    assert!(waiting(&db,cursor,&["peer_message"]));
    assert!(!waiting(&db,cursor+1,&["peer_message"]));
}

#[test]
fn unrelated_events_and_context_selection_do_not_wake_peer_wait() {
    let db = fixture();
    event(&db,"other-task","persona-2","persona-1","message",Some("call"),json!({}));
    event(&db,"task","persona-2","persona-3","message",Some("call"),json!({}));
    event(&db,"task","persona-1","persona-1","message",Some("call"),json!({}));
    event(&db,"task","user","persona-1","user_message",None,json!({}));
    assert!(!waiting(&db,0,&["peer_message"]));
    let cursor = db.last_insert_rowid();
    event(&db,"task","persona-1","persona-1","appraisal_selected",Some("call"),json!({}));
    assert!(!waiting(&db,cursor,&[]));
}

#[test]
fn outside_wait_requires_a_newer_report_on_the_chosen_request() {
    let db = fixture();
    let trigger = ["human_request:help-a@2"];
    event(&db,"task","Volunteer","persona-1","user_message",None,json!({"human_request":"human_request:help-b@3"}));
    event(&db,"task","Volunteer","persona-1","human_request_progress",None,json!({"human_request":"human_request:help-a@2"}));
    assert!(!waiting(&db,0,&trigger));
    event(&db,"task","Observer","persona-1","user_message",None,json!({"human_request":"human_request:help-a@3"}));
    assert!(waiting(&db,0,&trigger));
}

#[test]
fn broadcast_visibility_starts_at_the_actual_invitation() {
    let db = fixture();
    event(&db,"task","persona-2","*","message",Some("call"),json!({}));
    assert!(!waiting(&db,0,&[]));
    let cursor=event(&db,"task","user","persona-1","invitation",None,json!({}));
    assert!(!waiting(&db,cursor,&[]));
    event(&db,"task","persona-2","*","message",Some("call"),json!({}));
    assert!(waiting(&db,cursor,&[]));
}
