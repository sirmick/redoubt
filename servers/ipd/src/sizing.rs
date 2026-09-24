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

/// A bucket's `State` units: its connections and its sockets, which are paid for in the same
/// resource (QA D3-code-review-5, P2-2).
pub fn state_for(state: u32, sockets: u32) -> u32 { state.saturating_add(sockets) }

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
    ///
    /// Sockets are `State` units ([`state_for`]), so the admission's own worst case covers their
    /// memory, each unit costed as a socket (a connection costs less), and a bucket's sockets are
    /// bounded by its units. The stack's bound on live sockets is those units at their worst: an
    /// override below the default can be idle while a default bucket takes its slot, so each
    /// override slot counts `max(its units, the default's)`, as the admission counts its caps
    /// (QA D3-code-review-3 and -5).
    pub fn sizing(&self) -> Result<Sizing, BadArgs> {
        use redoubt_rt::server::{Admission, Cost, Limits, Override};
        let limits = Limits {
            buckets: self.buckets,
            in_flight: DEFAULT_IN_FLIGHT,
            files: files_for(DEFAULT_SOCKETS),
            state: state_for(DEFAULT_STATE, DEFAULT_SOCKETS),
        };
        let mut overrides = Vec::new();
        for l in &self.limits {
            overrides.push(Override {
                badge: l.badge,
                in_flight: l.in_flight,
                files: files_for(l.sockets),
                state: state_for(l.state, l.sockets),
            });
        }
        let admission = Admission::with_overrides(limits, &overrides)
            .map_err(|_| BadArgs("the parked calls every bucket may hold leave no headroom"))?;
        // A bucket's sockets are bounded by its `State` units, so the stack's bound is theirs at
        // their worst: each override slot at max(its units, the default's).
        let default = state_for(DEFAULT_STATE, DEFAULT_SOCKETS);
        let rest = u64::from(self.buckets).saturating_sub(self.limits.len() as u64);
        let max_sockets =
            self.limits.iter().map(|l| u64::from(state_for(l.state, l.sockets).max(default))).sum::<u64>()
                + rest * u64::from(default);
        let cost =
            Cost { in_flight: PARKED_BYTES, file: FID_BYTES, state: SOCKET_BYTES.max(CONNECTION_BYTES) };
        if !admission.fits(&cost, BUDGET - OWN_USE) {
            return Err(BadArgs("every bucket at its cap does not fit the budget"));
        }
        // The stack's own per-bucket count agrees with the admission's cap; the admission, with its
        // shares, is what binds.
        let caps = crate::fs::SocketCaps {
            default: default as usize,
            overrides: self
                .limits
                .iter()
                .map(|l| (l.badge, state_for(l.state, l.sockets) as usize))
                .collect(),
        };
        Ok(Sizing { admission, caps, max_sockets: max_sockets as usize })
    }
}
