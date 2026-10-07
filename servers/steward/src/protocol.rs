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
    ChannelClosedReply, EndSessionReply, ErrorCode, LoginReply, Message, Reply,
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
            | Message::ApprovalClosed(_) => Class::Sshd,
            Message::Pending(_) | Message::Approve(_) | Message::Deny(_) => Class::Approval,
            Message::Blame(_) => Class::Init,
            Message::Submit(_) | Message::StartAgent(_) | Message::EndLease(_) | Message::EndSession(_) => {
                Class::Session
            }
        }
    }
}

/// A label set as the protocol carries it: eight bytes per id, little-endian.
fn label_bytes(ids: &[u64]) -> Vec<u8> { ids.iter().flat_map(|l| l.to_le_bytes()).collect() }

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
    }
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

/// Test-only, for the bench's `steward-restart` (feature `restart-probe`, off in every default
/// build, as `littlefsd`'s and `netd`'s are): a login for this principal ends the steward with
/// [`PROBE_EXIT`] while it holds `sshd`'s call, so `init` empties `users` and restarts it. No
/// principal has the name; only the case's one session logs in as it, once.
#[cfg(feature = "restart-probe")]
pub const PROBE: &str = "steward-restart-probe";
#[cfg(feature = "restart-probe")]
pub const PROBE_EXIT: u32 = 9;

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
        if Class::of(caller.badge) != Class::of_message(&request) {
            return Err(ErrorCode::Malformed);
        }
        let mut login_labels = Vec::new();
        let (kind, ok) = match request {
            Message::Login(m) => {
                #[cfg(feature = "restart-probe")]
                if m.principal == PROBE {
                    redoubt_rt::handle::process_exit(PROBE_EXIT);
                }
                // A label the manifest does not name is one the principal does not own.
                login_labels = self.steward.label(m.label).ok_or(ErrorCode::NotOwner)?;
                let key = <[u8; 32]>::try_from(m.key).map_err(|_| ErrorCode::BadKey)?;
                // The channel's console, which the session's console slot binds to.
                self.kernel.console(handles.first().copied());
                let kind = EventKind::Login {
                    principal: m.principal.into(),
                    labels: login_labels.clone(),
                    key: key_id(&key),
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
            // The core decides these, and the server binds no batch for them until STEWARD3.
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
            (Some(Answer::Session { id, name }), None) => {
                self.name = name;
                self.label_bytes = label_bytes(&login_labels);
                let reply = LoginReply { session: id, name: &self.name, labels: &self.label_bytes };
                Ok(Typed::new(Reply::Login(reply)))
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
