//! Firmware feature control.
//!
//! # References
//!
//! - Specification: [RISC-V SBI FWFT extension](https://docs.riscv.org/reference/sbi/v3.0/ext-firmware-features.html) —
//!   feature identifiers and get/set semantics.

use runtime::rustsbi::SbiRet;
use runtime::rustsbi::spec::fwft::feature_type;

use crate::riscv::csr::{CSR_MENVCFG, menvcfg};

// `menvcfg` fields defined by the corresponding RISC-V extensions, where the privileged
// specification places them ("Machine Environment Configuration Register (menvcfg)").
const ENVCFG_LPE: usize = 1 << 2; // Landing pad (Zicfilp)
const ENVCFG_SSE: usize = 1 << 3; // Shadow stack (Zicfiss)
// On rv32 DTE and ADUE are in menvcfgh, which this module has no path to, so they are refused
// there; PMM is RV64-only in the specification.
#[cfg(target_pointer_width = "64")]
const ENVCFG_PMM_SHIFT: usize = 32; // Pointer masking tag length (Smnpm)
#[cfg(target_pointer_width = "64")]
const ENVCFG_PMM: usize = 0b11 << ENVCFG_PMM_SHIFT;
#[cfg(target_pointer_width = "64")]
const ENVCFG_DTE: usize = 1 << 59; // Double trap (Smdbltrp)
#[cfg(target_pointer_width = "64")]
const ENVCFG_ADUE: usize = 1 << 61; // PTE A/D hardware updating (Svadu)

/// Whether no two of `fields` share a bit.
const fn disjoint(fields: &[usize]) -> bool {
    let mut seen = 0;
    let mut i = 0;
    while i < fields.len() {
        if seen & fields[i] != 0 {
            return false;
        }
        seen |= fields[i];
        i += 1;
    }
    true
}

// No feature this extension sets can reach a field the firmware sets itself, nor FIOM (bit 0).
// On rv32 the cast keeps the low word, the only one this extension writes there.
const ENVCFG_FIOM: u64 = 1 << 0;
const FIRMWARE: usize =
    (menvcfg::CBIE | menvcfg::CBCFE | menvcfg::CBZE | menvcfg::PBMTE | menvcfg::STCE | ENVCFG_FIOM)
        as usize;
const _: () = assert!(disjoint(&[FIRMWARE, ENVCFG_LPE, ENVCFG_SSE]));
#[cfg(target_pointer_width = "64")]
const _: () = assert!(disjoint(&[
    FIRMWARE,
    ENVCFG_LPE,
    ENVCFG_SSE,
    ENVCFG_PMM,
    ENVCFG_DTE,
    ENVCFG_ADUE
]));

/// Firmware Features extension backed by narrow Runtime operations.
///
/// Misaligned exception delegation and `menvcfg` are trap-sensitive CSR
/// mechanism, so every register access goes through the Runtime: the
/// misaligned-delegation bits through a dedicated narrow operation, and
/// `menvcfg` through the Runtime's guarded CSR leaves.
pub(crate) struct SbiFwft;

impl SbiFwft {
    fn has_s_mode() -> bool {
        riscv::register::misa::read().has_extension('S')
    }

    fn menvcfg_read() -> Option<usize> {
        runtime::trap::read_csr_guarded::<CSR_MENVCFG>().ok()
    }

    fn menvcfg_write(value: usize) -> bool {
        runtime::trap::write_csr_guarded::<CSR_MENVCFG>(value).is_ok()
    }

    fn menvcfg_bit(feature_id: usize) -> Option<usize> {
        match feature_id {
            feature_type::LANDING_PAD => Some(ENVCFG_LPE),
            feature_type::SHADOW_STACK => Some(ENVCFG_SSE),
            #[cfg(target_pointer_width = "64")]
            feature_type::DOUBLE_TRAP => Some(ENVCFG_DTE),
            #[cfg(target_pointer_width = "64")]
            feature_type::PTE_AD_HW_UPDATING => Some(ENVCFG_ADUE),
            _ => None,
        }
    }

