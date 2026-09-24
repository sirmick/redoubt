use std::{env, fs, path::PathBuf};

fn main() {
    let target = env::var("TARGET").unwrap();
    // Only the on-target `stub` binary needs the fixed-address linker script -- scoped to that
    // one bin target (`rustc-link-arg-bin`), so this package's other on-target binaries (test
    // fixtures) link at their ordinary default address, not `STUB_ENTRY`. `cargo test -p stub
    // --lib` builds `lib.rs`'s pure logic for the host, which must link normally (kernel/build.rs
    // has the same target guard, for the same reason).
    if target.starts_with("riscv") {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        fs::copy("link.x", out_dir.join("stub.x")).unwrap();
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rustc-link-arg-bin=stub=-Tstub.x");
    }
    println!("cargo:rerun-if-changed=link.x");
    println!("cargo:rerun-if-changed=build.rs");
}
