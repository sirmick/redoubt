//! The three edges that cross domains, R34's control-plane edges (servers/steward.md,
//! "Domains"): the request and approval path, the crossing, and lease supervision. Only this
//! module can borrow two domains at once (`Store::pair`), and only the approval path does: an
//! approval that would start a lease reads that lease's domain's lockout.

use crate::cx::Out;
use crate::domain::Domain;
use crate::effect::Kind;
use crate::event::{Content, Event, EventKind};
use crate::gen;
use crate::machines::{Call, CrossingM, LeaseM, RequestM, answered, release, route, run, run_in, unknown};
use crate::store::{Crossing, Store};

/// The key `Store::pair` takes: only this module can make one.
pub(crate) struct TwoDomains(());

/// The request and approval path: an approval channel's `Pending`, `Approve` and `Deny`, on the
/// requests of its principal's domains.
pub(crate) fn approval(store: &mut Store, event: &Event, out: &mut Out) {
    let channel = match &event.kind {
        EventKind::Pending { channel }
        | EventKind::Approve { channel, .. }
        | EventKind::Deny { channel, .. } => *channel,
        _ => return,
    };
    let Some(principal) = store.index.channels.get(&channel).map(|c| c.principal) else {
        return unknown(event, out);
    };
    let account = store.fixed.principals[principal].account.get();
    let call = Call { channel: Some(channel), ..Call::of(event) };
    let (request, e) = match &event.kind {
        EventKind::Approve { request, .. } => (*request, gen::request::Event::Approve),
        EventKind::Deny { request, .. } => (*request, gen::request::Event::Deny),
        _ => {
            // Every pending request of the principal's domains is offered to the channel.
            let domains: alloc::vec::Vec<_> = store.of_account(account).cloned().collect();
            for d in domains {
                let ids: alloc::vec::Vec<u64> =
                    store.domain(&d).map_or(alloc::vec::Vec::new(), |s| s.requests.keys().copied().collect());
                for id in ids {
                    run::<RequestM>(store, &call, out, &d, id, gen::request::Event::Pending);
                }
            }
            return;
        }
    };
    let Some(domain) = store.used.get(&request).filter(|d| d.account().get() == account).cloned() else {
        return unknown(event, out);
    };
    let Some(r) = store.domain(&domain).and_then(|s| s.requests.get(&request)) else {
        return unknown(event, out);
    };
    // The domain an approved labelled agent would start in, when it is another.
    let grant = match (&r.content, e) {
        (Content::Agent { .. }, gen::request::Event::Approve) if r.audit != domain => Some(r.audit.clone()),
        _ => None,
    };
    let next = match grant {
        Some(g) => {
            let Some((policy, parts, grant)) = store.pair(&TwoDomains(()), &domain, &g) else {
                return unknown(event, out);
            };
            run_in::<RequestM>(policy, parts, Some(grant), &call, out, request, e)
        }
        None => run::<RequestM>(store, &call, out, &domain, request, e),
    };
    release(store, &domain, Kind::Request, request);
    answered(next, event, out);
}

/// Lease supervision: an unlabelled session of the sponsor ends a lease, in the lease's domain.
/// Ahead of admission, as the table marks it.
pub(crate) fn end_lease(store: &mut Store, event: &Event, out: &mut Out) {
    let EventKind::EndLease { badge, lease } = &event.kind else { return };
    let Some(by) = route(store, *badge) else { return unknown(event, out) };
    let Some((domain, Kind::Lease)) = store.index.ids.get(lease).cloned() else { return unknown(event, out) };
    if domain.account() != by.domain.account() {
        return unknown(event, out);
    }
    let call = Call { caller: Some(by), ..Call::of(event) };
    let next = run::<LeaseM>(store, &call, out, &domain, *lease, gen::lease::Event::EndLease);
    answered(next, event, out);
}

/// The crossing: a request opens one in the labelled side's domain (a declassification's read
/// or copy out in its own, a push's write in its target's).
pub(crate) fn cross(store: &mut Store, event: &Event, domain: Domain, crossing: Crossing, out: &mut Out) {
    let id = crossing.id;
    let Some(state) = store.domain_mut(&domain) else { return };
    state.crossings.insert(id, crossing);
    store.used.insert(id, domain.clone());
    let call = Call { created: true, ..Call::of(event) };
    run::<CrossingM>(store, &call, out, &domain, id, gen::crossing::Event::Open);
    release(store, &domain, Kind::Crossing, id);
}
