use super::*;
use crate::provider::{AttemptReceipt, RequestAudit, Usage};

fn fixture() -> Connection {
    let db=Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE objects(digest TEXT PRIMARY KEY,media TEXT,bytes BLOB);
        CREATE TABLE invocations(id TEXT PRIMARY KEY,work TEXT,actor TEXT,state TEXT,response TEXT,receipt TEXT);
        CREATE TABLE versions(invocation TEXT); CREATE TABLE outbox(invocation TEXT);
        CREATE TABLE events(seq INTEGER PRIMARY KEY,work TEXT,actor TEXT,target TEXT,kind TEXT,digest TEXT,invocation TEXT,created INTEGER);").unwrap();
    db
}
fn attempt(call: &str) -> AttemptReceipt {
    AttemptReceipt {
        audit:RequestAudit { invocation_id:call.into(),prompt_sha256:"0".repeat(64),prompt_bytes:0,
            reusable_prefix_bytes:None,input_history:None,schema_sha256:"0".repeat(64),schema_bytes:0,
            instructions_sha256:"0".repeat(64),request_sha256:"0".repeat(64),request_bytes:0,
            configuration_sha256:"0".repeat(64),transport_profile:"test".into(),max_output_tokens:None,image_inputs:vec![] },
        intake_expires_at_unix:None,intake_prelaunch_checked_at_unix:None,dispatch_may_have_begun:true,
        http_status:Some(200),http_diagnostic:None,network_diagnostic:None,stream_bytes_observed:4468,
        stream_eof:false,event_count:4,response_completed:false,reported_terminal_status:None,
        stream_diagnostic:Some(super::tests::observed_overload()),stream_error_diagnostic:None,
        stream_progress:None,response_id:None,response_id_sha256:None,private_output_items:None,
        web_search:None,reported_effort:None,previous_reported_effort:None,reported_model:None,
        usage:Usage::default(),elapsed_ms:929,remote_stop_confirmed:false,remote_effects_and_charges_may_remain:true,
    }
}
fn retained(db: &Connection, call: &str, receipt: &AttemptReceipt) -> String {
    let digest=store::put(db,receipt).unwrap();
    db.execute("INSERT INTO invocations VALUES(?1,'task','persona-1','provider_failed',NULL,?2)",params![call,digest]).unwrap();
    format!("object:{digest}")
}

#[test]
fn failed_overloads_persist_backoff_without_a_retry_quota_or_effect_replay() {
    let db=fixture();
    for (index,delay) in [5,10,20,40,60,60,60].into_iter().enumerate() {
        let call=format!("call-{index}");let receipt=attempt(&call);let reference=retained(&db,&call,&receipt);
        let error=ProviderError::Attempt { failure:Failure::RemoteFailure,receipt:Box::new(receipt) };
        assert!(schedule(&db,"task","persona-1",&call,&reference,&error,false).unwrap());
        let record=scheduled(&db,&call).unwrap().unwrap();
        assert_eq!(record["delay_seconds"],delay);
        assert_eq!(record["consecutive_failures"],index+1);
        assert_eq!(record["failed_receipt"],reference);
        assert_eq!(record["external_operations_replayed"],false);
        assert_eq!(record["prior_response_adopted"],false);
        assert_eq!(record["remote_stop_confirmed"],false);
        assert_eq!(record["remote_effects_and_charges_may_remain"],true);
        assert!(record["not_before_unix"].as_i64().unwrap()>=now()+delay-1);
    }
    assert!(pending(&db,"persona-1").unwrap().is_some());
    assert_eq!(db.query_row("SELECT count(*) FROM outbox",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn probes_completed_responses_and_provider_refusals_do_not_schedule() {
    for case in 0..4 {
        let db=fixture();let mut receipt=attempt("call-test");
        if case==1 { receipt.response_completed=true; }
        if case==2 { receipt.reported_terminal_status=Some(crate::provider::ReportedTerminalStatus::Failed); }
        let reference=retained(&db,"call-test",&receipt);
        let error=ProviderError::Attempt { failure:if case==3 { Failure::Binding } else { Failure::RemoteFailure },receipt:Box::new(receipt) };
        assert!(!schedule(&db,"task","persona-1","call-test",&reference,&error,case==0).unwrap());
        assert!(scheduled(&db,"call-test").unwrap().is_none());
    }
}

#[test]
fn adopted_state_effects_and_receipt_mismatches_cannot_be_recovered() {
    for case in 0..4 {
        let db=fixture();let receipt=attempt("call-test");let mut reference=retained(&db,"call-test",&receipt);
        match case {
            0=>{db.execute("INSERT INTO versions VALUES('call-test')",[]).unwrap();},
            1=>{db.execute("INSERT INTO outbox VALUES('call-test')",[]).unwrap();},
            2=>{reference=format!("object:{}","f".repeat(64));},
            _=>{db.execute("UPDATE invocations SET response='already-complete'",[]).unwrap();},
        }
        let error=ProviderError::Attempt { failure:Failure::RemoteFailure,receipt:Box::new(receipt) };
        assert!(schedule(&db,"task","persona-1","call-test",&reference,&error,false).is_err());
        assert!(scheduled(&db,"call-test").unwrap().is_none());
    }
}
