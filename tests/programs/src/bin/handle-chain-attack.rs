//! A destruction closes every handle that depends on the dying subtree wherever it is held
//! (kernel/budgets.md, "Residual risks", item 3; R10, I1, I2). Handles held outside a budget are
//! found through its chains, not a sweep of every table, so the case puts them where a missed
//! chain entry would leave them alive: several levels up, beside the subtree and across it.
//!
//! The tree, under `system` (a mint narrows only to the minter's budget or below it, I3): `upper`
//! holds `top` and `beside`; `top` holds `middle`, which holds `low`. The dying top is `top`.
//! - The launcher runs in `system`, two levels above `top` and outside everything below it.
//! - Holder A runs in `upper`, above `top`; holder B runs in `beside`, next to it.
//! - Maker C runs in `low` and creates an endpoint there, so `low` owns it.
//!
//! Each holder is given four handles that must go with `top` and one that must stay:
//! - `stamped`: the launcher's endpoint minted into `low`'s scope (its stamp dies);
//! - `budget`: `low` itself, stamped by the launcher's budget (its object dies);
//! - `owned`: C's endpoint, which `low` owns (its object dies);
//! - `process`: a process object holder A creates, so `upper` is charged for it and stamps it, for a process
//!   in `low` whose exit endpoint is `owned`. Both outlive the destruction, but the object does not: its
//!   notice is dropped with `owned`, and the object freed with it. A holds it as its creator, B inside A's
//!   budget, and the launcher outside it under the live stamp;
//! - `kept`: the launcher's endpoint under the launcher's own stamp.
//!
//! The launcher destroys `top`, then each holder closes all five. The verdict is the kernel's: a
//! closed handle is `BadHandle`, a kept one closes `Ok`, and the checked build's chain audit
//! (`check_handle_chains`, after the walk) finds every live handle where it should be. With
//! `handle-chain-fault` the kernel installs handles without their stamp entry, and with
//! `process-chain-fault` without their process object's entry, and that audit must stop it
//! (tests/handle-chain-fault.toml, tests/process-chain-fault.toml).

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

/// What each holder answers, a bit per handle: set when the kernel's answer was the right one.
const ALL_RIGHT: usize = 0b11111;

/// The handle at `i` of a received message's list, or 0.
fn received(m: &rd::Message, i: usize) -> u32 {
    m.body.handles.as_slice().get(i).copied().flatten().map_or(0, |h| h.index())
}

/// A holder: slot 1 sends to the launcher's report endpoint, slot 2 receives its orders, slots 3
/// to 5 hold `stamped`, `budget` and `kept`. `owned` arrives in the first order, and `process`
/// with it, unless the order's first word asks this holder to create it; then the reply carries it.
/// After the second order, it closes all five and reports which answers were right.
extern "C" fn holder(_: usize) -> ! {
    let (rep, go, stamped, budget, kept) = (1, 2, 3, 4, 5);
    let (owned, mut process, create) = match rd::receive(Some(go), rd::FOREVER, 0) {
        Ok(Received::Message(m)) => (received(&m, 0), received(&m, 1), m.body.words[0] != 0),
        _ => (0, 0, false),
    };
    if create {
        process = rd::process_create(budget, owned).unwrap_or(0);
    }
    let made = &[process][..usize::from(create && process != 0)];
    let _ = rd::send(rep, &rd::body_with([owned as usize, 0, 0, 0], made), None, rd::FOREVER);
    let _ = rd::receive(Some(go), rd::FOREVER, 0);
    let gone = |h: u32| rd::close(h) == Err(Error::BadHandle);
    let right = usize::from(gone(stamped))
        | usize::from(gone(budget)) << 1
        | usize::from(gone(owned)) << 2
        | usize::from(rd::close(kept).is_ok()) << 3
        | usize::from(gone(process)) << 4;
    let _ = rd::send(rep, &rd::body([right, 0, 0, 0]), None, rd::FOREVER);
    rd::process_exit(0)
}

