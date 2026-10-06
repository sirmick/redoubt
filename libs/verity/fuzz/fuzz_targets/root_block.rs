//! Arbitrary bytes as a signed volume's root block, the first thing `verityd` parses of a signed
//! volume, before any signature is checked. The claim: [`RootBlock::parse`] refuses them or
//! reads one root block that encodes back to exactly those bytes, and never panics.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_verity::RootBlock;

fuzz_target!(|data: &[u8]| {
    if let Ok(block) = RootBlock::parse(data) {
        assert_eq!(&block.encode()[..], data);
    }
});
