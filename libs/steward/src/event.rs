//! Events: what the embedder hands `decide` (servers/steward.md, "Machines").

use alloc::string::String;
use alloc::vec::Vec;

use crate::consts::RANDOM_WORDS;
use crate::effect::{Object, Produced, ReplyTo, StepFailed};

/// What a request asks for, fixed at submission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// Nothing the steward starts: only its text matters.
    Note { what: String },
    /// An agent labelled `labels`, under the requester's principal, for `lease` µs.
    Agent { labels: Vec<u64>, lease: u64 },
    /// Copy item `item` of the volume of `labels` out to the unlabelled volume.
    Declassify { labels: Vec<u64>, item: u64 },
    /// Copy unlabelled item `source` into item `item` of the volume of `target`.
    Push { source: u64, target: Vec<u64>, item: u64 },
}

/// One event. `now` never goes back; `random` are fresh words from the kernel's generator, for
/// every id the event may need (R36); `reply` answers the caller, if it waits for one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub now: u64,
    pub random: [u64; RANDOM_WORDS],
    pub reply: ReplyTo,
    pub kind: EventKind,
}

/// The events, by the caller's role (the badge class the embedder received it on). A session's
/// or an agent's events name it by its badge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    // From `sshd`.
    Login {
        principal: String,
        labels: Vec<u64>,
        key: u64,
    },
    /// The SSH channel of session `session` closed.
    ChannelClosed {
        session: u64,
    },
    ApprovalOpened {
        channel: u64,
        principal: String,
        key: u64,
    },
    ApprovalClosed {
        channel: u64,
    },
    // From a session or an agent.
    StartAgent {
        badge: u64,
        lease: u64,
    },
    Submit {
        badge: u64,
        content: Content,
        reason: String,
    },
    EndLease {
        badge: u64,
        lease: u64,
    },
    EndSession {
        badge: u64,
    },
    // From an approval channel.
    Pending {
        channel: u64,
    },
    Approve {
        channel: u64,
        request: u64,
        hash: [u8; 32],
    },
    Deny {
        channel: u64,
        request: u64,
    },
    // From `init`.
    Blame {
        account: u64,
        labels: Vec<u64>,
    },
    // From the steward's exit endpoint: the process of a session, a lease or a crossing.
    Exited {
        object: Object,
    },
    // A batch's outcome.
    Done {
        object: Object,
        result: Result<Vec<Produced>, StepFailed>,
    },
}

impl EventKind {
    /// Answered ahead of admission, as the tables mark it: the embedder reads this.
    pub fn ahead(&self) -> bool {
        match self {
            EventKind::EndLease { .. } => crate::gen::lease::Event::EndLease.ahead(),
            _ => false,
        }
    }
}