    fn set_menvcfg_bit(bit: usize, value: usize) -> SbiRet {
        if value > 1 {
            return SbiRet::invalid_param();
        }
        let Some(current) = Self::menvcfg_read() else {
            return SbiRet::not_supported();
        };
        let next = if value == 1 {
            current | bit
        } else {
            current & !bit
        };
        if !Self::menvcfg_write(next) {
            return SbiRet::not_supported();
        }
        let Some(read_back) = Self::menvcfg_read() else {
            return SbiRet::not_supported();
        };
        // WARL fields may ignore writes when the backing extension is absent.
        if (read_back & bit) != (next & bit) {
            return SbiRet::not_supported();
        }
        SbiRet::success(0)
    }

    #[cfg(target_pointer_width = "64")]
    fn set_pmm(value: usize) -> SbiRet {
        if value > 3 {
            return SbiRet::invalid_param();
        }
        let Some(current) = Self::menvcfg_read() else {
            return SbiRet::not_supported();
        };
        let next = (current & !ENVCFG_PMM) | (value << ENVCFG_PMM_SHIFT);
        if !Self::menvcfg_write(next) {
            return SbiRet::not_supported();
        }
        let Some(read_back) = Self::menvcfg_read() else {
            return SbiRet::not_supported();
        };
        // PMM is WARL and may reject tag lengths unsupported by Smnpm.
        if (read_back & ENVCFG_PMM) != (next & ENVCFG_PMM) {
            return SbiRet::not_supported();
        }
        SbiRet::success(0)
    }

    // Probe whether WARL fields retain set bits, then attempt to restore
    // the original value.
    fn menvcfg_bits_supported(mask: usize) -> bool {
        let Some(current) = Self::menvcfg_read() else {
            return false;
        };
        if !Self::menvcfg_write(current | mask) {
            return false;
        }
        let Some(probed) = Self::menvcfg_read() else {
            let _ = Self::menvcfg_write(current);
            return false;
        };
        let _ = Self::menvcfg_write(current);
        (probed & mask) != 0
    }
}

impl runtime::rustsbi::Fwft for SbiFwft {
    fn set(&self, feature_id: u32, value: usize, flags: usize) -> SbiRet {
        // The LOCK flag is not supported: locked features can never be
        // modified again, which would prevent firmware reconfiguration.
        if flags != 0 {
            return SbiRet::invalid_param();
        }
        match feature_id as usize {
            feature_type::MISALIGNED_EXC_DELEG => {
                if !Self::has_s_mode() {
                    return SbiRet::not_supported();
                }
                if value > 1 {
                    return SbiRet::invalid_param();
                }
                runtime::trap::set_misaligned_delegation(value == 1);
                SbiRet::success(0)
            }
            #[cfg(target_pointer_width = "64")]
            feature_type::POINTER_MASKING_PMLEN => Self::set_pmm(value),
            _ => match Self::menvcfg_bit(feature_id as usize) {
                Some(bit) => Self::set_menvcfg_bit(bit, value),
                None => SbiRet::not_supported(),
            },
        }
    }

    fn get(&self, feature_id: u32) -> SbiRet {
        match feature_id as usize {
            feature_type::MISALIGNED_EXC_DELEG => {
                if !Self::has_s_mode() {
                    return SbiRet::not_supported();
                }
                SbiRet::success(runtime::trap::misaligned_delegated() as usize)
            }
            #[cfg(target_pointer_width = "64")]
            feature_type::POINTER_MASKING_PMLEN => {
                if !Self::menvcfg_bits_supported(ENVCFG_PMM) {
                    return SbiRet::not_supported();
                }
                match Self::menvcfg_read() {
                    Some(value) => SbiRet::success((value & ENVCFG_PMM) >> ENVCFG_PMM_SHIFT),
                    None => SbiRet::not_supported(),
                }
            }
            _ => match Self::menvcfg_bit(feature_id as usize) {
                Some(bit) => {
                    if !Self::menvcfg_bits_supported(bit) {
                        return SbiRet::not_supported();
                    }
                    match Self::menvcfg_read() {
                        Some(value) => SbiRet::success(((value & bit) != 0) as usize),
                        None => SbiRet::not_supported(),
                    }
                }
                None => SbiRet::not_supported(),
            },
        }
    }
}
