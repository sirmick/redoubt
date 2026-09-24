//! Builds the loader stub (`stub/`, WP-R2) as a flat binary for whatever target/profile
//! `test-programs` itself is being built for, and embeds it via `include_bytes!` so a launcher
//! test program never needs to read it from the boot bundle (that path is K4's bundle-file
//! readback, R3's gate -- WP-R2 does not depend on it: PACKAGES.md's stub is "a flat binary...
//! the same for everyone", not something a program reads at runtime).
//!
//! Mirrors `tools/testbench/src/build.rs`'s own nested `cargo build` calls and
//! `bios/xtask`'s ELF->flat conversion with `rust-objcopy`.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap();
    // Only the on-target binaries are buildable this way; a host `cargo check`/`cargo test` of
    // this crate must not try to cross-build them.
    if !target.starts_with("riscv") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let workspace = workspace_root();
    let profile = if env::var("PROFILE").as_deref() == Ok("release") { "release" } else { "dev" };
    let profile_dir = if profile == "dev" { "debug" } else { "release" };

    // A separate `--target-dir` avoids the file-lock deadlock a build script would otherwise hit
    // invoking `cargo` while the outer `cargo` (building this very crate) still holds the lock
    // on the ordinary `target/` (`stub` is a different package, so this is not recursive: its
    // own `build.rs` never calls back into `test-programs`).
    let stub_target_dir = workspace.join("target/wp-r2-stub-build");
    build(&workspace, &stub_target_dir, &target, profile);

    // `stub` itself: objcopied to the flat binary a launcher maps (PACKAGES.md: "a flat
    // binary... mapping it needs no parsing").
    let stub_elf = stub_target_dir.join(&target).join(profile_dir).join("stub");
    let stub_bin = out_dir.join("stub.bin");
    objcopy(&stub_elf, &stub_bin);
    println!("cargo:rustc-env=STUB_BIN={}", stub_bin.display());
    println!("cargo:rerun-if-changed={}", stub_elf.display());

    // `fixture-child`: kept as an ordinary ELF -- the image the stub itself parses, exactly as
    // PACKAGES.md's step 4 hands it a program's bytes. Stripped, as a shipped program would be:
    // the rv32 build otherwise carries debug sections from the prebuilt `core`, several pages
    // the launcher would copy for nothing.
    let child_elf = stub_target_dir.join(&target).join(profile_dir).join("fixture-child");
    let child_stripped = out_dir.join("fixture-child.elf");
    let status = Command::new("rust-objcopy")
        .args(["--strip-all", child_elf.to_str().unwrap(), child_stripped.to_str().unwrap()])
        .status()
        .expect("running rust-objcopy (cargo install cargo-binutils)");
    assert!(status.success(), "stripping {} failed", child_elf.display());
    println!("cargo:rustc-env=STUB_CHILD_ELF={}", child_stripped.display());
    println!("cargo:rerun-if-changed={}", child_elf.display());

    // The outputs above are this script's own products, so watching them alone never rebuilds
    // the stub after its sources change: watch what `cargo build -p stub` reads instead.
    for input in
        ["stub/src", "stub/link.x", "stub/build.rs", "stub/Cargo.toml", "libs/sys/src", "libs/wire/src"]
    {
        println!("cargo:rerun-if-changed={}", workspace.join(input).display());
    }
    println!("cargo:rerun-if-env-changed=CARGO");
}

/// `cargo build -p stub` for `target`/`profile` into `target_dir`, both its binaries at once.
fn build(workspace: &std::path::Path, target_dir: &std::path::Path, target: &str, profile: &str) {
    let mut cargo = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo.current_dir(workspace).args([
        "build",
        "--profile",
        profile,
        "--target",
        target,
        "-p",
        "stub",
        "--target-dir",
        target_dir.to_str().unwrap(),
    ]);
    let status = cargo.status().expect("running cargo build -p stub");
    assert!(status.success(), "building the loader stub crate for {target} failed");
}

fn objcopy(elf: &std::path::Path, bin: &std::path::Path) {
    let status = Command::new("rust-objcopy")
        .args(["-O", "binary", elf.to_str().unwrap(), bin.to_str().unwrap()])
        .status()
        .expect("running rust-objcopy (cargo install cargo-binutils)");
    assert!(status.success(), "objcopy of {} failed", elf.display());
}

/// The workspace root: this crate's manifest directory is `<root>/tests/programs`.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    manifest_dir.parent().unwrap().parent().unwrap().to_path_buf()
}
