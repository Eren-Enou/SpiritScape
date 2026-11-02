//! Human-readable log explaining what happened and why.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Player,
    Spirit,
    Planner,
    Construction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub tick: u64,
    pub kind: EventKind,
    pub text: String,
    /// Routine detail (a spirit starting a job, going idle). The game hides
    /// these unless the player asks for every event.
    pub detail: bool,
}

#[derive(Clone, Debug)]
pub struct EventLog {
    entries: VecDeque<Event>,
    capacity: usize,
    /// Total events ever pushed (entries may have been dropped).
    pub total: u64,
}

impl EventLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity,
            total: 0,
        }
    }

    pub fn push(&mut self, tick: u64, kind: EventKind, text: impl Into<String>) {
        self.push_entry(tick, kind, text.into(), false);
    }

    /// Like `push`, for routine detail.
    pub fn push_detail(&mut self, tick: u64, kind: EventKind, text: impl Into<String>) {
        self.push_entry(tick, kind, text.into(), true);
    }

    fn push_entry(&mut self, tick: u64, kind: EventKind, text: String, detail: bool) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(Event {
            tick,
            kind,
            text,
            detail,
        });
        self.total += 1;
    }

    /// Oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Event> {
        self.entries.iter()
    }

    pub fn contains(&self, kind: EventKind, needle: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.kind == kind && e.text.contains(needle))
    }
}
