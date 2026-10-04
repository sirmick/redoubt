# INIT2 deliverable 2: four questions, each with a recommendation (init2-implementer-2)

## (a) bootfsd's arguments vs `public`
Page: bootfsd.md "Started by `init`": "`init` starts `bootfsd` ... with the manifest's `public`
list as its arguments". Code: `servers/bootfsd/src/bin/bootfsd.rs` reads `buckets=N` from its args
and builds its table from `own_args(&args)` (the rest). The manifest's bootfsd entry also carries
`args` (`buckets=4` in image/manifest.json).
Options: (1) init appends the `public` names after the manifest entry's own `args`; (2) the
manifest repeats them in bootfsd's `args` and init checks they match `public`; (3) bootfsd's
manifest `args` are refused and init writes `buckets=` itself.
**Recommend (1):** the page's sentence holds, no duplication in the manifest, bootfsd unchanged.
`check::blocks` then sizes bootfsd's startup block with the public names appended, so a long
`public` list is refused before boot (Why::Block), not at launch. Manifest `args` on bootfsd that
are not `buckets=` would then be refused as a public name the bundle lacks (bootfsd refuses
them anyway): I would refuse them in `check::public` with Why::Argument.

## (b) rt's heap vs ruling 1's fixed arena
Ruling 1: "`init`'s heap is a fixed arena, taken once at start." `libs/rt/src/lib.rs` makes
`heap::Heap` (over `map_anon`, on demand, pages never bounded) the `#[global_allocator]` for every
target program, unconditionally; a program cannot install its own. libs/rt is not INIT2's (only
admit.rs).
Options: (1) a small libs/rt change: a cargo feature (e.g. `own-heap`) that leaves the global
allocator out, and init installs its own `GlobalAlloc`, a bump/free-list over one `map_anon` of
`ARENA_PAGES` taken at start; (2) the same, but rt's `Heap` gains a fixed-region constructor that
init uses (refuses beyond the region); (3) keep rt's heap and count its growth in the bound
instead (contradicts ruling 1).
**Recommend (2):** one constructor in heap.rs (`Heap::fixed(base, len)`: small classes and large
blocks carve from the region, never `map_anon`), plus the feature gate from (1); about 30 lines
in libs/rt, no K16 path. Needs your/the Architect's leave to touch libs/rt.

## (c) No `device_info` wrapper in rt
Options: (1) init calls `redoubt_sys::syscall(&Call::DeviceInfo { device })` directly, as
`tests/programs/src/rd.rs` does; (2) add a wrapper in libs/rt/handle.rs (`Device::info`).
**Recommend (1):** init already depends on redoubt-sys, no libs/rt or libs/sys change, a
three-line helper in init's bin.

## (d) Reading root's usage relative to the arena
Ruling 1's bound includes the arena; if init reads `budget_usage(root)` after mapping the arena,
the arena is already in root's usage and is counted twice (bound vs free).
Options: (1) read root's usage first, before taking the arena, and compare the whole bound
(arena included) with that free; (2) read after, and compare the bound less the arena.
**Recommend (1):** the bound stays one pure function whose value is what the boot costs root
from init's start; the order (usage, then arena, then read and check) is init's first three
lines. The page line from ruling 1 reads unchanged.
