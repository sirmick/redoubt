pub mod allwinner_v821;
#[cfg(not(feature = "qemu-virt"))]
pub mod allwinner_v861;
pub mod csr;
#[cfg(not(feature = "qemu-virt"))]
pub mod spacemit_k1;
