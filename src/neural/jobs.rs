//! Persona-selected background commands. A submitted job, a launched process,
//! an output observation and a terminal result are separate retained facts.
//! Workers belong to the runner; recovery never launches them a second time.
use super::{
    failure, id, now,
    store::{self, Record, Store},
    tools, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};

type Worker = (String, JoinHandle<Result<()>>);

#[derive(Clone, Default)]
pub struct Manager(Arc<Workers>);

#[derive(Default)]
struct Workers {
    handles: Mutex<Vec<Worker>>,
    stopping: Arc<AtomicBool>,
}

impl Drop for Workers {
    fn drop(&mut self) {
        // An orderly runner failure must not abandon locally owned processes.
        // SIGKILL/crash remains an uncertain outcome for startup reconciliation.
        self.stopping.store(true, Ordering::SeqCst);
        let handles = self
            .handles
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        for (_, worker) in handles.drain(..) {
            let _ = worker.join();
        }
    }
}

fn active(body: &Value) -> bool {
    matches!(body["state"].as_str(), Some("submitted" | "running"))
}

pub fn owned(db: &Connection, actor: &str, reference: &str) -> Result<Record> {
    let exact = store::exact(db, reference)?;
    if exact.reference.kind != "job" || exact.body["owner"] != actor {
        return Err(failure(
            "expected an exact reference to your own background job",
        ));
    }
    store::required(db, "job", &exact.reference.id)
}

/// Compact current records, without command bodies or accumulated output.
pub fn index(db: &Connection, owner: Option<&str>, unfinished_only: bool) -> Result<Vec<Value>> {
    Ok(store::list(db, "job")?
        .into_iter()
        .filter(|r| owner.is_none_or(|owner| r.body["owner"] == owner))
        .filter(|r| !unfinished_only || active(&r.body) || r.body["state"] == "uncertain")
        .map(|r| {
            json!({"reference":r.reference.handle(),"owner":r.body["owner"],
            "work":r.body["work"],"state":r.body["state"],"invocation":r.body["invocation"],
            "operation":r.body["operation"],"input":r.body["input"],
            "submitted":r.body["submitted"],"started":r.body["started"],
            "finished":r.body["finished"],"result":r.body["result"],
            "cancellation":r.body["cancellation"]})
        })
        .collect())
}

fn uncertain(db: &Connection, job: Record, reason: &str) -> Result<()> {
    if !active(&job.body) {
        return Ok(());
    }
    let mut body = job.body;
    body["state"] = json!("uncertain");
    body["failure"] = json!(reason);
    let call = body["invocation"]
        .as_str()
        .ok_or_else(|| failure("job invocation missing"))?;
    let actor = body["owner"]
        .as_str()
        .ok_or_else(|| failure("job owner missing"))?;
    let work = body["work"]
        .as_str()
        .ok_or_else(|| failure("job work missing"))?;
    let reference = store::write(
        db,
        "job",
        &job.reference.id,
        job.reference.revision,
        &body,
        Some(call),
    )?;
    store::event(
        db,
        work,
        "runtime",
        actor,
        "uncertain_job",
        &json!({
            "job":reference.handle(),"reason":reason,"automatic_retry":false,
            "meaning":"Inspect actual host state before choosing a new action. Process termination and output completeness are unknown."
        }),
        Some(call),
    )?;
    db.execute("INSERT INTO controls VALUES(?1,'paused',1,?2,?3) ON CONFLICT(scope) DO UPDATE SET state='paused',revision=controls.revision+1,reason=excluded.reason,updated=excluded.updated",
        params![format!("persona:{actor}"),reason,now()])?;
    Ok(())
}

pub fn recover(db: &Connection) -> Result<()> {
    for job in store::list(db, "job")? {
        uncertain(
            db,
            job,
            "Interrupted background job requires reconciliation; never replayed automatically",
        )?;
    }
    Ok(())
}

