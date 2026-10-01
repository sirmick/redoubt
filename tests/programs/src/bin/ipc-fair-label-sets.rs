//! One label set's turns at a shared endpoint are the same whatever another label set sends
//! (kernel/ipc.md R2: the oldest message of the group served least recently).
//!
//! Three sender groups, keyed so the vault's sits between the other two in group order:
//! `low` (account `LOW`, no labels), `vault` (account `LOW`, label `LABEL`) and `high` (account
//! `HIGH`, no labels). Each round runs on a fresh endpoint: `high` is served once; in the vault
//! round the vault is served next; then `low` and `high` queue, `low` first, and the judge takes
//! both. The unlabelled order must be `low`, `high` in both rounds. A single cursor over every
//! group would leave the vault round's turn after the vault's group, so `high` would come first
//! there and `low` first in the other.
//!
//! A last round tells least recently served from oldest first: `high` queues twice, then `low`
//! once. `high`'s first message goes first, and then `high` has been served, so `low` goes before
//! `high`'s second. Oldest first across groups would take both of `high`'s before `low`'s.
//!
//! Every sender is a child that sends once through a handle badged with its group and exits; the
//! judge reads the badges the kernel delivered, so each `ok:` line is a kernel result.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::Bench;
use test_programs::spawn::{self, Image};

const LOW: u64 = 1001;
const HIGH: u64 = 1002;
const LABEL: u64 = 9;

/// Badges: which group a delivered message came from.
const B_LOW: u64 = 1;
const B_VAULT: u64 = 2;
const B_HIGH: u64 = 3;

/// Long enough for a spawned sender to run and block in `send` (virtual time).
const SETTLE_MS: u64 = 50;

/// A sender: one message through slot 1, then exit.
extern "C" fn sender(_: usize) -> ! {
    let _ = rd::send(1, &rd::body([0; rd::WORDS]), None, rd::FOREVER);
    rd::process_exit(0)
}

/// A group's budget under `users`: its account and labels are its R2 group.
fn group(account: u64, label: Option<u64>) -> u32 {
    let mut spec = rd::spec(2_000, 8, 100);
    spec.account = account;
    if let Some(l) = label {
        spec.labels.push(l).expect("a label");
    }
    rd::create(rd::USERS, &spec).expect("a group's budget")
}

/// Queue one message from `budget` on `e`, badged `badge`, and let it block there.
fn queue(image: &Image, exit: u32, e: u32, budget: u32, badge: u64) {
    let to = rd::mint_from_handle(e, badge, None).expect("a send right");
    spawn::spawn(image, budget, exit, sender as *const () as usize, &[], &[to]).expect("a sender");
    rd::close(to).expect("closing the judge's copy");
    test_programs::wait_ms(SETTLE_MS);
}

/// The badge of the next message waiting on `e`, or 0 if none is.
fn take(e: u32) -> u64 {
    match rd::receive(Some(e), 0, 0) {
        Ok(rd::Received::Message(m)) => m.badge,
        _ => 0,
    }
}

/// One round on a fresh endpoint: the badges of the two unlabelled messages, in the order taken.
fn round(b: &mut Bench, image: &Image, groups: [u32; 3], vault: bool) -> [u64; 2] {
    let [low, vlt, high] = groups;
    let exit = b.exit_endpoint();
    let e = rd::endpoint_create().expect("the shared endpoint");
    queue(image, exit, e, high, B_HIGH);
    let first = take(e);
    if vault {
        queue(image, exit, e, vlt, B_VAULT);
        let v = take(e);
        b.check(v == B_VAULT, format_args!("the vault's message is taken alone (badge {})", v));
    }
    queue(image, exit, e, low, B_LOW);
    queue(image, exit, e, high, B_HIGH);
    let order = [take(e), take(e)];
    b.check(
        first == B_HIGH && take(e) == 0,
        format_args!("round with vault={}: high served first, and nothing left over", vault),
    );
    while rd::receive(Some(exit), 0, 0).is_ok() {}
    rd::close(e).expect("closing the round's endpoint");
    order
}

/// The badges of three messages on a fresh endpoint: `high` queues two, then `low` one.
fn served_behind(b: &mut Bench, image: &Image, groups: [u32; 3]) -> [u64; 3] {
    let [low, _, high] = groups;
    let exit = b.exit_endpoint();
    let e = rd::endpoint_create().expect("the shared endpoint");
    queue(image, exit, e, high, B_HIGH);
    queue(image, exit, e, high, B_HIGH);
    queue(image, exit, e, low, B_LOW);
    let order = [take(e), take(e), take(e)];
    while rd::receive(Some(exit), 0, 0).is_ok() {}
    rd::close(e).expect("closing the round's endpoint");
    order
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("fair-labels");
    let image = spawn::image();
    let groups = [group(LOW, None), group(LOW, Some(LABEL)), group(HIGH, None)];
    let without = round(&mut b, &image, groups, false);
    let with = round(&mut b, &image, groups, true);
    b.note(format_args!("unlabelled order without the vault {:?}, with it {:?}", without, with));
    b.check(
        without == [B_LOW, B_HIGH],
        format_args!("without the vault, the older waiting group goes first"),
    );
    b.check(with == without, format_args!("a vault turn leaves the unlabelled order unchanged"));
    let behind = served_behind(&mut b, &image, groups);
    b.note(format_args!("high twice then low: taken {:?}", behind));
    b.check(
        behind == [B_HIGH, B_LOW, B_HIGH],
        format_args!("a served group goes behind one already waiting"),
    );
    b.finish("IPC-FAIR-LABEL-SETS")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("fair-labels", info) }
