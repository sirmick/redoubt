//! The steward's policy core (servers/steward.md, "The policy core"): everything the steward
//! decides, as one pure state machine keyed by domain. The steward server and the model both
//! embed it; neither decides anything itself.
//!
//! `decide(&mut Store, Event) -> Effects`: the event carries the time and the fresh random words
//! its ids need, and the effects are data, which the embedder carries out and reports back as
//! `Done` events. Each machine's dispatch is generated from its table in `libs/steward/tables` by
//! `redoubt-steward-gen` (`gen`); the guards and effects it calls are written here by hand
//! (`guards`, `effects`), through the `Policy` table.
//!
//! `no_std` with `alloc`, no `unsafe`, no I/O.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod audit;
pub mod consts;
pub mod cx;
pub mod domain;
mod edges;
pub mod effect;
pub mod effects;
pub mod event;
#[cfg(feature = "fuzz")]
pub mod fuzz;
pub mod gen;
pub mod guards;
pub mod hash;
pub mod inspect;
mod machines;
pub mod manifest;
pub mod render;
pub mod store;

use alloc::vec::Vec;

use crate::cx::Out;
pub use crate::effect::Effects;
pub use crate::event::Event;
pub use crate::gen::Policy;
use crate::manifest::{Carve, Fixed, Manifest};
pub use crate::store::Store;

impl Store {
    /// The store for a checked manifest, deciding by `policy` (`Policy::SHIPPED` in the server),
    /// and what boot carves: each principal's top budget and its fixed sub-budgets. Each domain's
    /// blame starts `Open` (the blame table's `Boot` row). `None` if the manifest is refused.
    pub fn boot(manifest: &Manifest, policy: Policy) -> Option<(Store, Vec<Carve>)> {
        let fixed = Fixed::new(manifest)?;
        let carves = fixed.carves();
        Some((Store::new(fixed, policy), carves))
    }
}

/// Decides one event: runs it through the machine of the object it is about, then the events
/// machines raise for each other, in order, and returns every effect. After an event the
/// embedder's guarantee excludes, the steward has exited, and decides nothing more.
pub fn decide(store: &mut Store, event: Event) -> Effects {
    if store.exited {
        return Effects { exit: true, ..Effects::default() };
    }
    let mut out = Out::new();
    machines::external(store, &event, &mut out);
    while let Some(raised) = out.raised.pop_front() {
        if out.effects.exit {
            break;
        }
        machines::internal(store, &event, raised, &mut out);
    }
    if out.effects.exit {
        store.exited = true;
        return Effects { exit: true, ..Effects::default() };
    }
    out.effects
}
