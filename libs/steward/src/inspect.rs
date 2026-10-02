//! A read-only view of the store, for the model's checks (servers/steward.md, "Two embedders and
//! a reference"). The server does not use it, and nothing in it changes anything.

use alloc::vec::Vec;

use crate::audit::Audit;
use crate::domain::{Domain, Labels};
use crate::gen::Policy;
use crate::manifest::Fixed;
use crate::store::{DomainState, Index, Store};

/// Every domain and its state, in the manifest's order.
pub fn domains(store: &Store) -> impl Iterator<Item = (&Domain, &DomainState)> { store.all() }

pub fn domain<'a>(store: &'a Store, d: &Domain) -> Option<&'a DomainState> { store.domain(d) }

/// The routing index and the approval channels.
pub fn index(store: &Store) -> &Index { &store.index }

pub fn fixed(store: &Store) -> &Fixed { &store.fixed }

pub fn policy(store: &Store) -> &Policy { &store.policy }

/// Whether the steward exited on a broken guarantee.
pub fn exited(store: &Store) -> bool { store.exited }

/// The records a reader with `reader` labels may read, through the policy's audit filter: every
/// audit read goes through it.
pub fn audit_view<'a>(store: &Store, records: &'a [Audit], reader: &Labels) -> Vec<&'a Audit> {
    let visible = store.policy.audit_visible;
    records.iter().filter(|a| visible(reader, a)).collect()
}
