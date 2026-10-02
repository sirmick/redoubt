//! What a manifest will cost `init` in `root` (kernel/budgets.md, "The tree from the boot
//! manifest"): a bound, computed before `init` creates anything, on every page its calls charge to
//! `root`, priced as kernel/objects.md's cost table prices them. `init` compares it with what
//! `root` has free and refuses a boot it cannot run in; a charge that fails later is a bug in
//! this bound, and refuses the boot too.
//!
//! The kernel exports no prices, so the ones `init` pays are named here, each from the table, and
//! `tests/bound.rs` reads each back from the page.

use redoubt_rt::abi::PAGE_SIZE;

/// An endpoint's page, charged to its owner (kernel/objects.md, "What objects cost").
pub const ENDPOINT_PAGES: u64 = 1;
/// A process object's page, charged to the creator's budget: `init`'s, `root`.
pub const PROCESS_OBJECT_PAGES: u64 = 1;
/// The handles one handle-table page holds (kernel/objects.md: page 0 holds 1 to 64).
pub const HANDLES_PER_TABLE_PAGE: u64 = 64;
/// The entries of one page-table page: the fewer of the two widths' (Sv39's 512).
pub const TABLE_ENTRIES: u64 = 512;
/// A startup block's page.
pub const BLOCK_PAGES: u64 = 1;
/// A thread's IPC page, charged to the budget its process runs in: `init`'s, `root`.
pub const THREAD_IPC_PAGES: u64 = 1;
/// The stack of the thread that watches one server's exit endpoint: it only receives and prints.
pub const WATCH_STACK_PAGES: u64 = 4;
/// The handles `init` keeps or mints per server beside its endpoints and badges: its budget,
/// its process, its exit endpoint and its console connection.
pub const HANDLES_PER_SERVER: u64 = 4;
/// The connections `init` holds as a caller itself: `keyd` (to ask `holds`), `consoled` (its
/// own console) and `bootfsd` (to push the public entries).
pub const INIT_CALLER_HANDLES: u64 = 3;

/// What the bound is computed from: the manifest's counts, and what `init` holds at the start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// The `servers` entries.
    pub servers: u64,
    /// The endpoints the servers receive on.
    pub endpoints: u64,
    /// The `handed` items: one badged handle `init` mints for each.
    pub handed: u64,
    /// The largest one launch copies through `init`'s pages: the stub, the image and the stack,
    /// in bytes each, before `process_map` moves them to the child.
    pub stub_bytes: u64,
    pub largest_image_bytes: u64,
    pub stack_pages: u64,
    /// The handles in `init`'s table when it starts: the budgets, the Reset right and every
    /// device.
    pub handles_at_start: u64,
    /// The arena `init` takes once, in pages.
    pub arena_pages: u64,
}

fn pages(bytes: u64) -> u64 { bytes.div_ceil(PAGE_SIZE as u64) }

/// The page-table pages one mapping of `pages` pages may need: a leaf table for each
/// `TABLE_ENTRIES` it spans, one more where it straddles a boundary, and one table above them.
fn tables(pages: u64) -> u64 { if pages == 0 { 0 } else { pages.div_ceil(TABLE_ENTRIES) + 2 } }

/// The bound, in pages, on what `init`'s calls charge to `root` for this boot, saturating.
pub fn bound(c: &Counts) -> u64 {
    // Every endpoint a server receives on, and an exit endpoint per server.
    let endpoints = sum(&[c.endpoints, c.servers]).saturating_mul(ENDPOINT_PAGES);
    let processes = c.servers.saturating_mul(PROCESS_OBJECT_PAGES);
    let blocks = c.servers.saturating_mul(BLOCK_PAGES + tables(BLOCK_PAGES));
    // A thread per server watching its exit endpoint: its IPC page, its stack and their tables.
    let watchers = c.servers.saturating_mul(THREAD_IPC_PAGES + WATCH_STACK_PAGES + tables(WATCH_STACK_PAGES));
    // One launch at a time holds its copies in `init`'s pages until they move.
    let (stub, image) = (pages(c.stub_bytes), pages(c.largest_image_bytes));
    let launch = sum(&[stub, tables(stub), image, tables(image), c.stack_pages, tables(c.stack_pages)]);
    // The handle table's growth: every page the added handles may open beyond those in use.
    let added =
        sum(&[c.endpoints, c.handed, c.servers.saturating_mul(HANDLES_PER_SERVER), INIT_CALLER_HANDLES]);
    let table = c.handles_at_start.saturating_add(added).div_ceil(HANDLES_PER_TABLE_PAGE)
        - c.handles_at_start.div_ceil(HANDLES_PER_TABLE_PAGE);
    let arena = sum(&[c.arena_pages, tables(c.arena_pages)]);
    sum(&[endpoints, processes, blocks, watchers, launch, table, arena])
}

fn sum(parts: &[u64]) -> u64 { parts.iter().fold(0u64, |sum, part| sum.saturating_add(*part)) }

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Counts { Counts { handles_at_start: 10, ..Counts::default() } }

    #[test]
    fn an_empty_manifest_costs_the_arena_and_init_s_own_connections() {
        let c = Counts { arena_pages: 512, ..none() };
        // The arena, its tables (1 + 2), and no table page: 10 + 3 handles fit page 0.
        assert_eq!(bound(&c), 512 + 3);
    }

    #[test]
    fn each_server_adds_its_endpoints_process_and_block() {
        let one = Counts { servers: 1, endpoints: 1, ..none() };
        let two = Counts { servers: 2, endpoints: 2, ..none() };
        // A receive endpoint and an exit endpoint, a process object, a block and its tables, and
        // the watching thread's IPC page, stack and the stack's tables.
        assert_eq!(bound(&two) - bound(&one), 2 + 1 + 1 + 3 + 1 + 4 + 3);
    }

    #[test]
    fn the_largest_launch_counts_once_and_whole() {
        let c =
            Counts { stub_bytes: 1, largest_image_bytes: PAGE_SIZE as u64 + 1, stack_pages: 16, ..none() };
        assert_eq!(bound(&c), 1 + 3 + 2 + 3 + 16 + 3);
    }

    #[test]
    fn handle_table_pages_count_only_past_those_in_use() {
        let at = |handles_at_start, handed| bound(&Counts { handles_at_start, handed, ..Counts::default() });
        // 61 + 3 init handles fill page 0 exactly; one more opens page 1.
        assert_eq!(at(61, 0), 0);
        assert_eq!(at(61, 1), 1);
        assert_eq!(at(64, 0), 1);
        assert_eq!(at(64, 64 * 3), 4);
    }

    #[test]
    fn a_hostile_count_saturates_rather_than_wraps() {
        let c = Counts { servers: u64::MAX, handed: u64::MAX, ..none() };
        assert_eq!(bound(&c), u64::MAX);
    }
}
