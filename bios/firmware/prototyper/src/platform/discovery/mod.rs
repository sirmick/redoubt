//! Translation from Platform Description nodes into [`BoardInfo`].

mod console;
#[cfg(not(feature = "qemu-virt"))]
mod devices;
#[cfg(feature = "qemu-virt")]
#[path = "devices_qemu.rs"]
mod devices;
mod harts;
#[cfg(not(feature = "qemu-virt"))]
mod imsic;
#[cfg(not(feature = "qemu-virt"))]
mod syscon;

use crate::devicetree::Tree;

use super::info::BoardInfo;

/// Reads the platform facts consumed by driver and SBI initialization.
pub(super) fn discover_platform(
    platform: &runtime::PlatformView<'_>,
) -> runtime::Result<BoardInfo> {
    let root = platform.root();
    let tree = root.deserialize::<Tree>();
    let mut board = BoardInfo::empty();
    harts::discover(&mut board, &tree)?;
    board.console = console::discover(platform)?;
    #[cfg(not(feature = "qemu-virt"))]
    {
        board.allwinner_v821 = platform.allwinner_v821_registers();
    }
    devices::discover(&mut board, platform)?;
    #[cfg(not(feature = "qemu-virt"))]
    {
        board.spacemit_k1 = platform.spacemit_k1_registers()?;
        board.allwinner_v861 = platform.allwinner_v861_registers();
    }
    Ok(board)
}