impl Manager {
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self
            .0
            .handles
            .lock()
            .map_err(|_| failure("background worker registry poisoned"))?
            .is_empty())
    }

    pub fn reap(&self, store: &mut Store) -> Result<()> {
        let finished = {
            let mut workers = self
                .0
                .handles
                .lock()
                .map_err(|_| failure("background worker registry poisoned"))?;
            let mut finished = Vec::new();
            let mut i = 0;
            while i < workers.len() {
                if workers[i].1.is_finished() {
                    finished.push(workers.swap_remove(i));
                } else {
                    i += 1;
                }
            }
            finished
        };
        for (id, worker) in finished {
            let failure = match worker.join() {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error.to_string()),
                Err(_) => {
                    Some("background worker panicked; terminal outcome is not established".into())
                }
            };
            if let Some(reason) = failure {
                store.transaction(|db| uncertain(db, store::required(db, "job", &id)?, &reason))?;
            }
        }
        Ok(())
    }

    pub fn submit(
        &self,
        store: &mut Store,
        work: &str,
        actor: &str,
        invocation: &str,
        operation: &str,
        input: &tools::HostCommand,
        directory: PathBuf,
    ) -> Result<Value> {
        let mut workers = self
            .0
            .handles
            .lock()
            .map_err(|_| failure("background worker registry poisoned"))?;
        let job_id = id("job")?;
        let reference = store.transaction(|db| {
            let input_digest = store::put(db, input)?;
            store::write(
                db,
                "job",
                &job_id,
                0,
                &json!({
                    "owner":actor,"work":work,"invocation":invocation,"operation":operation,
                    "input":format!("object:{input_digest}"),"state":"submitted","submitted":now(),
                    "started":null,"finished":null,"process":null,"result":null,"cancellation":null
                }),
                Some(invocation),
            )
        })?;
        let root = store.root.clone();
        let input = input.clone();
        let worker_job = job_id.clone();
        let stopping = self.0.stopping.clone();
        match std::thread::Builder::new()
            .name(format!("persona-{job_id}"))
            .spawn(move || execute(&root, &worker_job, &input, &directory, &stopping))
        {
            Ok(worker) => workers.push((job_id, worker)),
            Err(error) => {
                // No worker was created, so no command was launched by it.
                store.transaction(|db| {
                    let current = store::required(db, "job", &job_id)?;
                    let mut body = current.body;
                    body["state"] = json!("not_started");
                    body["finished"] = json!(now());
                    body["failure"] = json!(error.to_string());
                    store::write(
                        db,
                        "job",
                        &job_id,
                        current.reference.revision,
                        &body,
                        Some(invocation),
                    )?;
                    Ok(())
                })?;
                return Ok(
                    json!({"job":reference.handle(),"submitted":false,"process_started":false,"error":error.to_string()}),
                );
            }
        }
        Ok(json!({"job":reference.handle(),"submitted":true,
            "meaning":"Submission receipt only. host.observe reads current state and optional exact output; completion arrives as a later tool result. Choose ready for independent work or wait for an event. No automatic polling or timeout."}))
    }
}

