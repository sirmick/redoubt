//! Builds what the rig launches and embeds it (`include_bytes!` in `src/rig.rs`): the loader stub
//! as a flat binary, and `netd`, `ipd` and `net-client` as stripped ELFs, for whatever target and
//! profile this crate is being built for. The same nested build as `tests/programs/build.rs`,
//! into its own target directory, so the outer `cargo`'s lock on `target/` is never waited for.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// (package, binary, environment variable naming the embedded file).
const ELFS: &[(&str, &str, &str)] = &[
    ("redoubt-netd", "netd", "NET_RIG_NETD"),
    ("redoubt-ipd", "ipd", "NET_RIG_IPD"),
    ("redoubt-net-client", "net-client", "NET_RIG_CLIENT"),
];

fn main() {
    let target = env::var("TARGET").unwrap();
    // A host `cargo test` of this crate runs only its host tests, which launch nothing.
    if !target.starts_with("riscv") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir.parent().unwrap().parent().unwrap().to_path_buf();
    let profile = if env::var("PROFILE").as_deref() == Ok("release") { "release" } else { "dev" };
    let profile_dir = if profile == "dev" { "debug" } else { "release" };
    let target_dir = workspace.join("target/net-rig-build");

    let mut packages = vec![("stub", "stub")];
    packages.extend(ELFS.iter().map(|(package, bin, _)| (*package, *bin)));
    build(&workspace, &target_dir, &target, profile, &packages);
    let built = target_dir.join(&target).join(profile_dir);

    let stub = out_dir.join("stub.bin");
    objcopy(&["-O", "binary"], &built.join("stub"), &stub);
    println!("cargo:rustc-env=NET_RIG_STUB={}", stub.display());
    for (_, bin, var) in ELFS {
        let stripped = out_dir.join(format!("{bin}.elf"));
        objcopy(&["--strip-all"], &built.join(bin), &stripped);
        println!("cargo:rustc-env={var}={}", stripped.display());
    }

    // What the nested build reads, so a change to any of it rebuilds the rig.
    for input in [
        "stub/src",
        "stub/link.x",
        "stub/build.rs",
        "stub/Cargo.toml",
        "servers/netd/src",
        "servers/netd/Cargo.toml",
        "servers/ipd/src",
        "servers/ipd/build.rs",
        "servers/ipd/Cargo.toml",
        "tests/net/client/src",
        "tests/net/client/Cargo.toml",
        "libs/rt/src",
        "libs/sys/src",
        "libs/wire/src",
        "vendor/smoltcp/src",
        "Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={}", workspace.join(input).display());
    }
    println!("cargo:rerun-if-env-changed=CARGO");
}

/// One `cargo build` of every `(package, bin)` for `target` and `profile` into `target_dir`.
fn build(workspace: &Path, target_dir: &Path, target: &str, profile: &str, packages: &[(&str, &str)]) {
    let mut cargo = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo.current_dir(workspace).args(["build", "--profile", profile, "--target", target]);
    cargo.arg("--target-dir").arg(target_dir);
    for (package, bin) in packages {
        cargo.args(["-p", package, "--bin", bin]);
    }
    let status = cargo.status().expect("running cargo build for the rig's programs");
    assert!(status.success(), "building the rig's programs for {target} failed");
}

fn objcopy(how: &[&str], from: &Path, to: &Path) {
    let status = Command::new("rust-objcopy")
        .args(how)
        .arg(from)
        .arg(to)
        .status()
        .expect("running rust-objcopy (cargo install cargo-binutils)");
    assert!(status.success(), "rust-objcopy {how:?} {} failed", from.display());
}
