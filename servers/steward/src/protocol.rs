//! The steward's protocol (`libs/wire/tables/steward.md`; servers/steward.md, "The steward's
//! protocol"): each call decoded, checked against its badge class, turned into the core's event,
//! and answered with what the core replied.
//!
//! **Badge classes.** The steward's root badges each name one caller role, handed by `init` from
//! the manifest: [`SSHD`], [`APPROVAL`] and [`INIT`]. Every badge from [`MINTED`] up is one the
//! steward minted for a session or an agent, which the core routes. An operation on a badge of
//! another class is malformed, the same answer as an unknown opcode, so a session cannot send
//! `login`, `approve` or `blame`.

use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::abi::{Handle as KernelHandle, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Words};
use redoubt_rt::server::typed::{Answer as Typed, Outcome, Protocol, TypedServer, answer};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::steward::{
    ChannelClosedReply, ContextsReply, DetachReply, EndContextReply, EndLeaseReply, EndSessionReply,
    ErrorCode, LoginReply, Message, Reply, StartAgentReply,
};
use redoubt_steward::effect::{Answer, Output, Refusal};
use redoubt_steward::event::EventKind;
use redoubt_steward::hash::key_id;

use crate::drive::{Exited, MINTED};
use crate::{Kernel, Steward};

/// `sshd`'s root badge: logins, channel ends and approval channels opening and closing.
pub const SSHD: u64 = 1;
/// The approval channel's root badge: what is pending, approve and deny.
pub const APPROVAL: u64 = 2;
/// `init`'s root badge: crash blame.
pub const INIT: u64 = 3;

/// The caller roles, by badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Sshd,
    Approval,
    Init,
    /// A session's or an agent's minted badge.
    Session,
    /// Any other badge: no operation is accepted on it.
    None,
}

impl Class {
    pub fn of(badge: u64) -> Class {
        match badge {
            SSHD => Class::Sshd,
            APPROVAL => Class::Approval,
            INIT => Class::Init,
            b if b >= MINTED => Class::Session,
            _ => Class::None,
        }
    }

    /// The class an operation belongs to.
    pub fn of_message(m: &Message<'_>) -> Class {
        match m {
            Message::Login(_)
            | Message::ChannelClosed(_)
            | Message::ApprovalOpened(_)
            | Message::ApprovalClosed(_)
            | Message::Watch(_) => Class::Sshd,
            Message::Pending(_) | Message::Approve(_) | Message::Deny(_) => Class::Approval,
            Message::Blame(_) => Class::Init,
            Message::Submit(_)
            | Message::StartAgent(_)
            | Message::EndLease(_)
            | Message::EndSession(_)
            | Message::Contexts(_)
            | Message::Detach(_)
            | Message::EndContext(_) => Class::Session,
            // The steward's own send to a session's notices endpoint: a call of it, on any badge,
            // is malformed.
            Message::LeaseEnded(_) => Class::None,
        }
    }
}

/// Whether a call is `sshd`'s `watch` on its root badge: the program holds it, unanswered, for
/// as long as it runs, so its end (R4b: `Dead` when the steward dies) is how `sshd` learns every
/// session it knew is over (servers/steward.md, "Failure and restart"). On any other badge it is
/// malformed, as every message on the wrong class is ([`answer_with`]).
pub fn watches(caller: &Caller, words: &Words) -> bool {
    Class::of(caller.badge) == Class::Sshd && matches!(Message::decode(words, &[], 0), Ok(Message::Watch(_)))
}

/// Whether a call is a sponsor's `end_lease`, which the core answers ahead of admission
/// (`EventKind::ahead`; servers/serving.md, "`admit`"): the steward's loop answers every call as
/// it arrives and parks none, so the one it must never hold back is never held back; this names
/// it, so a loop that one day parks its calls can take it first. Only a minted badge's counts:
/// on another class the call is malformed, as every message on the wrong class is.
pub fn ahead(caller: &Caller, words: &Words) -> bool {
    Class::of(caller.badge) == Class::Session
        && match Message::decode(words, &[], 0) {
            Ok(Message::EndLease(m)) => EventKind::EndLease { badge: caller.badge, lease: m.lease }.ahead(),
            _ => false,
        }
}

