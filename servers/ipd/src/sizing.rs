//! How big `ipd` is allowed to get, from its arguments (NAMESPACES.md, the milestone manifest;
//! answer 174): admission for the 9P skeleton's buckets, the sockets `ipd` counts itself, and
//! the check that every bucket at its cap fits, or `ipd` does not start.

use alloc::vec::Vec;

use crate::args::{BadArgs, Config};

/// Parked calls one default bucket may hold (NAMESPACES.md, the milestone manifest).
pub const DEFAULT_IN_FLIGHT: u32 = 5;
/// Sockets one default bucket may hold.
pub const DEFAULT_SOCKETS: u32 = 8;
/// Connections one default bucket may mint (`new_connection`, `grant`).
pub const DEFAULT_STATE: u32 = 4;
/// The bytes `ipd` is sized to fit: every bucket at its cap, and the stack itself.
pub const BUDGET: u64 = 8 << 20;
/// What `ipd` itself takes before any client: code, the interface, the tables.
pub const OWN_USE: u64 = 1 << 20;
/// What one socket costs: its two buffers and smoltcp's state for it.
pub const SOCKET_BYTES: u64 = 2 * crate::stack::BUFFER as u64 + 1024;
/// What a parked call costs: the lend it holds (at most `MAX_LEND_PAGES`) until it is answered.
pub const PARKED_BYTES: u64 = redoubt_rt::abi::MAX_LEND_PAGES as u64 * redoubt_rt::abi::PAGE_SIZE as u64;
/// What a fid and a minted connection cost, generously.
pub const FID_BYTES: u64 = 512;
pub const CONNECTION_BYTES: u64 = 512;

/// The fids a bucket may hold, for a socket cap: a `ctl` and a `data` for each socket, and room
/// for the directories and `clone`; at most the skeleton's per-connection limit.
pub fn files_for(sockets: u32) -> u32 { (2 * sockets + 8).min(redoubt_rt::server::ninep::MAX_FIDS as u32) }

/// How `ipd` is sized: admission for the skeleton, and the socket caps `ipd` counts itself.
pub struct Sizing {
    pub admission: redoubt_rt::server::Admission,
    pub caps: crate::fs::SocketCaps,
    /// The most sockets that can exist at once: every bucket at its cap.
    pub max_sockets: usize,
}

impl Config {
    /// Sizes `ipd` from its arguments, or refuses to start (answer 174): the overrides must be
    /// root scope badges, and the worst case, every override's bucket and every other bucket at
    /// the defaults, must leave `MAX_OPEN_CALLS`' headroom and fit [`BUDGET`].
    pub fn sizing(&self) -> Result<Sizing, BadArgs> {
        use redoubt_rt::server::{Admission, Cost, Limits, Override};
        let limits = Limits {
            buckets: self.buckets,
            in_flight: DEFAULT_IN_FLIGHT,
            files: files_for(DEFAULT_SOCKETS),
            state: DEFAULT_STATE,
        };
        let mut overrides = Vec::new();
        for l in &self.limits {
            overrides.push(Override {
                badge: l.badge,
                in_flight: l.in_flight,
                files: files_for(l.sockets),
                state: l.state,
            });
        }
        let admission = Admission::with_overrides(limits, &overrides)
            .map_err(|_| BadArgs("the parked calls every bucket may hold leave no headroom"))?;
        let rest = u64::from(self.buckets).saturating_sub(self.limits.len() as u64);
        let max_sockets =
            self.limits.iter().map(|l| u64::from(l.sockets)).sum::<u64>() + rest * u64::from(DEFAULT_SOCKETS);
        let cost = Cost { in_flight: PARKED_BYTES, file: FID_BYTES, state: CONNECTION_BYTES };
        let left = BUDGET
            .checked_sub(OWN_USE + max_sockets * SOCKET_BYTES)
            .ok_or(BadArgs("the sockets do not fit the budget"))?;
        if !admission.fits(&cost, left) {
            return Err(BadArgs("every bucket at its cap does not fit the budget"));
        }
        let caps = crate::fs::SocketCaps {
            default: DEFAULT_SOCKETS as usize,
            overrides: self.limits.iter().map(|l| (l.badge, l.sockets as usize)).collect(),
        };
        Ok(Sizing { admission, caps, max_sockets: max_sockets as usize })
    }
}
