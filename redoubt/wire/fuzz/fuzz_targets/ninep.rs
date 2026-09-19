//! 9P2000: decoding never panics, and every accepted message re-encodes to exactly the bytes
//! it was decoded from (strict decoding: one encoding per message). Encoding into a buffer
//! too short fails and leaves it zeroed. Directory data parses without panicking, its stats
//! re-encode exactly, and an entry that does not fit is not written at all.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_wire::codec::Writer;
use redoubt_wire::ninep::{message_size, stats, Message};
use redoubt_wire::MSIZE;

fuzz_target!(|data: &[u8]| {
    // The last byte picks how short the short buffers are.
    let cut = usize::from(data.last().copied().unwrap_or(0));
    if let Ok(m) = Message::decode(data) {
        let size = message_size(data).expect("decoded, so the size is valid");
        let mut out = vec![0u8; MSIZE];
        let n = m.encode(&mut out).expect("a decoded message encodes");
        assert_eq!(&out[..n], &data[..size], "re-encoding differs");
        assert_eq!(Message::decode(&out[..n]), Ok(m));
        let short = n.saturating_sub(1 + cut % n);
        let mut out = vec![0u8; short];
        assert!(m.encode(&mut out).is_err(), "encoded into too short a buffer");
        assert!(out.iter().all(|&b| b == 0), "a failed encode left bytes behind");
    }
    let mut out = vec![0u8; data.len()];
    let mut w = Writer::new(&mut out);
    let mut consumed = true;
    for stat in stats(data) {
        match stat {
            Ok(s) => s.write_entry(&mut w).expect("a decoded stat encodes in the space it came from"),
            Err(_) => consumed = false,
        }
    }
    let written = w.position();
    if consumed {
        assert_eq!(&out[..written], data, "stats re-encode differently");
    }
    // The same entries into a buffer `cut` bytes short: whole entries only, the rest zero.
    let mut out = vec![0u8; written.saturating_sub(cut)];
    let mut w = Writer::new(&mut out);
    for s in stats(data).map_while(Result::ok) {
        if s.write_entry(&mut w).is_err() {
            break;
        }
    }
    let fit = w.position();
    assert!(out[fit..].iter().all(|&b| b == 0), "a failed entry left bytes behind");
    assert!(stats(&out[..fit]).all(|s| s.is_ok()), "a partial entry was written");
});
