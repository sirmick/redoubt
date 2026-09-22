//! QEMU virt interrupt-controller and reset-device discovery.

use runtime::node_is_enabled;
use serde_device_tree::buildin::Node;

use crate::devicetree::compatible_strings;
use crate::driver;
use crate::platform::info::BoardInfo;

pub(super) fn discover(
    board: &mut BoardInfo,
    platform: &runtime::PlatformView<'_>,
) -> runtime::Result<()> {
    visit_subtree(board, platform, platform.root())
}

fn visit_subtree<'tree>(
    board: &mut BoardInfo,
    platform: &runtime::PlatformView<'tree>,
    node: &Node<'tree>,
) -> runtime::Result<()> {
    if !node_is_enabled(node) {
        return Ok(());
    }
    if let Some(compatibles) = compatible_strings(node)
        && compatibles.iter().any(|compatible| {
            driver::ClintKind::from_fdt(compatible).is_some()
                || driver::SIFIVE_TEST_COMPATIBLES.contains(&compatible)
        })
    {
        let registers = platform
            .device_registers(node)?
            .and_then(|ranges| ranges.first().copied())
            .ok_or(runtime::Error::InvalidArgs)?;
        for compatible in compatibles.iter() {
            if let Some(kind) = driver::ClintKind::from_fdt(compatible) {
                board.clint = Some((registers, kind));
            }
            if driver::SIFIVE_TEST_COMPATIBLES.contains(&compatible) {
                board.reset = Some(registers);
            }
        }
    }
    for child in node.nodes() {
        visit_subtree(board, platform, &child.deserialize::<Node<'tree>>())?;
    }
    Ok(())
}
