//! `bootfsd` against WP-W1's 9P2000 conformance vectors (`redoubt/wire/vectors/9p.txt`). What
//! they check is in the runner's own docs; what is checked here on top is what only `bootfsd`
//! knows: a run of hostile and well-formed messages publishes nothing, changes nothing and
//! leaves every entry exactly as `init` sealed it.

#[path = "../../../libs/rt/tests/common/vectors.rs"]
mod vectors;

use redoubt_bootfsd::server::{BootFs, LIMITS};
use redoubt_rt::abi::Labels;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FIRST_MINTED_BADGE, NineServer};
use redoubt_rt::server::typed::TypedServer;
use redoubt_rt::wire::proto::bootfs::{Add, Message, Seal};

const ENTRIES: [(&str, &[u8]); 2] = [("keyd", b"\x7fELF keyd"), ("beamlet", b"\x7fELF beamlet")];

fn sealed() -> BootFs {
    let founder = Caller { badge: 1, account: 0, labels: Labels::from_slice(&[]).unwrap() };
    let mut fs = BootFs::new(ENTRIES.iter().map(|(name, _)| *name)).unwrap();
    for (name, data) in ENTRIES {
        fs.handle(&founder, Message::Add(Add { name, offset: 0, data }), &[]).unwrap();
    }
    fs.handle(&founder, Message::Seal(Seal {}), &[]).unwrap();
    fs
}

#[test]
fn the_conformance_vectors_run_against_bootfsd() {
    let mut server = NineServer::new(sealed(), LIMITS, 0x1234_5678_9abc_def0).unwrap();
    // A client, with the badge a minted connection carries and an account of its own.
    let who =
        Caller { badge: FIRST_MINTED_BADGE + 7, account: 1001, labels: Labels::from_slice(&[]).unwrap() };
    let counts = vectors::run(&mut server, &who);
    assert!(counts.well_formed > 20 && counts.malformed > 5, "{counts:?}");
    // Every entry is exactly what was sealed: the vectors include a write, a create, a remove
    // and a wstat, and none of them reached anything.
    for (name, data) in ENTRIES {
        assert_eq!(server.fs.entry(name), Some(data), "{name} changed");
    }
    assert!(server.fs.sealed(), "a vector unsealed /boot");
    assert_eq!(server.fs.len(), ENTRIES.len(), "a vector added or removed an entry");
}