/// The maker: an endpoint in `low`, sent to the launcher, then a wait that only its end ends.
extern "C" fn maker(_: usize) -> ! {
    let owned = rd::endpoint_create().expect("an endpoint in low");
    let _ = rd::send(1, &rd::body_with([0, 0, 0, 0], &[owned]), None, rd::FOREVER);
    let _ = rd::receive(Some(owned), rd::FOREVER, 0);
    rd::process_exit(0)
}

/// A budget under `parent` with `pages` pages, room for `processes`, and `weight`.
fn budget(parent: u32, pages: u64, processes: u32, weight: u32) -> u32 {
    rd::create(parent, &rd::spec(pages, processes, weight)).expect("a budget")
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("chain-attack");
    // Room for a process each, nested: every level carves its child's pages and one of its own.
    let per = b.image().pages() as u64 + 128;
    let upper = budget(rd::SYSTEM, 4 * per + 64, 4, 100);
    // `low` runs C and A's process.
    let top = budget(upper, per + 32, 2, 30);
    let middle = budget(top, per + 16, 2, 20);
    let low = budget(middle, per, 2, 10);
    let beside = budget(upper, per, 1, 10);
    let rep = rd::endpoint_create().unwrap();
    let mine = rd::endpoint_create().unwrap();

    // C makes the endpoint `low` owns and sends it here.
    let to_rep = rd::mint_from_handle(rep, 9, None).unwrap();
    spawn::spawn(b.image(), low, b.exit_endpoint(), maker as *const () as usize, &[], &[to_rep]).expect("C");
    let owned = match rd::receive(Some(rep), 60_000_000, 0) {
        Ok(Received::Message(m)) => received(&m, 0),
        _ => 0,
    };
    b.check(owned != 0, format_args!("C's endpoint, owned by low, is held in system"));

    let stamped = rd::mint_from_handle(mine, 1, Some(low)).expect("a mint into low's scope");
    let kept = rd::mint_from_handle(mine, 2, None).expect("a mint under system's stamp");
    let mut go = [0u32; 2];
    let mut process = 0;
    for (i, place) in [upper, beside].into_iter().enumerate() {
        let order = rd::endpoint_create().unwrap();
        go[i] = rd::mint_from_handle(order, 1, None).unwrap();
        let to_rep = rd::mint_from_handle(rep, i as u64 + 1, None).unwrap();
        spawn::spawn(
            b.image(),
            place,
            b.exit_endpoint(),
            holder as *const () as usize,
            &[],
            &[to_rep, order, stamped, low, kept],
        )
        .expect("a holder");
        // A, in `upper`, creates the process object; B is given a copy.
        let order = if i == 0 {
            rd::body_with([1, 0, 0, 0], &[owned])
        } else {
            rd::body_with([0; 4], &[owned, process])
        };
        let _ = rd::send(go[i], &order, None, rd::FOREVER);
        let took = match rd::receive(Some(rep), 60_000_000, 0) {
            Ok(Received::Message(m)) => {
                if i == 0 {
                    process = received(&m, 0);
                }
                m.body.words[0] != 0 && process != 0
            }
            _ => false,
        };
        let place = ["upper", "beside"][i];
        b.check(took, format_args!("holder {} in {} holds C's endpoint and the process object", i, place));
    }

    b.check(rd::destroy(top) == Ok(()), format_args!("top is destroyed"));
    let gone = |h: u32| rd::close(h) == Err(Error::BadHandle);
    let mine_right = usize::from(gone(stamped))
        | usize::from(gone(low)) << 1
        | usize::from(gone(owned)) << 2
        | usize::from(rd::close(kept).is_ok()) << 3
        | usize::from(gone(process)) << 4;
    b.check(mine_right == ALL_RIGHT, format_args!("system, several levels up: answers {:#07b}", mine_right));
    for (i, order) in go.into_iter().enumerate() {
        let _ = rd::send(order, &rd::body([0, 0, 0, 0]), None, rd::FOREVER);
        let right = match rd::receive(Some(rep), 60_000_000, 0) {
            Ok(Received::Message(m)) => m.body.words[0],
            _ => 0,
        };
        let place = ["upper, above top", "beside, next to it"][i];
        b.check(right == ALL_RIGHT, format_args!("{}: answers {:#07b}", place, right));
    }
    b.finish("HANDLE-CHAIN-ATTACK")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("chain-attack", info) }
