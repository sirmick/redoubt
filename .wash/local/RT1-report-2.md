# RT1, second report: keepers, SAFETY, pages

Three new commits on wp-rt1 after 5b5a6bfe2 (HEAD b7f22cd53, tree clean apart from the report
files):

- b4c530f1e rt: each unsafe in the heap, the page buffers and premapped names its guarantor
- a71fc6f0e rt: a device's registers are reached under Miri, to the last byte and no further
- b7f22cd53 docs: the runtime's unsafe is few, each names its guarantor, and rt-miri attacks it

## Keeper per remaining site (10)

| Site | Guarantor named in its SAFETY comment | Keeper |
| --- | --- | --- |
| GlobalAlloc impl, alloc, dealloc (3) | the heap's lists under `Locked`; the kernel (`map_anon`); the caller (GlobalAlloc's contract) | tests/heap.rs under Miri (rt-miri) |
| heap `words`, `set_words` (2) | the kernel (`map_anon`); the heap; `Locked` | tests/heap.rs under Miri, plus the const assert beside MIN_SMALL |
| Mapping `bytes`, `bytes_mut` (2) | the kernel (map_anon, transfer and lend R3/R4, completion); `Mapping` | mapping_views under Miri |
| Registers `read_u8`, `write_u8` (2) | the kernel (`map_device`); `Registers` (consumed by unmap, !Sync) | tests/registers.rs `registers_reach_exactly_their_mapping` under Miri |
| `premapped` (1) | the loader (bundle); the parent through the stub (startup page) | target only: `bundle-mapped`, `init-boot`, `init-servers` |

The Registers test runs on a one-page device. It reads and writes byte 0 and the last byte, and
checks that `len`, `len+1` and `usize::MAX` are refused for both read and write. The writes are
seen through the fake's hardware view.

## Fake kernel (granted)

`device()` keeps its pointer in `anon`, beside MapAnon's. The doc on `anon` now covers both.
Nothing else in the fake changes.

## Pages

- native.md: a new last bullet in the runtime section, with N = 10, naming the heap, the views
  and the registers under Miri.
- testbench.md: the rt-miri paragraph adds `heap` and `registers`.
- The rt-miri case's description adds device registers.

## Gates (through in-dev, each exit 0)

- `cargo testbench`: rt-miri (90 s, with heap and registers in its list), rt-host-tests,
  netd-host-tests, rt-build on rv64 and rv32, unsafe-budget (10), size-budget (2920, unchanged
  by this round), docs.
- `cargo +nightly fmt --check -p redoubt-rt -p redoubt-fake-kernel`.
- The whole bench was not run.

## Found, not changed (outside what RT1 writes)

The native.md bullet "No safe call pulls memory from under its owner" says "`unmap` is private to
the heap and `Buffer`". `Dma` and `Registers` call it too; the same bullet says so further on.
That is a one-word fix if you want it in RT1.

## Rebase

When FSD1 merges, the size line becomes main's ceiling plus 6, with the same reason.
