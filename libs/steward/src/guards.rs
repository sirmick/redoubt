//! The guards the tables name (servers/steward.md, "Guards and effects"). Each reads the
//! context and changes nothing; `Err` says why it fails. A guard that carries a rule has a
//! mutation in the model; one that reads only an object's kind has none.

use crate::audit::Audit;
use crate::consts::{BLAME_COUNT, BLAME_WINDOW, DECLASSIFY_MAX, MAX_LEASE, PENDING_CAP};
use crate::cx::Cx;
use crate::domain::Labels;
use crate::effect::{Kind, Refusal};
use crate::event::{Content, EventKind};
use crate::store::{CrossingKind, Lease, Request, Session};

type Verdict = Result<(), Refusal>;

fn ok_if(holds: bool, otherwise: Refusal) -> Verdict { if holds { Ok(()) } else { Err(otherwise) } }

pub fn session<'a>(cx: &'a Cx<'_>) -> Option<&'a Session> { cx.state.sessions.get(&cx.id) }

pub fn lease<'a>(cx: &'a Cx<'_>) -> Option<&'a Lease> { cx.state.leases.get(&cx.id) }

pub fn request<'a>(cx: &'a Cx<'_>) -> Option<&'a Request> { cx.state.requests.get(&cx.id) }

/// A request's labels as a set, or `None` if they are not one.
fn set(labels: &[u64]) -> Option<Labels> { Labels::new(labels) }

/// The labels a request asks for, beyond its own: a labelled agent's, a declassified item's, a
/// push's target.
pub fn asked(r: &Request) -> Option<Labels> {
    match &r.content {
        Content::Note { .. } => Some(Labels::empty()),
        Content::Agent { labels, .. } | Content::Declassify { labels, .. } => set(labels),
        Content::Push { target, .. } => set(target),
    }
}

/// A login uses one of the principal's login keys, never one `keyd` holds.
pub fn login_key(cx: &Cx<'_>) -> Verdict {
    let s = session(cx).ok_or(Refusal::Unknown)?;
    let p = &cx.fixed.principals[s.principal];
    ok_if(p.login_keys.contains(&s.key) && !cx.fixed.keyd.contains(&s.key), Refusal::BadKey)
}

/// An approval channel uses one of the principal's approval keys, never a login key or one
/// `keyd` holds.
pub fn approval_key(cx: &Cx<'_>) -> Verdict {
    let c = cx.index.channels.get(&cx.id).ok_or(Refusal::Unknown)?;
    let p = &cx.fixed.principals[c.principal];
    let login = cx.fixed.principals.iter().any(|q| q.login_keys.contains(&c.key));
    ok_if(p.approval_keys.contains(&c.key) && !login && !cx.fixed.keyd.contains(&c.key), Refusal::BadKey)
}

/// A vault login, a labelled agent, a declassification or a push needs the labels' owner, read
/// from the manifest's owned labels, never from a domain's existence.
pub fn owns_labels(cx: &Cx<'_>) -> Verdict {
    let (principal, wanted) = match cx.kind {
        Kind::Session => (session(cx).ok_or(Refusal::Unknown)?.principal, Labels::empty()),
        Kind::Request => {
            let r = request(cx).ok_or(Refusal::Unknown)?;
            (r.principal, asked(r).ok_or(Refusal::NotOwner)?)
        }
        _ => return Err(Refusal::Unknown),
    };
    let owned = &cx.fixed.principals[principal].owned;
    ok_if(owned.includes(cx.domain.labels()) && owned.includes(&wanted), Refusal::NotOwner)
}

