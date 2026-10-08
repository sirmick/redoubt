//! `piped` against the 9P2000 conformance vectors (`redoubt/wire/vectors/9p.txt`). What they check
//! is in the runner's own docs; what is checked here on top is what only `piped` knows: hostile and
//! well-formed messages through a stage's connection make no pipe, hold no end and move no byte.

use redoubt_fake_kernel::vectors;
use redoubt_piped::server::{Pipes, limits};
use redoubt_rt::abi::Labels;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FIRST_MINTED_BADGE, NineServer};

#[test]
fn the_conformance_vectors_run_against_piped() {
    let mut server = NineServer::new(Pipes::new(), limits(2), 0x0fed_cba9_8765_4321).unwrap();
    let who = Caller { badge: FIRST_MINTED_BADGE + 3, account: 0, labels: Labels::from_slice(&[]).unwrap() };
    let counts = vectors::run(&mut server, &who);
    assert!(counts.well_formed > 20 && counts.malformed > 5, "{counts:?}");
    assert_eq!(counts.waiting, 0, "a vector was held: {counts:?}");
    assert!(server.fs.is_empty(), "a vector made a pipe");
    assert!(!server.fs.take_moved(), "a vector moved a pipe");
}
