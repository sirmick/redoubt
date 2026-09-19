//! Typed messages, through the generated `example` codec (every field type, both shapes,
//! replies, error replies, the file framing): decoding arbitrary words, buffers and handle
//! counts never panics, and every accepted request, reply or file re-encodes to exactly
//! what was decoded.
//!
//! Input layout: 1 byte picks request, reply or file; then 4 words (8 bytes each,
//! little-endian) and 1 byte of handle count; the rest is the buffer (for a file, all of it
//! after the first byte). Words are biased towards small values so that valid messages are
//! common.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_wire::proto::example::{Message, Reply};
use redoubt_wire::typed::Words;
use redoubt_wire::MSIZE;

/// The opcodes the fixture defines, 0, one past them, and the extremes.
fn opcode(raw: u64) -> u64 {
    match raw % 12 {
        9 => u64::from(u32::MAX),
        10 => raw >> 4,
        11 => 1 << 32,
        n => n,
    }
}

/// A buffer message re-encodes to the first word-1 bytes of what arrived.
fn check_prefix(words: &Words, buf: &[u8], out: &[u8]) {
    if !buf.is_empty() {
        let len = words[1] as usize;
        assert_eq!(&out[..len], &buf[..len], "buffer differs");
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let mut out = vec![0u8; MSIZE];
    if mode % 3 == 2 {
        if let Ok(m) = Message::decode_file(rest) {
            let n = m.encode_file(&mut out).expect("a decoded file encodes");
            assert_eq!(&out[..n], rest, "file differs");
            assert!(m.handle_names().is_empty());
        }
        return;
    }
    let Some((head, buf)) = rest.split_at_checked(33) else { return };
    let mut words = [0u64; 4];
    for (w, chunk) in words.iter_mut().zip(head.chunks_exact(8)) {
        let raw = u64::from_le_bytes(chunk.try_into().unwrap());
        // The top byte picks the range: mostly 32-bit words, sometimes anything.
        *w = if raw >> 56 < 0xf0 { raw & 0xffff_ffff } else { raw };
    }
    let handles = usize::from(head[32] % 6);
    if mode % 3 == 0 {
        words[0] = opcode(words[0]);
        if let Ok(m) = Message::decode(&words, buf, handles) {
            assert_eq!(m.encode(&mut out).expect("a decoded message encodes"), words, "words differ");
            check_prefix(&words, buf, &out);
            assert_eq!(m.handle_names().len(), handles);
        }
    } else {
        // The request's opcode comes from the handle byte's high bits; word 0 is the status,
        // mostly 0 or a defined code.
        let request = opcode(u64::from(head[32] >> 3)) as u32;
        words[0] = match words[0] % 6 {
            0..=2 => 0,
            3 => 1,
            4 => u64::from(u32::MAX),
            _ => words[0],
        };
        match Reply::decode(request, &words, buf, handles) {
            Ok(Ok(r)) => {
                assert_eq!(r.encode(&mut out).expect("a decoded reply encodes"), words, "words differ");
                check_prefix(&words, buf, &out);
                assert_eq!(r.handle_names().len(), handles);
            }
            Ok(Err(code)) => {
                assert_eq!(code.encode(), words, "error reply differs");
                assert_eq!(handles, 0);
            }
            Err(_) => {}
        }
    }
});
