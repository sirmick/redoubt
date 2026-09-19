//! Typed messages, through the generated `example` codec (every field type, both shapes):
//! decoding arbitrary words, buffers and handle counts never panics, and every accepted
//! message re-encodes to the same words and the same message bytes.
//!
//! Input layout: 4 words (8 bytes each, little-endian), 1 byte of handle count, the rest is
//! the buffer. Words are biased towards small values so that valid messages are common.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_wire::proto::example::Message;
use redoubt_wire::MSIZE;

fuzz_target!(|data: &[u8]| {
    let Some((head, buf)) = data.split_at_checked(33) else { return };
    let mut words = [0u64; 4];
    for (w, chunk) in words.iter_mut().zip(head.chunks_exact(8)) {
        let raw = u64::from_le_bytes(chunk.try_into().unwrap());
        // The top byte picks the range: mostly 32-bit words, sometimes anything.
        *w = if raw >> 56 < 0xf0 { raw & 0xffff_ffff } else { raw };
    }
    // Opcodes: mostly the defined ones, plus the extremes.
    words[0] = match words[0] % 11 {
        8 => u64::from(u32::MAX),
        9 => words[0] >> 4,
        10 => 0,
        n => n,
    };
    let handles = usize::from(head[32] % 6);
    if let Ok(m) = Message::decode(&words, buf, handles) {
        let mut out = vec![0u8; MSIZE];
        let again = m.encode(&mut out).expect("a decoded message encodes");
        assert_eq!(again, words, "words differ");
        if !buf.is_empty() {
            let len = words[1] as usize;
            assert_eq!(&out[..len], &buf[..len], "buffer differs");
        }
        assert_eq!(m.handle_names().len(), handles);
    }
});
