//! The context one transition runs in: one domain, the event, and what the transition emits.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use crate::audit::{Audit, Record};
use crate::domain::{Domain, Labels};
use crate::effect::{
    Answer, Effects, Kind, Object, Output, Produced, Refusal, ReplyTo, Step, StepFailed, Token,
};
use crate::event::Event;
use crate::manifest::{Fixed, Principal};
use crate::store::{Crossing, DomainState, Index, Route};

/// A guard: reads the context, changes nothing; `Err` is why it fails.
pub type Guard = fn(&Cx<'_>) -> Result<(), Refusal>;
/// An effect.
pub type Effect = fn(&mut Cx<'_>);
/// Whether a reader with these labels may read this record.
pub type AuditFilter = fn(&Labels, &Audit) -> bool;
/// Whether an approval channel of this principal reaches this domain's requests.
pub type Reach = fn(&Principal, &Domain) -> bool;

/// Where a dispatch sends its object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next<S> {
    /// `=`.
    Stay,
    To(S),
    /// `-`: nothing was created.
    Nothing,
    /// The table has no row for the pair: an event naming an object that does not exist.
    NoRow,
    /// The embedder's guarantee was broken: the steward exits.
    Unreachable,
}

/// An event one machine raises for another: `decide` runs them in order before it returns.
#[derive(Clone, Debug)]
pub(crate) enum Raised {
    /// An approved request starts a lease in `domain`.
    Granted {
        domain: Domain,
        id: u64,
        principal: usize,
        lease: u64,
    },
    /// A request opens a crossing.
    Open {
        domain: Domain,
        crossing: Crossing,
    },
    /// A read crossing's snapshot (bytes, and the reader's kernel id), or its failure, for its
    /// request.
    Snapshot {
        request: Object,
        result: Option<(Vec<u8>, u64)>,
    },
    LockedOut {
        object: Object,
    },
    SessionEnded {
        request: Object,
    },
}

/// What a `decide` emits, and the event's fresh words.
pub(crate) struct Out {
    pub effects: Effects,
    pub raised: VecDeque<Raised>,
    /// The steps of the transition running now: its object's batch.
    pub steps: Vec<Step>,
    /// Ids drawn in this `decide`, not yet in the store.
    pub drawn: Vec<u64>,
}

impl Out {
    pub(crate) fn new() -> Out {
        Out { effects: Effects::default(), raised: VecDeque::new(), steps: Vec::new(), drawn: Vec::new() }
    }
}

/// One transition's context: the fixed part, the routing index, the one domain the event is
/// about and its object, and the output. `grant` is set only by the request and approval path.
pub struct Cx<'a> {
    pub(crate) fixed: &'a Fixed,
    pub(crate) index: &'a mut Index,
    pub(crate) used: &'a BTreeMap<u64, Domain>,
    pub(crate) domain: &'a Domain,
    pub(crate) state: &'a mut DomainState,
    /// The domain an approved request would start a lease in, read by `not_locked`.
    pub(crate) grant: Option<&'a DomainState>,
    pub(crate) event: &'a Event,
    /// The session or lease the event came from, if one.
    pub(crate) caller: Option<Route>,
    /// The approval channel the event came on, if one.
    pub(crate) channel: Option<u64>,
    /// The object: its id in its machine.
    pub(crate) kind: Kind,
    pub(crate) id: u64,
    /// A batch's outcome, for `Done` and `Failed`.
    pub(crate) result: Option<&'a Result<Vec<Produced>, StepFailed>>,
    /// The call a reply answers.
    pub(crate) reply: ReplyTo,
    /// Why the last `!guard` failed.
    pub(crate) reason: Option<Refusal>,
    pub(crate) out: &'a mut Out,
}

/// What a guard or an effect may read and emit: enough to write the model's broken ones, which
/// stand in a `Policy` the model makes; the shipped crate has no mutation switch.
impl<'a> Cx<'a> {
    pub fn fixed(&self) -> &Fixed { self.fixed }

    pub fn index(&self) -> &Index { self.index }

    /// The one domain the transition is about, and its state.
    pub fn domain(&self) -> &Domain { self.domain }

    pub fn state(&self) -> &DomainState { self.state }

    pub fn event(&self) -> &Event { self.event }

    pub fn now(&self) -> u64 { self.event.now }

    /// The session or lease the event came from, if one.
    pub fn caller(&self) -> Option<&Route> { self.caller.as_ref() }

    /// The approval channel the event came on, if one.
    pub fn channel(&self) -> Option<u64> { self.channel }

    pub fn kind(&self) -> Kind { self.kind }

    pub fn id(&self) -> u64 { self.id }

    pub fn object(&self) -> Object { Object { domain: self.domain.clone(), kind: self.kind, id: self.id } }

    /// A name for something this object's steps make.
    pub fn token(&self, slot: u8) -> Token { Token { owner: self.object(), slot } }

    /// A step of this object's batch.
    pub fn step(&mut self, step: Step) { self.out.steps.push(step); }

    pub fn output(&mut self, output: Output) { self.out.effects.outputs.push(output); }

    /// A `!guard` failed for `reason`: the row's refusal takes it.
    pub(crate) fn refused(&mut self, reason: Refusal) { self.reason = Some(reason); }

    pub fn reply(&mut self, answer: Answer) {
        if self.reply != 0 {
            let to = self.reply;
            self.output(Output::Reply { to, answer });
        }
    }

    /// A record in this domain.
    pub fn audit(&mut self, record: Record) {
        let a = Audit::new(self.domain, self.now(), record);
        self.output(Output::Audit(a));
    }

    /// A record in another domain: one stamped with the side it must be read under.
    pub fn audit_in(&mut self, domain: &Domain, record: Record) {
        let a = Audit::new(domain, self.now(), record);
        self.output(Output::Audit(a));
    }

    /// A fresh id from the event's random words: never 0 and naming nothing yet. When the words
    /// run out the embedder broke its guarantee, and the steward exits.
    pub fn fresh(&mut self) -> u64 {
        for w in self.event.random {
            if self.index.fresh(w) && !self.used.contains_key(&w) && !self.out.drawn.contains(&w) {
                self.out.drawn.push(w);
                return w;
            }
        }
        self.out.effects.exit = true;
        0
    }

    /// The bytes a batch's read produced, and the kernel id of the budget it made first.
    pub fn produced(&self) -> (Option<Vec<u8>>, u64) {
        let Some(Ok(list)) = self.result else { return (None, 0) };
        let bytes = list.iter().find_map(|p| match p {
            Produced::Bytes(b) => Some(b.clone()),
            _ => None,
        });
        let budget = list.iter().find_map(|p| match p {
            Produced::Budget(b) => Some(*b),
            _ => None,
        });
        (bytes, budget.unwrap_or(0))
    }
}
