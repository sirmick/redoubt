//! CLINT backend selection from FDT identity and register layout.
//!
//! Compatible strings follow the pinned [SiFive CLINT binding]. Register
//! offsets and access widths are documented by the selected driver.
//!
//! [SiFive CLINT binding]: https://github.com/torvalds/linux/blob/a500db7819c50db59e55f1b4fa1c3baa5a2616f3/Documentation/devicetree/bindings/timer/sifive%2Cclint.yaml

/// The CLINT device kinds the firmware can drive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClintKind {
    SiFive,
    #[cfg(not(feature = "qemu-virt"))]
    THead,
}

const SIFIVE_CLINT_COMPATIBLES: [&str; 3] =
    ["riscv,clint0", "starfive,jh7110-clint", "sifive,clint0"];
#[cfg(not(feature = "qemu-virt"))]
const T_HEAD_CLINT_COMPATIBLES: [&str; 1] = ["thead,c900-clint"];

impl ClintKind {
    /// Maps one `compatible` string to a driver.
    pub(crate) fn from_fdt(compatible: &str) -> Option<Self> {
        if SIFIVE_CLINT_COMPATIBLES.contains(&compatible) {
            return Some(Self::SiFive);
        }
        #[cfg(not(feature = "qemu-virt"))]
        if T_HEAD_CLINT_COMPATIBLES.contains(&compatible) {
            Some(Self::THead)
        } else {
            None
        }
        #[cfg(feature = "qemu-virt")]
        None
    }

    /// Device name reported in boot logs.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::SiFive => "SiFiveClint",
            #[cfg(not(feature = "qemu-virt"))]
            Self::THead => "THeadClint",
        }
    }
}