fn execute(
    root: &Path,
    job_id: &str,
    input: &tools::HostCommand,
    directory: &Path,
    stopping: &AtomicBool,
) -> Result<()> {
    let mut store = Store::open(root)?;
    let initial = store::required(&store.db, "job", job_id)?;
    let actor = initial.body["owner"]
        .as_str()
        .ok_or_else(|| failure("job owner missing"))?
        .to_owned();
    let work = initial.body["work"]
        .as_str()
        .ok_or_else(|| failure("job work missing"))?
        .to_owned();
    let call = initial.body["invocation"]
        .as_str()
        .ok_or_else(|| failure("job invocation missing"))?
        .to_owned();
    let controls = Store::open(root)?;
    let mut launch_store = Store::open(root)?;
    let host = crate::host_command::Inputs {
        command: input.command.clone(),
        stdin: input.stdin.clone().map(crate::host_command::Stdin::Utf8),
        working_directory: input.working_directory.clone(),
        timeout_seconds: input.timeout_seconds,
    };
    let outcome = crate::host_command::execute_with_start(
        &host,
        directory,
        || {
            if stopping.load(Ordering::SeqCst) {
                return Some(json!({"runner_lifecycle":"background worker registry is closing"}));
            }
            if let Some(signal) = crate::process_control::signal() {
                return Some(json!({"signal":signal}));
            }
            let observed = (|| -> Result<Option<Value>> {
                if let Some(control) = store::stopped(&controls.db, &work, &actor)? {
                    return Ok(Some(control));
                }
                use rusqlite::OptionalExtension;
                Ok(controls.db.query_row("SELECT state,reason,revision FROM controls WHERE scope=?1 AND state!='running'",
                [format!("job:{job_id}")], |r| Ok(json!({"job":job_id,"state":r.get::<_,String>(0)?,"reason":r.get::<_,String>(1)?,"revision":r.get::<_,i64>(2)?}))).optional()?)
            })();
            observed.unwrap_or_else(|error| Some(json!({"control_read_failed":error.to_string()})))
        },
        |pid, stdout, stderr| {
            launch_store
                .transaction(|db| {
                    let current = store::required(db, "job", job_id)?;
                    let mut body = current.body;
                    body["state"] = json!("running");
                    body["started"] = json!(now());
                    body["process"] = json!({"pid":pid,"stdout":stdout,"stderr":stderr});
                    let r = store::write(
                        db,
                        "job",
                        job_id,
                        current.reference.revision,
                        &body,
                        Some(&call),
                    )?;
                    store::event(
                        db,
                        &work,
                        &actor,
                        &actor,
                        "job_started",
                        &json!({"job":r.handle(),"process_started":true}),
                        Some(&call),
                    )?;
                    Ok(())
                })
                .map_err(std::io::Error::other)
        },
    );
    let raw_logs = retain_logs(&store.db, &outcome)?;
    let result = json!({"input":input,"outcome":outcome,"raw_logs":raw_logs,
        "context_sources":input.context_sources,"job":initial.reference.handle(),"phase":"terminal"});
    store.transaction(|db| {
        let current = store::required(db, "job", job_id)?;
        let mut body = current.body;
        let digest = store::put(db, &result)?;
        body["state"] = json!("completed");
        body["finished"] = json!(now());
        body["result"] = json!(format!("object:{digest}"));
        let r = store::write(db, "job", job_id, current.reference.revision, &body, Some(&call))?;
        store::event(db, &work, &actor, &actor, "tool_result", &json!({
            "tool":"host.start","operation":body["operation"],"job":r.handle(),
            "result":format!("object:{digest}"),"context":input.result_context,"phase":"terminal"
        }), Some(&call))?;
        Ok(())
    })
}

pub fn retain_logs(db: &Connection, outcome: &crate::host_command::Outcome) -> Result<Vec<Value>> {
    let mut logs = Vec::new();
    for (stream, capture) in [("stdout", &outcome.stdout), ("stderr", &outcome.stderr)] {
        if let Some(path) = &capture.log_path {
            match std::fs::read(path) {
                Ok(bytes) => logs.push(json!({"stream":stream,"object":format!("object:{}",store::put_bytes(db,"application/octet-stream",&bytes)?),"bytes":bytes.len()})),
                Err(error) => logs.push(json!({"stream":stream,"retention_error":error.to_string()})),
            }
        }
    }
    Ok(logs)
}