/// A label set as the protocol carries it ([`redoubt_rt::wire::labels`]).
fn label_bytes(ids: &[u64]) -> Vec<u8> { redoubt_rt::wire::labels::encode(ids).collect() }

fn error(r: Refusal) -> ErrorCode {
    match r {
        Refusal::Unknown => ErrorCode::Unknown,
        Refusal::BadKey => ErrorCode::BadKey,
        Refusal::NotOwner => ErrorCode::NotOwner,
        Refusal::Labelled => ErrorCode::Labelled,
        Refusal::Cap => ErrorCode::Cap,
        Refusal::TooBig => ErrorCode::TooBig,
        Refusal::NotPrintable => ErrorCode::NotPrintable,
        Refusal::NotRendered => ErrorCode::NotRendered,
        Refusal::HashMismatch => ErrorCode::HashMismatch,
        Refusal::BadLease => ErrorCode::BadLease,
        Refusal::LockedOut => ErrorCode::LockedOut,
        Refusal::NotSponsor => ErrorCode::NotSponsor,
        Refusal::Failed => ErrorCode::Failed,
        Refusal::InUse => ErrorCode::InUse,
    }
}

/// A listing as `contexts` carries it: one line per context, `name<TAB>attached|detached<TAB>`
/// and the whole seconds since it last started running, attached or detached.
pub fn listing(list: &[redoubt_steward::effect::Listed]) -> String {
    let mut s = String::new();
    for c in list {
        let state = if c.attached { "attached" } else { "detached" };
        s.push_str(&alloc::format!("{}\t{state}\t{}\n", c.name, c.age / 1_000_000));
    }
    s
}

/// The protocol, for `redoubt-rt`'s typed dispatch.
pub struct StewardProtocol;

impl Protocol for StewardProtocol {
    type Error = ErrorCode;
    type Reply<'a> = Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// One call's serving: the steward, the kernel, and what the call left to deliver.
pub struct Serving<'a, K: Kernel> {
    pub steward: &'a mut Steward<K::Budget, K::Handle>,
    pub kernel: &'a mut K,
    /// The outputs other than this call's reply: audit records, notices.
    pub outputs: Vec<Output>,
    /// The core exited: the server ends once this call is answered.
    pub exited: bool,
    /// The reply's strings, which the reply borrows.
    name: String,
    label_bytes: Vec<u8>,
}

impl<'a, K: Kernel> Serving<'a, K> {
    pub fn new(steward: &'a mut Steward<K::Budget, K::Handle>, kernel: &'a mut K) -> Self {
        Serving {
            steward,
            kernel,
            outputs: Vec::new(),
            exited: false,
            name: String::new(),
            label_bytes: Vec::new(),
        }
    }
}

/// The reply token every call gets: the core answers it, at once or when its batch is done, and
/// the server runs each batch before it receives again.
const CALL: u64 = 1;

