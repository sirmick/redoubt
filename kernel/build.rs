// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();

    // Startup and trap entry are `global_asm!` (src/arch/riscv/asm.rs) for both widths, so there
    // is no prebuilt startup library to link: just the linker script. Sv39 (rv64) and Sv32 (rv32)
    // put the kernel at the same virtual addresses, sign-extended; the two scripts differ only in
    // address width.
    let linker_file_path = if target.starts_with("riscv64") { "link64.x" } else { "link.x" };
    println!("cargo:rerun-if-changed={linker_file_path}");
    // Put the linker script somewhere the linker can find it.
    fs::write(out_dir.join("link.x"), fs::read(linker_file_path).expect("linker file read")).unwrap();

    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-arg=-Tlink.x");
    println!("cargo:rustc-link-arg=-Map=kernel.map");
    println!("cargo:rerun-if-changed=build.rs");
}