/// A labelled session or agent starts nothing: the lease is asked for from the unlabelled
/// domain.
pub fn caller_unlabelled(cx: &Cx<'_>) -> Verdict { ok_if(cx.domain.labels().is_empty(), Refusal::Labelled) }

/// No new session or lease while the domain is locked out (R40); for an approval, the domain
/// the grant would start a lease in. In the blame machine, it says the window has passed.
pub fn not_locked(cx: &Cx<'_>) -> Verdict {
    let blame = match (cx.kind, cx.grant) {
        (Kind::Request, Some(grant)) => &grant.blame,
        _ => &cx.state.blame,
    };
    if cx.kind == Kind::Request && !matches!(request(cx).map(|r| &r.content), Some(Content::Agent { .. })) {
        return Ok(());
    }
    ok_if(cx.now() >= blame.until, Refusal::LockedOut)
}

/// R40: this blame is the `BLAME_COUNT`th within `BLAME_WINDOW`.
pub fn blame_window(cx: &Cx<'_>) -> Verdict {
    let now = cx.now();
    let recent = cx.state.blame.times.iter().filter(|t| now.saturating_sub(**t) < BLAME_WINDOW).count();
    ok_if(recent + 1 >= BLAME_COUNT, Refusal::Unknown)
}

/// The pending cap per domain.
pub fn pending_cap(cx: &Cx<'_>) -> Verdict {
    let pending = cx.state.requests.values().filter(|r| r.id != cx.id && !r.state.is_final()).count();
    ok_if(pending < PENDING_CAP, Refusal::Cap)
}

/// A fair share of the cap per session or agent: the cap divided among the domain's live
/// sessions and agents, at least one.
pub fn fair_share(cx: &Cx<'_>) -> Verdict {
    let r = request(cx).ok_or(Refusal::Unknown)?;
    let mine =
        cx.state.requests.values().filter(|o| o.id != cx.id && o.by == r.by && !o.state.is_final()).count();
    let live = cx.state.sessions.len() + cx.state.leases.len();
    ok_if(mine < (PENDING_CAP / live.max(1)).max(1), Refusal::Cap)
}

/// R39: a lease of more than 0 and at most `MAX_LEASE`, asked for directly or in a request.
pub fn lease_bounded(cx: &Cx<'_>) -> Verdict {
    let asked = match cx.kind {
        Kind::Lease => lease(cx).ok_or(Refusal::Unknown)?.lease,
        Kind::Request => match &request(cx).ok_or(Refusal::Unknown)?.content {
            Content::Agent { lease, .. } => *lease,
            _ => return Ok(()),
        },
        _ => return Err(Refusal::Unknown),
    };
    ok_if(asked > 0 && asked <= MAX_LEASE, Refusal::BadLease)
}

/// R42: a declassification is submitted from a session with exactly the item's labels, a push
/// from an unlabelled one. Each crosses a label: an unlabelled item is neither declassified nor
/// pushed (its record, read unlabelled, would name a crossing budget).
pub fn exact_labels(cx: &Cx<'_>) -> Verdict {
    match &request(cx).ok_or(Refusal::Unknown)?.content {
        Content::Declassify { labels, .. } => {
            let exact = set(labels).as_ref() == Some(cx.domain.labels());
            ok_if(exact && !cx.domain.labels().is_empty(), Refusal::NotOwner)
        }
        Content::Push { target, .. } => {
            ok_if(cx.domain.labels().is_empty() && !target.is_empty(), Refusal::NotOwner)
        }
        _ => Ok(()),
    }
}

/// R42: a declassified item is at most `DECLASSIFY_MAX` bytes of printable text.
pub fn item_fits(cx: &Cx<'_>) -> Verdict {
    let r = request(cx).ok_or(Refusal::Unknown)?;
    let bytes = r.snapshot.as_deref().unwrap_or_default();
    if bytes.len() > DECLASSIFY_MAX {
        return Err(Refusal::TooBig);
    }
    ok_if(crate::render::printable(bytes), Refusal::NotPrintable)
}

/// R38's screens: a request is shown only on its principal's channels, and a labelled one only
/// to an owner of every label it carries.
pub fn may_see(cx: &Cx<'_>) -> Verdict {
    let r = request(cx).ok_or(Refusal::Unknown)?;
    let c = cx.channel.and_then(|c| cx.index.channels.get(&c)).ok_or(Refusal::Unknown)?;
    let owned = &cx.fixed.principals[c.principal].owned;
    ok_if(c.principal == r.principal && owned.includes(cx.domain.labels()), Refusal::Unknown)
}

/// R38's binding: answered only on the channel that rendered the request last.
pub fn rendered_here(cx: &Cx<'_>) -> Verdict {
    let r = request(cx).ok_or(Refusal::Unknown)?;
    ok_if(r.channel.is_some() && r.channel == cx.channel, Refusal::NotRendered)
}

/// R38's binding: the approval names the frozen request's hash.
pub fn hash_matches(cx: &Cx<'_>) -> Verdict {
    let r = request(cx).ok_or(Refusal::Unknown)?;
    match &cx.event.kind {
        EventKind::Approve { hash, .. } => ok_if(*hash == r.hash, Refusal::HashMismatch),
        _ => Err(Refusal::HashMismatch),
    }
}

/// R38's bound: an approval grants no label the approver lacks.
pub fn approver_holds(cx: &Cx<'_>) -> Verdict {
    let r = request(cx).ok_or(Refusal::Unknown)?;
    let c = cx.channel.and_then(|c| cx.index.channels.get(&c)).ok_or(Refusal::Unknown)?;
    let owned = &cx.fixed.principals[c.principal].owned;
    let asked = asked(r).ok_or(Refusal::NotApprover)?;
    ok_if(
        c.principal == r.principal && owned.includes(cx.domain.labels()) && owned.includes(&asked),
        Refusal::NotApprover,
    )
}

/// A lease is ended only from an unlabelled session of its sponsor.
pub fn sponsor_session(cx: &Cx<'_>) -> Verdict {
    let by = cx.caller.as_ref().ok_or(Refusal::NotSponsor)?;
    let sponsor = by.kind == Kind::Session
        && by.domain.account() == cx.domain.account()
        && by.domain.labels().is_empty();
    ok_if(sponsor, Refusal::NotSponsor)
}

/// Every audit read goes through it: a record is read under R25, so only a reader holding all
/// its labels reads it.
pub fn audit_visible(reader: &Labels, a: &Audit) -> bool { reader.includes(a.domain().labels()) }

// The kind guards: no rule, no mutation.

/// A lease an approved request started.
pub fn granted(cx: &Cx<'_>) -> Verdict { ok_if(lease(cx).is_some_and(|l| l.granted), Refusal::Unknown) }

fn content(cx: &Cx<'_>, f: fn(&Content) -> bool) -> Verdict {
    ok_if(request(cx).is_some_and(|r| f(&r.content)), Refusal::Unknown)
}

/// A request for a labelled agent.
pub fn grants_lease(cx: &Cx<'_>) -> Verdict { content(cx, |c| matches!(c, Content::Agent { .. })) }

/// A declassification.
pub fn declassifies(cx: &Cx<'_>) -> Verdict { content(cx, |c| matches!(c, Content::Declassify { .. })) }

/// A push.
pub fn pushes(cx: &Cx<'_>) -> Verdict { content(cx, |c| matches!(c, Content::Push { .. })) }

fn crossing(cx: &Cx<'_>, kind: CrossingKind) -> Verdict {
    ok_if(cx.state.crossings.get(&cx.id).is_some_and(|c| c.kind == kind), Refusal::Unknown)
}

/// A declassification's read.
pub fn reading(cx: &Cx<'_>) -> Verdict { crossing(cx, CrossingKind::Read) }

/// A declassification's copy out.
pub fn copying(cx: &Cx<'_>) -> Verdict { crossing(cx, CrossingKind::CopyOut) }
