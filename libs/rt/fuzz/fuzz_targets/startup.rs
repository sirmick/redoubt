//! The startup block parser on hostile bytes: never panics, and whatever it accepts it reads
//! back consistently (every handle in range, every path clean, resolution total).
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_rt::abi::MAX_START_HANDLES;
use redoubt_rt::path;
use redoubt_rt::startup::Startup;

fuzz_target!(|data: &[u8]| {
    let Ok(startup) = Startup::parse(data) else { return };
    for (p, handle) in startup.namespace() {
        assert!(path::is_clean_absolute(p));
        assert!(handle.index() as usize <= MAX_START_HANDLES);
        assert!(startup.resolve(p).is_some());
    }
    for arg in startup.args() {
        let _ = startup.handle(arg);
    }
    // image_addr is 0 exactly when image_len is, page-aligned, and does not overflow
    // (servers/init.md, "The startup block").
    if let Some((addr, len)) = startup.image() {
        assert!(addr != 0 && len != 0);
        assert_eq!(addr % 4096, 0);
        assert!(addr.checked_add(len).is_some());
    }
});
