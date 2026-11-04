//! One read-only helper keeps decoded history between snapshot requests.
//! Its fixed root/operator binding and native fresh-epoch checks govern reuse.
use super::*;
use std::{process::Child, sync::mpsc};

pub(super) struct Reader {
    args: Vec<String>,
    child: Child,
    responses: mpsc::Receiver<Result<Value>>,
    output: Option<thread::JoinHandle<()>>,
}

impl Reader {
    fn start(helpers: &Helpers, args: &[String]) -> Result<Self> {
        let mut child = Command::new(helpers.copies.path().join("personas-session"))
            .args(args).stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::null()).process_group(0).spawn()
            .map_err(|_| "Could not start the workspace reader.")?;
        let stdout = child.stdout.take().ok_or("Workspace reader output is unavailable.")?;
        let (tx, responses) = mpsc::channel();
        let output = thread::spawn(move || {
            for line in BufReader::new(stdout).split(b'\n') {
                let result = (|| {
                    let mut value = journal::decode(&line.map_err(journal::io)?)?;
                    journal::require(value["version"] == 1, "Unexpected workspace reader version.")?;
                    if value["ok"] != true {
                        let code = value["code"].as_str().filter(|s|
                            s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'));
                        return Err(friendly(code.unwrap_or("helper_unavailable")));
                    }
                    Ok(value["result"].take())
                })();
                let failed = result.is_err();
                if tx.send(result).is_err() || failed { break; }
            }
        });
        Ok(Self { args: args.to_vec(), child, responses, output: Some(output) })
    }

    fn request(&mut self, request: &Value, cancel: &Cancel) -> Result<Value> {
        let stdin = self.child.stdin.as_mut().ok_or("Workspace reader input is unavailable.")?;
        serde_json::to_writer(&mut *stdin, request)
            .map_err(|_| "Could not send the workspace read request.")?;
        writeln!(stdin).and_then(|_| stdin.flush()).map_err(journal::io)?;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("Workspace read superseded by a later action.".into());
            }
            match self.responses.recv_timeout(Duration::from_millis(25)) {
                Ok(value) => return value,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) =>
                    return Err("The workspace reader stopped. Refresh to inspect saved progress.".into()),
            }
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // This child only projects state. Cancelling its read cannot cancel a
        // model invocation or host command in the independent session worker.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(output) = self.output.take() { let _ = output.join(); }
    }
}

impl Helpers {
    pub fn snapshot(&self, args: &[String], request: &Value, cancel: &Cancel) -> Result<Value> {
        let mut slot = self.snapshots.lock().map_err(|_| "Workspace reader lock failed.")?;
        if slot.as_ref().is_some_and(|reader| reader.args != args) { slot.take(); }
        if slot.is_none() { *slot = Some(Reader::start(self, args)?); }
        let result = slot.as_mut().unwrap().request(request, cancel);
        if result.is_err() { slot.take(); }
        result
    }

    pub fn close_snapshot_reader(&self) {
        if let Ok(mut reader) = self.snapshots.lock() { reader.take(); }
    }
}
