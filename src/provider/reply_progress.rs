//! An opt-in observer sees only the parser's short public reply excerpt. The
//! decision carrier and all parsing state stay inside the dispatching collector.
use super::public_reply::PublicReply;
use std::{
    fmt,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct State {
    bound: bool,
    closed: bool,
    revision: u64,
    reply: Option<(String, bool)>,
}

pub(super) struct ReplyProgress {
    invocation: String,
    state: Mutex<State>,
}

impl fmt::Debug for ReplyProgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReplyProgress")
    }
}

impl ReplyProgress {
    pub(super) fn new(invocation: &str) -> Self {
        Self {
            invocation: invocation.to_owned(),
            state: Mutex::default(),
        }
    }

    pub(super) fn bind(self: &Arc<Self>, invocation: &str, effort: &str) -> Option<Projection> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if invocation != self.invocation || state.bound || state.closed {
            return None;
        }
        state.bound = true;
        Some(Projection {
            observer: Arc::clone(self),
            expected_effort: effort.to_owned(),
            item: None,
        })
    }

    pub(super) fn since(&self, revision: u64) -> Option<(u64, Option<(String, bool)>)> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.revision != revision).then(|| (state.revision, state.reply.clone()))
    }

    fn publish(&self, reply: Option<(&str, bool)>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed || state.reply.as_ref().map(|(s, t)| (s.as_str(), *t)) == reply {
            return;
        }
        state.reply = reply.map(|(s, t)| (s.to_owned(), t));
        state.revision = state.revision.wrapping_add(1);
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        if state.reply.take().is_some() {
            state.revision = state.revision.wrapping_add(1);
        }
    }
}

struct ItemPrefix {
    index: usize,
    bytes: usize,
    finished: bool,
    parser: PublicReply,
}

pub(super) struct Projection {
    observer: Arc<ReplyProgress>,
    pub(super) expected_effort: String,
    item: Option<ItemPrefix>,
}

impl Projection {
    pub(super) fn unbind_item(&mut self) {
        if self.item.take().is_some() {
            self.observer.publish(None);
        }
    }

    pub(super) fn observe_item(&mut self, index: usize, text: &str, complete: bool) {
        if self.item.as_ref().is_none_or(|item| item.index != index) {
            self.unbind_item();
            self.item = Some(ItemPrefix {
                index,
                bytes: 0,
                finished: false,
                parser: PublicReply::new(&self.observer.invocation),
            });
        }
        let item = self.item.as_mut().unwrap();
        // Collector reconciliation makes repeated snapshots identical and a
        // selected carrier's deltas monotonically append at UTF-8 boundaries.
        let mut changed = false;
        if text.len() > item.bytes {
            changed = item.parser.push(&text[item.bytes..]);
            item.bytes = text.len();
        }
        if complete && !item.finished {
            changed |= item.parser.finish();
            item.finished = true;
        }
        if changed {
            self.observer.publish(item.parser.current());
        }
    }

    pub(super) fn terminal(&mut self, text: &str) {
        // A completed response can be the first snapshot that authoritatively
        // resolves the carrier. Validate it even if no incremental item qualified.
        let mut parser = PublicReply::new(&self.observer.invocation);
        parser.push(text);
        parser.finish();
        self.observer.publish(parser.current());
    }
}

impl Drop for Projection {
    fn drop(&mut self) {
        self.observer.close();
    }
}
