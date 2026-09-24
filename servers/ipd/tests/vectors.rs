//! `ipd` against WP-W1's 9P2000 conformance vectors (`libs/wire/vectors/9p.txt`), run by the
//! shared runner (`libs/rt/tests/common/vectors.rs`, whose docs say what it checks). On top of it,
//! what only `ipd` knows: a run of hostile and well-formed messages leaves no socket behind and
//! sends nothing on the wire, from a root badge that may connect anywhere.

mod common;
#[path = "../../../libs/rt/tests/common/vectors.rs"]
mod vectors;

use common::{ANY, World, caller, owner};

#[test]
fn the_conformance_vectors_run_against_ipd() {
    let mut w = World::new(64);
    // A root badge with the widest scope, so a vector gets as far into `/net` as any client can.
    let who = caller(ANY, 1001, &[]);
    let counts = vectors::run(&mut w.nine, &who);
    assert!(counts.well_formed > 20 && counts.malformed > 5, "{counts:?}");
    assert_eq!(counts.waiting, 0, "a vector was held: {counts:?}");
    // The vectors name no file `ipd` has but the root, so nothing reached the stack.
    assert!(w.nine.fs.stack.numbers(owner(&who)).is_empty());
    assert_eq!(w.nine.fs.stack.live(), 0);
    w.pump();
    assert!(w.wire.borrow().sent.is_empty(), "a vector put something on the wire");
}
