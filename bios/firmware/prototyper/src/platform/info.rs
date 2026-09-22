//! Platform facts retained after inspecting the Platform Description.

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(not(feature = "qemu-virt"))]
use riscv_aia::Iid;
#[cfg(not(feature = "qemu-virt"))]
use runtime::SpacemitK1Registers;
#[cfg(not(feature = "qemu-virt"))]
use runtime::memory::PhysAddr;
use runtime::memory::{DeviceRegisterRange, PhysAddrRange};

use crate::cfg::NUM_HART_MAX;
use crate::driver;

pub(super) type HartEnableList = [bool; NUM_HART_MAX];

/// Address layout of the machine-level IMSIC interrupt files.
#[cfg(not(feature = "qemu-virt"))]
pub(crate) struct ImsicAddressLayout {
    pub(crate) machine_base: PhysAddr,
    pub(crate) hart_index_bits: u32,
    group_index_shift: u32,
    hart_index_shift: u32,
}

#[cfg(not(feature = "qemu-virt"))]
impl ImsicAddressLayout {
    pub(super) const fn new(
        machine_base: PhysAddr,
        hart_index_bits: u32,
        group_index_shift: u32,
        hart_index_shift: u32,
    ) -> Self {
        Self {
            machine_base,
            hart_index_bits,
            group_index_shift,
            hart_index_shift,
        }
    }

    pub(super) fn machine_file_address(
        &self,
        hart_index: u32,
        group_index: u32,
    ) -> Option<PhysAddr> {
        let group_offset = if group_index == 0 {
            0
        } else {
            usize::try_from(group_index)
                .ok()?
                .checked_shl(self.group_index_shift)?
        };
        let hart_offset = usize::try_from(hart_index)
            .ok()?
            .checked_shl(self.hart_index_shift)?;
        self.machine_base
            .checked_add(group_offset)?
            .checked_add(hart_offset)
    }
}

/// Machine-level IMSIC resources selected from the Platform Description.
#[cfg(not(feature = "qemu-virt"))]
pub(crate) struct ImsicInfo {
    pub(crate) layout: ImsicAddressLayout,
    pub(crate) num_ids: u16,
    pub(crate) ipi_iid: Iid,
    pub(crate) hart_files: [Option<DeviceRegisterRange>; NUM_HART_MAX],
}

/// Console resources selected from the `/chosen/stdout-path` node.
pub(crate) struct ConsoleInfo {
    pub(crate) registers: DeviceRegisterRange,
    pub(crate) kind: driver::ConsoleKind,
    #[cfg_attr(feature = "qemu-virt", allow(dead_code))]
    pub(crate) clock_hz: Option<u32>,
}

/// Hardware information used while initializing and serving the platform.
pub(crate) struct BoardInfo {
    pub(crate) ram_ranges: Vec<PhysAddrRange>,
    pub(crate) firmware_ram_range: Option<PhysAddrRange>,
    pub(crate) console: Option<ConsoleInfo>,
    pub(crate) reset: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) syscon_poweroff: Option<driver::SysconConfig>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) syscon_reboot: Option<driver::SysconConfig>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) sunxi_wdt_v104: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) sunxi_wdt_v105: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) sunxi_rtc_v203_gprcm: Option<DeviceRegisterRange>,
    pub(crate) clint: Option<(DeviceRegisterRange, driver::ClintKind)>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) imsic: Option<ImsicInfo>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) machine_aplic: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) thead_plic: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) spacemit_k1: Option<SpacemitK1Registers>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) v821_usb: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) andes_l2: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) plmt: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) plicsw: Option<DeviceRegisterRange>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) allwinner_v821: Option<runtime::AllwinnerV821Registers>,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) allwinner_v861: Option<runtime::AllwinnerV861Registers>,
    pub(crate) hart_count: usize,
    pub(crate) timebase_frequency_hz: Option<u32>,
    pub(crate) enabled_harts: HartEnableList,
    pub(crate) model: String,
    #[cfg(not(feature = "qemu-virt"))]
    pub(crate) spacemit_p1_pmic_reset: Option<(DeviceRegisterRange, driver::I2cAddress)>,
}

impl BoardInfo {
    pub(super) const fn empty() -> Self {
        Self {
            ram_ranges: Vec::new(),
            firmware_ram_range: None,
            console: None,
            reset: None,
            #[cfg(not(feature = "qemu-virt"))]
            syscon_poweroff: None,
            #[cfg(not(feature = "qemu-virt"))]
            syscon_reboot: None,
            #[cfg(not(feature = "qemu-virt"))]
            sunxi_wdt_v104: None,
            #[cfg(not(feature = "qemu-virt"))]
            sunxi_wdt_v105: None,
            #[cfg(not(feature = "qemu-virt"))]
            sunxi_rtc_v203_gprcm: None,
            clint: None,
            #[cfg(not(feature = "qemu-virt"))]
            imsic: None,
            #[cfg(not(feature = "qemu-virt"))]
            machine_aplic: None,
            #[cfg(not(feature = "qemu-virt"))]
            thead_plic: None,
            #[cfg(not(feature = "qemu-virt"))]
            spacemit_k1: None,
            #[cfg(not(feature = "qemu-virt"))]
            v821_usb: None,
            #[cfg(not(feature = "qemu-virt"))]
            andes_l2: None,
            #[cfg(not(feature = "qemu-virt"))]
            plmt: None,
            #[cfg(not(feature = "qemu-virt"))]
            plicsw: None,
            #[cfg(not(feature = "qemu-virt"))]
            allwinner_v821: None,
            #[cfg(not(feature = "qemu-virt"))]
            allwinner_v861: None,
            hart_count: 0,
            timebase_frequency_hz: None,
            enabled_harts: [false; NUM_HART_MAX],
            model: String::new(),
            #[cfg(not(feature = "qemu-virt"))]
            spacemit_p1_pmic_reset: None,
        }
    }

    #[cfg_attr(feature = "qemu-virt", allow(dead_code))]
    pub(crate) fn is_qemu_virt(&self) -> bool {
        self.model == "riscv-virtio,qemu"
    }

    pub(super) fn ram_range_containing(&self, range: PhysAddrRange) -> Option<PhysAddrRange> {
        self.ram_ranges
            .iter()
            .copied()
            .find(|ram| ram.start() <= range.start() && range.end() <= ram.end())
    }
}