pub fn cancel(
    db: &Connection,
    actor: &str,
    call: &str,
    input: &tools::HostCancel,
) -> Result<Value> {
    if input.reason.trim().is_empty() {
        return Err(failure("job cancellation requires a reason"));
    }
    let job = owned(db, actor, &input.job)?;
    if !active(&job.body) {
        return Ok(
            json!({"job":job.reference.handle(),"state":job.body["state"],"cancellation_requested":false,
            "meaning":"No locally running worker is known for this state. An uncertain job requires actual host reconciliation."}),
        );
    }
    db.execute("INSERT INTO controls VALUES(?1,'canceled',1,?2,?3) ON CONFLICT(scope) DO UPDATE SET state='canceled',revision=controls.revision+1,reason=excluded.reason,updated=excluded.updated",
        params![format!("job:{}",job.reference.id),input.reason,now()])?;
    let mut body = job.body;
    body["cancellation"] = json!({"requested":now(),"invocation":call,"reason":input.reason});
    let r = store::write(
        db,
        "job",
        &job.reference.id,
        job.reference.revision,
        &body,
        Some(call),
    )?;
    Ok(
        json!({"job":r.handle(),"cancellation_requested":true,"completion_observed":false,
        "meaning":"The owning worker observes this request; consult the later terminal receipt for actual exit and captured output."}),
    )
}

pub fn observe(store: &Store, actor: &str, input: &tools::HostObserve) -> Result<Value> {
    let job = owned(&store.db, actor, &input.job)?;
    let mut value =
        json!({"job":job.reference.handle(),"record":job.body,"observed":now(),"capture":null});
    let Some(stream) = &input.stream else {
        return Ok(value);
    };
    let stream = match stream {
        tools::HostStream::Stdout => "stdout",
        tools::HostStream::Stderr => "stderr",
    };
    // Finished output is read from immutable bytes, not a possibly changed log.
    let retained = if let Some(result) = job.body["result"]
        .as_str()
        .and_then(|r| r.strip_prefix("object:"))
    {
        let result: Value = store::object(&store.db, result)?;
        result["raw_logs"]
            .as_array()
            .and_then(|logs| logs.iter().find(|r| r["stream"] == stream))
            .and_then(|r| r["object"].as_str())
            .map(str::to_owned)
    } else {
        None
    };
    if job.body["state"] == "completed" && retained.is_none() {
        value["capture"] = json!({"available":false,"stream":stream,
            "meaning":"No immutable terminal stream was retained. A mutable host log cannot establish the terminal bytes."});
        return Ok(value);
    }
    let (bytes, observed_length, source) = if let Some(reference) = retained {
        let raw = store::bytes(
            &store.db,
            reference
                .strip_prefix("object:")
                .ok_or_else(|| failure("invalid stream object"))?,
        )?;
        let length = raw.len() as u64;
        let end = input.end.unwrap_or(length);
        if input.start > end || end > length {
            return Err(failure("stream range exceeds observed bytes"));
        }
        (
            raw[input.start as usize..end as usize].to_vec(),
            length,
            json!({"retained":reference}),
        )
    } else if let Some(path) = job.body["process"][stream].as_str() {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let end = input.end.unwrap_or(length);
        if input.start > end || end > length {
            return Err(failure("stream range exceeds observed bytes"));
        }
        file.seek(SeekFrom::Start(input.start))?;
        let mut bytes = Vec::new();
        file.take(end - input.start).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != end - input.start {
            return Err(failure("log changed during range capture"));
        }
        (
            bytes,
            length,
            json!({"host_log":path,"mutable_source":true}),
        )
    } else {
        value["capture"] = json!({"available":false,"stream":stream,"meaning":"No captured log location or retained stream is available."});
        return Ok(value);
    };
    let digest = store::put_bytes(&store.db, "application/octet-stream", &bytes)?;
    value["capture"] = json!({"available":true,"stream":stream,"source":source,
        "reference":format!("object:{digest}"),"start":input.start,"end":input.start+bytes.len() as u64,
        "observed_stream_bytes":observed_length,"text":String::from_utf8_lossy(&bytes),
        "lossy_utf8":std::str::from_utf8(&bytes).is_err(),
        "meaning":"Exact captured range, not evidence of subsequent output or successful completion."});
    Ok(value)
}
