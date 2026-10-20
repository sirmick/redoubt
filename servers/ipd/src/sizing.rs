//! How big `ipd` is allowed to get, from its arguments (servers/ipd.md, "Sizing"): admission for
//! the 9P skeleton's buckets, the sockets `ipd` counts itself, and the checks that every bucket at
//! its cap fits the budget and every socket's buffers the heap, or `ipd` does not start.

use alloc::vec::Vec;

use crate::args::{BadArgs, Config};

/// Parked calls one default bucket may hold (servers/ipd.md, "Sizing").
pub const DEFAULT_IN_FLIGHT: u32 = 5;
/// Sockets one default bucket may hold.
pub const DEFAULT_SOCKETS: u32 = 8;
/// Connections one default bucket may mint (`new_connection`, `grant`).
pub const DEFAULT_STATE: u32 = 4;
/// Multiplexed requests one bucket may hold (servers/serving.md, `admit`), and the pages their
/// transfers may hold: as `littlefsd`'s. Overrides keep them.
pub const REQUESTS: u32 = 128;
pub const PAGES: u32 = 32;
/// What one socket costs the heap: its two buffers, a large block of two pages each, and its slot
/// in smoltcp's set with two entries of `ipd`'s table, 912 bytes on 64-bit (asserted in
/// `crate::stack`), both tables sized at start.
pub const SOCKET_BYTES: u64 = 2 * crate::stack::BUFFER as u64 + 1024;
/// The heap `ipd` holds beside its sockets, in pages: twice its largest measured heap peak apart
/// from its socket tables, 37 pages (testbench.md, "The memory budget").
pub const HEAP_BASE_PAGES: u64 = 74;
/// What a parked call costs: the lend it holds (at most `MAX_LEND_PAGES`) until it is answered.
pub const PARKED_BYTES: u64 = redoubt_rt::abi::MAX_LEND_PAGES as u64 * redoubt_rt::abi::PAGE_SIZE as u64;
/// What a fid and a minted connection cost, generously.
pub const FID_BYTES: u64 = 512;
pub const CONNECTION_BYTES: u64 = 512;
const PAGE: u64 = redoubt_rt::abi::PAGE_SIZE as u64;

/// The fids a bucket may hold, for a socket cap: a `ctl` and a `data` for each socket, and room
/// for the directories and `clone`; at most the skeleton's per-connection limit.
pub fn files_for(sockets: u32) -> u32 { (2 * sockets + 8).min(redoubt_rt::server::ninep::MAX_FIDS as u32) }

/// The least heap cap, in pages, for `max_sockets` sockets: the base and every socket's buffers
/// (servers/ipd.md, "Sizing").
pub fn heap_pages_for(max_sockets: u64) -> u64 {
    HEAP_BASE_PAGES.saturating_add(max_sockets.saturating_mul(SOCKET_BYTES).div_ceil(PAGE))
}

/// A bucket's `State` units: its connections and its sockets, which are paid for in the same
/// resource.
pub fn state_for(state: u32, sockets: u32) -> u32 { state.saturating_add(sockets) }

/// How `ipd` is sized: admission for the skeleton, and the socket caps `ipd` counts itself.
pub struct Sizing {
    pub admission: redoubt_rt::server::Admission,
    pub caps: crate::fs::SocketCaps,
    /// The most sockets that can exist at once: every bucket at its cap.
    pub max_sockets: usize,
    /// What every bucket at its cap costs together, in bytes: the entry rule's term
    /// (servers/walfsd.md, "Serving"), which the budget held.
    pub need: u64,
    /// The least heap cap, in pages, that holds every socket beside the base ([`heap_pages_for`]).
    pub heap_pages: u64,
}

impl Sizing {
    /// Whether a heap cap of `heap_pages` holds every socket beside the base; with no cap (0) the
    /// budget holds them instead ([`Sizing::budget_need`]).
    pub fn heap_holds(&self, heap_pages: u64) -> bool { heap_pages == 0 || heap_pages >= self.heap_pages }

    /// What the budget must hold beside a heap cap of `heap_pages`: every bucket at its caps, and
    /// with no cap (0) every socket's heap as well.
    pub fn budget_need(&self, heap_pages: u64) -> u64 {
        match heap_pages {
            0 => self.need.saturating_add(self.heap_pages.saturating_mul(PAGE)),
            _ => self.need,
        }
    }
}

impl Config {
    /// Sizes `ipd` from its arguments, or refuses to start. The overrides must be root scope
    /// badges or non-zero accounts. The worst case, every override's bucket and every other bucket
    /// at the defaults, must leave `MAX_OPEN_CALLS`' headroom and fit `budget` bytes, its manifest
    /// entry's pages less its heap cap and stack ([`redoubt_rt::server::budget`]). And the heap
    /// must hold every socket's buffers beside its base ([`heap_pages_for`]): a heap cap of
    /// `heap_pages` at least that, or with no cap (0) the budget holds it beside the buckets.
    ///
    /// Sockets are `State` units ([`state_for`]), so a bucket's sockets are bounded by its units.
    /// Each socket is counted once, where it lives: its buffers in the heap term, its unit in the
    /// budget's at a connection's cost. The stack's bound on live sockets is those units at their
    /// worst: an override below the default can be idle while a default bucket takes its slot, so
    /// each override slot counts `max(its units, the default's)`, as the admission counts its caps.
    pub fn sizing(&self, budget: u64, heap_pages: u64) -> Result<Sizing, BadArgs> {
        use redoubt_rt::server::{Admission, Cost, Limits, Override};
        let limits = Limits {
            buckets: self.buckets,
            in_flight: DEFAULT_IN_FLIGHT,
            files: files_for(DEFAULT_SOCKETS),
            state: state_for(DEFAULT_STATE, DEFAULT_SOCKETS),
            requests: REQUESTS,
            pages: PAGES,
        };
        let mut overrides = Vec::new();
        for l in &self.limits {
            overrides.push(Override {
                account: l.account,
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
        let cost = Cost {
            in_flight: PARKED_BYTES,
            file: FID_BYTES,
            state: CONNECTION_BYTES,
            request: redoubt_rt::server::ninep::REQUEST_STATE,
            page: PAGE,
        };
        let need = admission.need(&cost).filter(|need| *need <= budget);
        let Some(need) = need else { return Err(BadArgs("every bucket at its cap does not fit the budget")) };
        // The stack's own per-bucket count agrees with the admission's cap; the admission, with its
        // shares, is what binds.
        let caps = crate::fs::SocketCaps { default: default as usize, overrides: overrides.clone() };
        let heap = heap_pages_for(max_sockets);
        let sizing = Sizing { admission, caps, max_sockets: max_sockets as usize, need, heap_pages: heap };
        if !sizing.heap_holds(heap_pages) || sizing.budget_need(heap_pages) > budget {
            return Err(BadArgs("the heap does not hold every socket's buffers"));
        }
        Ok(sizing)
    }
}
