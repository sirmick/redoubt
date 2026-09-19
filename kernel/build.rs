// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    // If we're not running on a desktop-class operating system, emit the "baremetal"
    // config setting. This will enable software to do tasks such as
    // managing memory.
    if target_os == "none" || target_os == "xous" {
        println!("Target {} is bare metal", target);
        println!("cargo:rustc-cfg=baremetal");
    } else {
        println!("Target {} is NOT bare metal", target);
    }

    // On RISC-V, startup and trap entry are `global_asm!` (src/arch/riscv/asm.rs) for both
    // widths, so there is no prebuilt startup library to link: just the linker script.
    if target.starts_with("riscv") {
        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rustc-link-arg=-Tlink.x");

        // Sv39 (rv64) and Sv32 (rv32) put the kernel at the same virtual addresses, sign-
        // extended; the two scripts differ only in address width.
        let linker_file_path =
            if target.starts_with("riscv64") { PathBuf::from("link64.x") } else { PathBuf::from("link.x") };
        println!("cargo:rerun-if-changed={}", linker_file_path.display());

        // Put the linker script somewhere the linker can find it
        fs::File::create(out_dir.join("link.x"))
            .unwrap()
            .write_all(fs::read_to_string(linker_file_path).expect("linker file read").as_bytes())
            .unwrap();

        println!("cargo:rustc-link-search={}", out_dir.display());
        println!("cargo:rerun-if-changed=link.x");
        println!("cargo:rustc-link-arg=-Map=kernel.map");
    }

    println!("cargo:rerun-if-changed=build.rs");

    // CI sets this variable. This changes how the panic handler works.
    println!("cargo:rerun-if-env-changed=CI");
    if option_env!("CI").is_some() {
        println!("cargo:rustc-cfg=ci");
    }
}
