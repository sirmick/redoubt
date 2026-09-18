#[cfg(all(any(target_os = "none", target_os = "xous"), target_arch = "arm"))]
mod arm;
#[cfg(all(any(target_os = "none", target_os = "xous"), target_arch = "arm"))]
pub use arm::*;

#[cfg(all(any(target_os = "xous", target_os = "none"), any(target_arch = "riscv32", target_arch = "riscv64")))]
pub mod riscv;
#[cfg(all(any(target_os = "xous", target_os = "none"), any(target_arch = "riscv32", target_arch = "riscv64")))]
pub use riscv::*;

#[cfg(all(
    not(feature = "processes-as-threads"),
    not(any(target_os = "xous", target_os = "none")),
    not(target_arch = "arm"),
))]
pub mod hosted;
#[cfg(all(
    not(feature = "processes-as-threads"),
    not(any(target_os = "xous", target_os = "none")),
    not(target_arch = "arm"),
))]
pub use hosted::*;

#[cfg(feature = "processes-as-threads")]
pub mod test;
#[cfg(feature = "processes-as-threads")]
pub use test::*;
