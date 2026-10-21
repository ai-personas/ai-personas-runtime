//! Foreground-owned host workers. Only the coordinator accesses Runtime/Store.
use super::*;
use std::ops::{Deref, DerefMut};
use std::thread::JoinHandle;

enum Worker {
    Running(JoinHandle<Terminal>),
    Ready(Terminal),
    Lost,
}

#[derive(Default)]
pub(in crate::runtime) struct Background {
    jobs: BTreeMap<String, Worker>,
}

impl Background {
    pub(super) fn contains(&self, id: &str) -> bool {
        self.jobs.contains_key(id)
    }

    pub(super) fn spawn(
        &mut self,
        id: &str,
        execute: impl FnOnce() -> Terminal + Send + 'static,
    ) -> Result<()> {
        require(!self.contains(id), "host command worker already exists")?;
        let worker = std::thread::Builder::new()
            .name("persona-host-command".into())
            .spawn(execute)?;
        self.jobs.insert(id.to_owned(), Worker::Running(worker));
        Ok(())
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        // Cancellation or unwinding cannot detach a worker from its execution
        // owner. Missing durable outcomes remain uncertain; never rerun them.
        for (_, worker) in std::mem::take(&mut self.jobs) {
            if let Worker::Running(handle) = worker {
                let _ = handle.join();
            }
        }
    }
}

/// Declared after and borrowing the owner, so even a cancelled run joins its
/// host workers before that owner can be released. No worker can mutate Store.
pub(in crate::runtime) struct Scope<'a> {
    app: &'a mut Runtime,
    _owner: &'a s::ExecutionOwner,
}

impl Deref for Scope<'_> {
    type Target = Runtime;
    fn deref(&self) -> &Runtime {
        self.app
    }
}

impl DerefMut for Scope<'_> {
    fn deref_mut(&mut self) -> &mut Runtime {
        self.app
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        drop(self.app.host_command_background.take());
    }
}

impl Runtime {
    pub(in crate::runtime) fn background_host_commands<'a>(
        &'a mut self,
        owner: &'a s::ExecutionOwner,
    ) -> Result<Scope<'a>> {
        require(
            self.host_command_background.is_none(),
            "host command scope already active",
        )?;
        self.host_command_background = Some(Background::default());
        Ok(Scope {
            app: self,
            _owner: owner,
        })
    }

    pub(in crate::runtime) fn has_background_host_commands(&self) -> bool {
        self.host_command_background
            .as_ref()
            .is_some_and(|jobs| !jobs.jobs.is_empty())
    }

    /// Called between decisions only: result publication advances semantic state.
    pub(in crate::runtime) fn publish_completed_host_commands(
        &mut self,
        now: i64,
    ) -> Result<usize> {
        let ready: Vec<_> = self
            .host_command_background
            .iter()
            .flat_map(|jobs| &jobs.jobs)
            .filter(|(_, worker)| match worker {
                Worker::Running(handle) => handle.is_finished(),
                _ => true,
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ready {
            let worker = self
                .host_command_background
                .as_mut()
                .unwrap()
                .jobs
                .get_mut(id)
                .unwrap();
            if matches!(worker, Worker::Running(_)) {
                let Worker::Running(handle) = std::mem::replace(worker, Worker::Lost) else {
                    unreachable!()
                };
                *worker = match handle.join() {
                    Ok(terminal) => Worker::Ready(terminal),
                    Err(_) => Worker::Lost,
                };
            }
            if let Worker::Ready(terminal) =
                &self.host_command_background.as_ref().unwrap().jobs[id]
            {
                let terminal = terminal.clone();
                // Preserve Ready on persistence failure; a second attempt is
                // the same immutable terminal evidence, never command execution.
                self.record_host_command_terminal(&terminal)?;
            }
            self.host_command_background
                .as_mut()
                .unwrap()
                .jobs
                .remove(id);
            // A worker panic has no authenticated outcome. Existing settlement
            // records uncertainty and retains the reservation in that case.
            self.advance_host_command(id, now, tools::ExecutionPhase::Settle)?;
        }
        Ok(ready.len())
    }

    /// No repeated Store reads, scheduling debits, model calls or imposed timeout
    /// while awaiting an actual process. A ready peer is considered first.
    pub(in crate::runtime) async fn wait_for_host_command(&mut self) -> Result<()> {
        while self.has_background_host_commands() {
            if self.publish_completed_host_commands(unix_now()?)? > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        Ok(())
    }
}
