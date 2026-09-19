//! Builds the littlefs C reference (v2.11.3, vendored in `c/` with its SPEC.md and DESIGN.md,
//! BSD-3-Clause) for the host.

fn main() {
    cc::Build::new()
        .files(["c/lfs.c", "c/lfs_util.c", "c/shim.c"])
        .include("c")
        .define("LFS_NO_DEBUG", None)
        .define("LFS_NO_WARN", None)
        .define("LFS_NO_ERROR", None)
        .warnings(false)
        .compile("lfs");
    println!("cargo:rerun-if-changed=c");
}
