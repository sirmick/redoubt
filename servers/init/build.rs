//! Builds the loader stub (`stub/`) as a flat binary for the target and profile `init` is built
//! for, and embeds it (`STUB_BIN`): `init` maps the stub into every child it starts, and never
//! reads it from the bundle (servers/init.md, "Launching through the loader stub"). The same
//! nested build `tests/programs/build.rs` makes for the bench's launcher, into a target directory
//! of its own so the two never wait on each other's lock.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap();
    // Only the machine's `init` launches anything; a host test of the library builds no stub.
    if !target.starts_with("riscv") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir.parent().unwrap().parent().unwrap().to_path_buf();
    let profile = if env::var("PROFILE").as_deref() == Ok("release") { "release" } else { "dev" };
    let profile_dir = if profile == "dev" { "debug" } else { "release" };

    let target_dir = workspace.join("target/init-stub-build");
    let status = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(&workspace)
        .args(["build", "--profile", profile, "--target", &target, "-p", "stub", "--bin", "stub"])
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("running cargo build -p stub");
    assert!(status.success(), "building the loader stub for {target} failed");

    let elf = target_dir.join(&target).join(profile_dir).join("stub");
    let bin = out_dir.join("stub.bin");
    objcopy(&elf, &bin);
    println!("cargo:rustc-env=STUB_BIN={}", bin.display());
    println!("cargo:rerun-if-changed={}", elf.display());
    // What `cargo build -p stub` reads, so a change to the stub rebuilds it here.
    for input in [
        "stub/src",
        "stub/link.x",
        "stub/build.rs",
        "stub/Cargo.toml",
        "libs/sys/src",
        "libs/sys/Cargo.toml",
        "libs/wire/src",
        "libs/wire/Cargo.toml",
        "Cargo.toml",
        "Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={}", workspace.join(input).display());
    }
    println!("cargo:rerun-if-env-changed=CARGO");
}

fn objcopy(elf: &Path, bin: &Path) {
    let status = Command::new("rust-objcopy")
        .args(["-O", "binary"])
        .arg(elf)
        .arg(bin)
        .status()
        .expect("running rust-objcopy (cargo install cargo-binutils)");
    assert!(status.success(), "objcopy of {} failed", elf.display());
}
