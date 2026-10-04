# RT1, first report: three removals, heap under Miri

Branch wp-rt1, base 2ab6dc81c, 5 commits, tree clean:

- 927c55e28 rt: the heap reaches a free block's words through one private pair
- 831073c1f rt: the startup page and the bundle are viewed through one private fn
- 688b9f717 rt: the fake kernel keeps the pointer of every page it maps anonymously
- 3c9549ccd rt: the heap's tests run under Miri in the bench
- 5b5a6bfe2 rt: a program names its panic hook in entry!, not at run time

## Count, site by site (libs/rt/src): 14 -> 10

| Site | Before | After |
| --- | --- | --- |
| heap.rs push write, pop read, run read, set_run write | 4 | 2 (`words`, `set_words`) |
| heap.rs `unsafe impl GlobalAlloc`, `alloc`, `dealloc` | 3 | 3 |
| handle.rs Registers read_u8 / write_u8 | 2 | 2 |
| ipc.rs Mapping's two views | 2 | 2 |
| start.rs startup page view, bundle view | 2 | 1 (`premapped`) |
| start.rs panic hook transmute | 1 | 0 |

The ratchet row's name now lists the sites that remain. Its comment says why the views stay two.
The rename carries its `Unsafe budget:` line.

## Deliverable 1: the form that landed

`panic_handler!()` / `panic_handler!(hook)` emits the `#[panic_handler]` (target only), which
calls `start::panic(info, Option<fn()>)`. `entry!(run, panic_hook = f)` and `first_entry!`
invoke it. `set_panic_hook` and `PANIC_HOOK` are gone. `run_panic_hook(hook)` keeps
HOOK_RAN/HOOK_DONE.

- netd: `panic_reset` is pub and named in `entry!`. `arm_panic_reset` only stores the registers.
- netd test: `a_panic_resets_the_device` calls `run_panic_hook(panic_reset)`.
- Four bins in tests/programs get `redoubt_rt::panic_handler!();` (granted).
- Docs: native.md "Start and end" gets the brief's sentence. serving.md: "if it set one" becomes
  "if it names one in `entry!`".

## Miri

`libs/rt/tests/heap.rs` holds heap_over_map_anon and heap_in_a_fixed_arena, moved from
tests/ipc.rs. The random test runs 1,000 rounds under cfg(miri). The file is in rt-miri's list
beside mapping_views.

The fake kernel's MapAnon keeps its pointer in `State::anon` (granted), so the heap's pages,
which by design are never unmapped, are not reported as leaks. Leak checks stay on for every
file.

## Deviation: the two-words check is a compile-time assert, not a #[test]

`const _: () = assert!(...)` sits beside `MIN_SMALL`. A host test only ever sees 64-bit. The
const assertion fails the build on rv32 as well as rv64, and rt-build compiles both.

## Size: libs/rt 2914 -> 2920 code lines (raised, with a `Size budget:` line)

- +3: the handler macro each program expands, net of what was removed.
- +2: `premapped`.
- +1: the const assertion.

The size count excludes test items.

## Gates (each through in-dev; exit codes)

- `cargo testbench`: 0 for each of unsafe-budget, size-budget, rt-host-tests, netd-host-tests,
  rt-miri (92 s), docs, and every *-build case on rv64 and rv32.
- `cargo build --release` of test-programs, redoubt-init-programs and redoubt-net-client, for
  riscv64imac and riscv32imac: 0. No build case covers these.
- `cargo +nightly fmt --check` for rt, netd, the fake kernel and test-programs: 0.
- The whole bench was not run, as instructed.

## Next (not started)

- Keeper tests: a Registers test under Miri over the fake's `device` memory, which the fake
  allocates for real, so it looks feasible. Named target keepers for `premapped`.
- The SAFETY rewrite, naming guarantors.
- native.md's new runtime bullet, and testbench.md's rt-miri sentence.
