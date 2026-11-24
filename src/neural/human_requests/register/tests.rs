use super::*;

fn fixture() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE objects(digest TEXT PRIMARY KEY,media TEXT,bytes BLOB);
        CREATE TABLE heads(kind TEXT,id TEXT,revision INTEGER,digest TEXT);").unwrap();
    for (id,work,state,holder) in [("a","task","open",Value::Null),
        ("b","task","in_progress",json!("Volunteer")),("c","task","completed",json!("Volunteer")),
        ("d","other-task","open",Value::Null)] {
        let body = json!({"work":work,"state":state,"claimed_by":holder,"title":id,
            "requester":"persona-1","action":"private instructions",
            "last_report":{"text":"private report"},"artifacts":["private attachment"]});
        let digest = store::put(&db,&body).unwrap();
        db.execute("INSERT INTO heads VALUES('human_request',?1,3,?2)",params![id,digest]).unwrap();
    }
    db
}
fn query(include_closed: bool, offset: usize, page_size: usize) -> Lookup {
    Lookup { include_closed, offset, page_size }
}

#[test]
fn outside_step_discovery_is_scoped_and_metadata_only() {
    let db = fixture();
    assert!(available(&db,"task").unwrap());
    assert!(!available(&db,"absent").unwrap());
    let result = list(&db,"task",query(false,0,20)).unwrap();
    let requests = result["requests"].as_array().unwrap();
    assert_eq!(requests.len(),2);
    assert_eq!(requests[0]["reference"],"human_request:a@3");
    assert_eq!(requests[1]["claimed_by"],"Volunteer");
    assert_eq!(result["next_offset"],Value::Null);
    for request in requests {
        assert_eq!(request.as_object().unwrap().len(),5);
        for key in ["action","last_report","artifacts"] { assert!(request.get(key).is_none()); }
    }
}

#[test]
fn pagination_does_not_omit_or_repeat_steps() {
    let db = fixture();
    let first = list(&db,"task",query(true,0,1)).unwrap();
    assert_eq!(first["next_offset"],1);
    let second = list(&db,"task",query(true,1,1)).unwrap();
    assert_eq!(second["requests"][0]["reference"],"human_request:b@3");
    assert_eq!(second["next_offset"],2);
    let third = list(&db,"task",query(true,2,1)).unwrap();
    assert_eq!(third["requests"][0]["state"],"completed");
    assert_eq!(third["next_offset"],Value::Null);
    assert!(list(&db,"task",query(false,0,0)).is_err());
    assert_eq!(list(&db,"task",query(false,2,1)).unwrap()["requests"],json!([]));
}
