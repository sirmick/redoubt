//! Arbitrary `netif` requests, from any caller, against an honest device: whatever the words,
//! lend and caller, `netd` answers without panicking, nothing but the client's well-formed
//! transmits reaches the wire, and every frame on the wire is one it was asked to send.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_netd::fake::FakeNic;
use redoubt_netd::server::answer_with;
use redoubt_netd::{NetServer, Up, bring_up};
use redoubt_rt::abi::{Labels, ReceivedHandles};
use redoubt_rt::ipc::Caller;

const CLIENT: u64 = 3;

fuzz_target!(|data: &[u8]| {
    let nic = FakeNic::new();
    let Ok(Up { mac, tx, .. }) = bring_up(&nic.rx_view(), &nic.tx_view()) else { return };
    let mut server = NetServer::new(nic.tx_view(), tx, mac, CLIENT);
    for chunk in data.chunks(64) {
        let Some((&head, body)) = chunk.split_first() else { continue };
        let badge = if head & 1 == 0 { CLIENT } else { u64::from(head) };
        let labels: &[u64] = if head & 2 == 0 { &[] } else { &[9] };
        let caller = Caller { badge, account: 0, labels: Labels::from_slice(labels).unwrap() };
        let word = |i: usize| body.get(i).copied().map_or(0, u64::from);
        let words = [word(0) % 4, word(1), word(2) % 2, 0];
        let mut lend = body.to_vec();
        let before = nic.wire().len();
        let _ = answer_with(&mut server, &caller, &words, &ReceivedHandles::new(), &mut lend);
        if badge != CLIENT || !labels.is_empty() {
            assert_eq!(nic.wire().len(), before, "a stranger's request reached the wire");
        }
    }
    assert_eq!(nic.strayed(), 0);
    for sent in nic.wire() {
        assert!((12 + 14..=12 + 1514).contains(&sent.len()));
    }
});
