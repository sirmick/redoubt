//! Effects: what `decide` asks the embedder to do (servers/steward.md, "One decision function").
//!
//! Effects are data. The steps that make or destroy something form batches, one per object
//! that waits on it, which the embedder runs in order, stopping at the first failure and
//! reporting the batch as one `Done` event about its owner. Replies, notices, screens and audit
//! records are outputs: they never fail a batch.

use alloc::string::String;
use alloc::vec::Vec;

use crate::audit::Audit;
use crate::domain::{Domain, Labels};
use crate::manifest::Limits;

/// The embedder's token for answering one call; 0 is none.
pub type ReplyTo = u64;

/// What kind of object a machine holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Session,
    Lease,
    Request,
    Crossing,
    /// A domain's blame: one per domain, id 0.
    Blame,
    /// An approval channel, outside every domain.
    Channel,
}

/// One object of a domain.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Object {
    pub domain: Domain,
    pub kind: Kind,
    pub id: u64,
}

/// Something a step makes, named by its owner and a slot: a later step, or a later batch of
/// the same owner, uses it by this name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Token {
    pub owner: Object,
    pub slot: u8,
}

/// A revocation scope: a budget with zero limits, made only by `CreateScope`. A connection is
/// narrowed to one of these, never to a budget that holds processes (R41).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope(Token);

impl Scope {
    pub fn token(&self) -> &Token { &self.0 }
}

/// Where a budget is carved from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parent {
    /// A domain's fixed sub-budget.
    Sub(Domain),
    /// A budget a step made: an agent's, for its sub-agent.
    Budget(Token),
    /// The `users` budget the steward holds: a reader or writer budget.
    Users,
}

/// A server a namespace holds a connection to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    /// The steward's own endpoint: the badge routes the session's requests.
    Steward,
    /// One of the manifest's shared servers.
    Shared(u16),
}

/// The bytes a write writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bytes {
    Literal(Vec<u8>),
    /// What an earlier step of the batch read.
    Read(Token),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    CreateBudget {
        token: Token,
        parent: Parent,
        limits: Limits,
        labels: Labels,
        deadline: Option<u64>,
    },
    /// A zero-limit budget inside `budget`: a revocation scope.
    CreateScope {
        scope: Scope,
        budget: Token,
    },
    Connect {
        token: Token,
        scope: Scope,
        server: Server,
        badge: u64,
    },
    Launch {
        token: Token,
        budget: Token,
        connections: Vec<Token>,
    },
    DestroyBudget {
        budget: Token,
    },
    /// Read one item of the volume of `labels`, through a budget carrying them, or (`None`) the
    /// steward's own read.
    Read {
        token: Token,
        through: Option<Token>,
        labels: Labels,
        item: u64,
    },
    /// Write one item, the same way.
    Write {
        through: Option<Token>,
        labels: Labels,
        item: u64,
        bytes: Bytes,
    },
}

impl Step {
    /// The scope step: the one place a `Scope` is made.
    pub(crate) fn scope(token: Token, budget: Token) -> Step {
        Step::CreateScope { scope: Scope(token), budget }
    }
}

/// The steps one object waits on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub owner: Object,
    pub steps: Vec<Step>,
}

/// What one step made, as `Done` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Produced {
    /// A budget: the kernel's id for it, which audit records name.
    Budget(u64),
    Scope,
    Connection,
    Process(u64),
    Bytes(Vec<u8>),
    /// A step that makes nothing (a destroy, a write).
    Done,
}

/// The step that failed, and the kernel's error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepFailed {
    pub step: usize,
    pub error: u32,
}

/// Why the steward said no. An unknown object and one the caller may not name get the same
/// answer, `Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Unknown,
    /// Not a key for this role, or one `keyd` holds.
    BadKey,
    /// The labels' owner is needed.
    NotOwner,
    /// A labelled caller only submits requests.
    Labelled,
    /// The domain's pending cap, or the caller's fair share of it.
    Cap,
    TooBig,
    NotPrintable,
    /// Not rendered on this channel.
    NotRendered,
    HashMismatch,
    /// A lease of 0 or over `MAX_LEASE`.
    BadLease,
    /// Locked out by crash blame until its window passes.
    LockedOut,
    /// Only an unlabelled session of the lease's sponsor ends it.
    NotSponsor,
    /// A step of the batch failed.
    Failed,
}

/// The answer to a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Ok,
    Session { id: u64, name: String },
    Lease { id: u64, name: String },
    Request { id: u64 },
    Refused(Refusal),
}

/// Who a notice goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notified {
    /// A session, by its badge.
    Session(u64),
    /// An approval channel.
    Channel(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// An approval is waiting; which one only the approval channel shows.
    ApprovalWaiting,
    /// A lease of the sponsor ended; not why.
    LeaseEnded { lease: u64 },
}

/// A request as an approval channel shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub id: u64,
    pub hash: [u8; 32],
    pub labels: Labels,
    /// Printable ASCII only.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Reply {
        to: ReplyTo,
        answer: Answer,
    },
    Notice {
        to: Notified,
        notice: Notice,
    },
    Screen {
        channel: u64,
        screen: Rendered,
    },
    Audit(Audit),
    /// The object is gone: the embedder drops what it holds for its tokens.
    Forget(Object),
}

/// One event's effects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub outputs: Vec<Output>,
    pub batches: Vec<Batch>,
    /// An event the embedder's guarantee excludes arrived: a steward bug. The server exits (fail
    /// closed) and `init` restarts it; nothing else here is carried out.
    pub exit: bool,
}