impl<K: Kernel> TypedServer<StewardProtocol> for Serving<'_, K>
where
    K::Budget: Copy,
    K::Handle: Copy,
{
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        handles: &[KernelHandle],
    ) -> Result<Typed<Reply<'s>>, ErrorCode> {
        let class = Class::of(caller.badge);
        if class == Class::None || class != Class::of_message(&request) {
            return Err(ErrorCode::Malformed);
        }
        let mut login_labels = Vec::new();
        let (kind, ok) = match request {
            Message::Login(m) => {
                // The key's id first, whatever else the login names; a label the manifest does
                // not name is then refused as a wrong key is, as the core refuses an unknown
                // principal or context (servers/steward.md, "Contexts").
                let key = <[u8; 32]>::try_from(m.key).map_err(|_| ErrorCode::BadKey)?;
                let key = key_id(&key);
                login_labels = self.steward.label(m.label).ok_or(ErrorCode::BadKey)?;
                // The channel's two consoles: the steward's, and the one its relay is given.
                self.kernel.console(handles.first().copied(), handles.get(1).copied());
                let kind = EventKind::Login {
                    principal: m.principal.into(),
                    labels: login_labels.clone(),
                    context: m.context.into(),
                    key,
                    from: m.from.into(),
                };
                (kind, None)
            }
            Message::ChannelClosed(m) => (
                EventKind::ChannelClosed { session: m.session },
                Some(Reply::ChannelClosed(ChannelClosedReply {})),
            ),
            Message::EndSession(_) => {
                (EventKind::EndSession { badge: caller.badge }, Some(Reply::EndSession(EndSessionReply {})))
            }
            // The session's contexts (servers/steward.md, "Contexts"): its listing, its own
            // detach, and its end of a context of its domain.
            Message::Contexts(_) => (EventKind::Contexts { badge: caller.badge }, None),
            Message::Detach(_) => {
                (EventKind::Leave { badge: caller.badge }, Some(Reply::Detach(DetachReply {})))
            }
            Message::EndContext(m) => (
                EventKind::EndContext { badge: caller.badge, name: m.name.into() },
                Some(Reply::EndContext(EndContextReply {})),
            ),
            // An agent on a lease (servers/steward.md, "Leases"): started from an unlabelled
            // session's badge, for `lease` µs, answered with the lease once its batch is done;
            // ended from any unlabelled session of its sponsor, which the core answers at once
            // and the loop never parks ([`ahead`]).
            Message::StartAgent(m) => (EventKind::StartAgent { badge: caller.badge, lease: m.lease }, None),
            Message::EndLease(m) => (
                EventKind::EndLease { badge: caller.badge, lease: m.lease },
                Some(Reply::EndLease(EndLeaseReply {})),
            ),
            // The powerbox's and crash blame's: the core decides them, and the server binds
            // their batches in STEWARD3B and STEWARD3C.
            _ => return Err(ErrorCode::Unknown),
        };
        let outputs = match self.steward.event(self.kernel, kind, CALL) {
            Ok(outputs) => outputs,
            Err(Exited) => {
                self.exited = true;
                return Err(ErrorCode::Failed);
            }
        };
        let mut answer = None;
        for o in outputs {
            match o {
                Output::Reply { to: CALL, answer: a } => answer = Some(a),
                o => self.outputs.push(o),
            }
        }
        match (answer, ok) {
            (Some(Answer::Contexts(list)), None) => {
                self.name = listing(&list);
                Ok(Typed::new(Reply::Contexts(ContextsReply { list: &self.name })))
            }
            (Some(Answer::Session { id, name }), None) => {
                self.name = name;
                self.label_bytes = label_bytes(&login_labels);
                let reply = LoginReply { session: id, name: &self.name, labels: &self.label_bytes };
                Ok(Typed::new(Reply::Login(reply)))
            }
            (Some(Answer::Lease { id, name }), None) => {
                self.name = name;
                Ok(Typed::new(Reply::StartAgent(StartAgentReply { lease: id, name: &self.name })))
            }
            // A notice the core takes without an answer (a channel's close) is answered empty;
            // the core answers only its refusal.
            (Some(Answer::Ok) | None, Some(reply)) => Ok(Typed::new(reply)),
            (Some(Answer::Refused(r)), _) => Err(error(r)),
            // An answer this operation has no reply for, or none: the core's unknown.
            _ => Err(ErrorCode::Unknown),
        }
    }
}

/// Answers one call without a system call but those `kernel` makes: what the program's loop
/// calls, and the entry host tests drive.
pub fn answer_with<K: Kernel>(
    serving: &mut Serving<'_, K>,
    caller: &Caller,
    words: &Words,
    handles: &ReceivedHandles,
    buf: &mut [u8],
) -> Outcome
where
    K::Budget: Copy,
    K::Handle: Copy,
{
    answer::<StewardProtocol, _>(serving, caller, words, handles, buf)
}
