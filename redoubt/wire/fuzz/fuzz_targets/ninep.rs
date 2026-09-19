//! 9P2000: decoding never panics, and every accepted message re-encodes to exactly the bytes
//! it was decoded from (strict decoding: one encoding per message). Directory data parses
//! without panicking and its stats re-encode exactly.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_wire::codec::Writer;
use redoubt_wire::ninep::{message_size, stats, Message};
use redoubt_wire::MSIZE;

fuzz_target!(|data: &[u8]| {
    if let Ok(m) = Message::decode(data) {
        let size = message_size(data).expect("decoded, so the size is valid");
        let mut out = vec![0u8; MSIZE];
        let n = m.encode(&mut out).expect("a decoded message encodes");
        assert_eq!(&out[..n], &data[..size], "re-encoding differs");
        assert_eq!(Message::decode(&out[..n]), Ok(m));
    }
    let mut out = vec![0u8; data.len()];
    let mut w = Writer::new(&mut out);
    let mut consumed = true;
    for stat in stats(data) {
        match stat {
            Ok(s) => s.write(&mut w).expect("a decoded stat encodes in the space it came from"),
            Err(_) => consumed = false,
        }
    }
    let written = w.position();
    if consumed {
        assert_eq!(&out[..written], data, "stats re-encode differently");
    }
});
