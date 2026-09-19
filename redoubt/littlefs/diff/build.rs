//! Builds the littlefs C reference (v2.11.3, vendored in `c/`, BSD-3-Clause) for the host.
//! LFS_MULTIVERSION lets a test ask the reference to write on-disk version 2.0.

fn main() {
    cc::Build::new()
        .files(["c/lfs.c", "c/lfs_util.c", "c/shim.c"])
        .include("c")
        .define("LFS_MULTIVERSION", None)
        .define("LFS_NO_DEBUG", None)
        .define("LFS_NO_WARN", None)
        .define("LFS_NO_ERROR", None)
        .warnings(false)
        .compile("lfs");
    println!("cargo:rerun-if-changed=c");
}
