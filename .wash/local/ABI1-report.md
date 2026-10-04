# ABI1 report: the system-call ABI apart from its transport

Branch `wp-abi1` in `/home/mcloonan/redoubt/.worktrees/abi1`, from main 75245a114. Tip 3314623c8.
All commands are `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, run from the worktree.

## Commits

1. e7302b627 `sys: a call travels by a Transport, and the ecall is one`:
   `libs/sys/src/{lib.rs,ecall.rs}`, `tests/size-budget.toml` (libs/sys 1009 -> 1016).
2. 624b0f640 `rt: the runtime's one seam takes the installed transport, the ecall by default`:
   `libs/rt/src/{sys.rs,lib.rs}`, `libs/rt/Cargo.toml` (feature `installed-transport`),
   `libs/rt/fake/{Cargo.toml,src/lib.rs,src/scripted.rs}`, `libs/rt/tests/{mapping_views.rs,transport.rs}`,
   `docs/kernel/abi.md`, `docs/userland/native.md`, `tests/size-budget.toml` (libs/rt 2955 -> 2956).
3. 628db5fb8 `init: device_info goes through the runtime`: `libs/rt/src/handle.rs`
   (`handle::device_info`), `servers/init/src/bin/init.rs`, `tests/size-budget.toml` (libs/rt -> 2962).
4. 3314623c8 `docs: the system-call ABI is kept free of the MMU, a direction`: `docs/beyond/README.md`.

## What was built

- `redoubt_sys::Transport: Sync { fn call(&self, &Call) -> Result<Return, Error> }`, in `lib.rs`, with the
  brief's doc. `Ecall` (unit struct, riscv only) implements it with the old body, unchanged `asm!`.
  `redoubt_sys::syscall(call)` stays as `Ecall.call(call)`. Not `#[inline]`: the call stays as it was.
- `redoubt_rt::sys::syscall`: `target_os = "none"` without `installed-transport` calls
  `redoubt_sys::Ecall.call(call)` directly; on the host the `OnceLock<&'static dyn Transport>` slot.
  `none` + `installed-transport` is `compile_error!("installed-transport has no slot on the machine yet:
  its backend's package adds one")`; checked: building redoubt-rt for riscv64 with the feature stops with
  that error.
- `HostKernel` -> `redoubt_rt::Transport` (re-export of `redoubt_sys::Transport`, on every target);
  `install_host_kernel` -> `install_transport` (host only). The trait method is `call` (was `syscall`).
  The fake, `scripted.rs` and `mapping_views.rs` (its direct `kernel.syscall` -> `kernel.call`) follow.
  `HostKernel` was `Send + Sync`; `Transport` is `Sync` as the brief settles; the slot needs only `Sync`.
- New `redoubt_rt::handle::device_info(Handle) -> Result<DeviceInfo, Error>`, beside `Mmio`: a free
  function, because init probes handles before it knows whether each is Reset, MMIO or an IRQ.
  init's two calls use it; init's now-unused `Call` and `Return` imports go.

## The case

`libs/rt/tests/transport.rs::a_second_transport_sees_every_call`: a `Counting` transport records each
call's name and forwards to `fake()`. It is installed before `fake()` runs, so the fake's own install
is the ignored second one. An echo round trip (client `call` with 4 words, server `receive` + `finish`
echoing them) passes; then the counted names, sorted, equal the fake's own log for both processes
(`Fake::calls`), sorted, and include `call` and `receive`. If the counter were not on the path, its
list would be empty and the test would fail. Not added to `tests/rt-miri.toml`'s explicit list (not
my path); say if it should be.

## Every direct caller of `redoubt_sys::syscall`

- `servers/init/src/bin/init.rs:145,616` (DeviceInfo): now `redoubt_rt::handle::device_info`.
- `stub/src/main.rs`, `stub/src/lib.rs:52`, `stub/src/bin/fixture-child.rs`: unchanged (below the runtime).
- `tests/programs/src/rd.rs` (most calls) and the bins `timeouts`, `ipc-outcomes`, `budget-deadline`,
  `receive-bad-record`, `proc-attack`, `proc-lifecycle`, `endpoint-destroy-open-calls`, plus
  `tests/programs/src/sched.rs:1114`: unchanged (below the runtime).
- No other: kernel, loader, paging, layout, steward and the fuzz crates use redoubt-sys's types only.

## Sizes (consoled, `--release`, `size`: text data bss)

| | rv64 before | rv64 after | rv32 before | rv32 after |
| --- | --- | --- | --- | --- |
| text/data/bss | 54881/0/112 | 54881/0/112 | 55491/0/60 | 55491/0/60 |
| ELF file | 170384 | 170408 | 143488 | 143512 |

Loadable sections are identical; the file grows 24 bytes (symbol names). Before was built from
`git archive 75245a114` with its own target dir.

## Commands and exit codes

At the tip 3314623c8, each `cargo testbench -- <filter>`, exit 0:
- `-build`: 24 PASS (blkd, bootfsd, client, consoled, fsd, init, ipd, keyd, netd, rt, sshd, vendor; rv64+rv32).
- `formatting`, `no-cruft`, `unsafe-budget`, `size-budget`, `docs`, `rt-miri`: PASS.
- `host-tests` (every host-tests case: rt, client, blkd, r4, fsd, init, ipd, netd, net, sshd, model,
  steward, stride, wire, littlefs, host-tests): 16 PASS, 0 FAIL; also each relevant one alone: PASS.
- Bins linking redoubt_rt outside the build cases, `cargo build --release --target {riscv64,riscv32}imac-unknown-none-elf
  -p redoubt-net-tests -p redoubt-net-client -p redoubt-init-programs -p redoubt-init -p stub -p test-programs
  -p redoubt-consoled`: exit 0 on both. The only warnings are already on main (vendor/managed, kernel-half-attack).

Each intermediate commit (e7302b627, 624b0f640, 628db5fb8): `cargo testbench -- size-budget`,
`rt-host-tests`, `init-host-tests`, `init-build`, `rt-build`: all exit 0.

unsafe: `unsafe-budget` passes; libs/sys's count unchanged (the same one `asm!` block, moved into the
impl); no `unsafe` added anywhere.

## Page lines, as written

- `libs/sys/src/lib.rs` module doc, after "...the kernel's business.": "How a call travels is a
  [`Transport`]: a call in, its [`Return`] out, records and buffers by address. On the machine it is
  [`Ecall`], the registers below; a host's fake kernel, or any other backend, implements the same trait
  and reads and writes the same records." "# Registers" now opens: "On the machine a call travels by
  [`Ecall`]: an `ecall` with `a0` = ..." (rest of the sentence as before; rustfmt rewrapped).
- `docs/kernel/abi.md`: "On the process's side a call travels through a transport
  (`redoubt_sys::Transport`): on the machine `Ecall`, the `ecall` itself (the crate's only `unsafe`),
  whose result `redoubt_sys::decode_result` reads; the runtime takes whichever transport is installed,
  the `ecall` by default." Rest of the paragraph unchanged, rewrapped.
- `docs/userland/native.md`: "Every system call goes through one function, to a `Transport`: on the
  machine the `ecall`, on the host the fake kernel a test installs, and any other backend the same way,
  so the runtime and programs built on it (the echo client and server) run in host tests."
- `docs/beyond/README.md`, after the table: "**A direction, the owner's:** the system-call ABI is kept
  free of the MMU (a transport trait, lends and transfers stated as ownership), so that a backend
  without one, cooperative and in one process, could implement it. Nothing builds that backend."

## Questions and risks

1. **Size budget raised, both crates (your call).** The brief says the budgets pass; main's ceilings
   sat exactly at main's counts, so the design's lines cannot fit: libs/sys +7 (the trait 3, `Ecall`'s
   struct/impl 4), libs/rt +7 (the `Transport` re-export 1, `device_info` 6; the seam itself nets 0
   after trimming). Each raise is in the commit that needs it, with its `Size budget:` line. If raises
   are not wanted, say which lines to drop.
2. abi.md: after the new sentence, the unchanged "It refuses any result..." has the runtime, not
   `decode_result`, as its nearest antecedent. Kept as the brief settles it; an editor may want
   "`decode_result` refuses".
3. Paths beyond the owned list: `libs/rt/src/handle.rs` (the brief's "beside the runtime's device
   types"), `libs/rt/fake/Cargo.toml` (its description named `HostKernel`), `tests/size-budget.toml`.
4. Read in full: every diff hunk I commit; not every whole committed file, per the brief's context
   rules (fake/src/lib.rs, init.rs, handle.rs are large). Say if the whole-file rule wins.
5. Stable `cargo fmt` (in-dev's default) reformats the whole workspace; I reverted it at once and use
   `rustfmt +nightly` per CONTRIBUTING. No stray change was committed (`formatting` passes).

## Not run

No QEMU case and no whole bench (yours).

## Next

Your whole bench on both widths, alone, at 3314623c8.

# Review fold (abi1-implementer-2)

Tip **d60dfc3ea**, five commits on 75245a114 (was four, tip 3314623c8). Range-diff against the old
branch: `.wash/local/ABI1-fold-range-diff.txt`; `git diff 3314623c8 d60dfc3ea`: 18 files, +117 -86.

1. 3b78169b4 `tests: bootfsd's outcomes lend pages no owner holds` (new): the scripted lend was the
   page of a Buffer the test still held; `given_up` now unmaps the Buffer first (the seam keeps the bytes).
2. fca753eda `sys:` + `pub unsafe trait Transport: Sync`, `# Safety` as the Architect gave it
   (`#[allow(unsafe_code)]` on it, since lib.rs denies unsafe_code); `unsafe impl Transport for Ecall`
   with the Architect's reason; module-doc and two code comments rewrapped to 100 columns.
   Size budget libs/sys 1016 -> 1017 (eight lines: the allow attribute). Unsafe budget redoubt-sys
   1 -> 3, **not +1**: the checker counts each `unsafe` word, so the trait and the impl are one each.
   Budget name kept (a rename reads as a drop); a comment names the new sites.
3. 020f33e22 `rt:` (subject now "...calls a Transport, the ecall on the machine"): installed-transport
   feature, compile_error!, sys.rs bullet gone; cfg is target_os alone. libs/rt ceiling 2954 (falls
   from main's 2955), so no Size budget line. `install_transport -> bool`; transport.rs asserts its
   install took. abi.md: "`decode_result` refuses...", "only `unsafe` code". Fake and Counting:
   `unsafe impl` with reasons. scripted: `pub unsafe fn script(&self, Received)` with `# Safety`;
   safe `request(words)` (no lend); `request` field private; callers: bootfsd x2, mapping_views x1,
   refusals x1 = **4 unsafe blocks in tests** (keyd uses the safe form). rt SAFETY guarantor wording
   in ipc.rs, handle.rs, heap.rs x2. No new unsafe in libs/rt/src.
4. 84032423e `init:` + the native.md sentence; libs/rt 2954 -> 2960 (`Size budget: libs/rt: device_info, six lines`).
5. d60dfc3ea `docs:` unchanged.

## Commands (all via in-dev, from the worktree)

At 0ad1180d7 (code identical to d60dfc3ea; only unsafe-budget.toml differs): `cargo testbench --`
`-build` 0, `formatting` 0, `no-cruft` 0, `size-budget` 0, `docs` 0, `rt-miri` 0, `host-tests` 0
(16 PASS); `cargo build --release --target {riscv64,riscv32}imac-unknown-none-elf -p redoubt-net-tests
-p redoubt-net-client -p redoubt-init-programs -p redoubt-init -p stub -p test-programs -p redoubt-consoled`:
0 on both (the warnings already on main). At d60dfc3ea: unsafe-budget, formatting, no-cruft,
size-budget all 0. Each of the five commits: size-budget, unsafe-budget, rt-host-tests, init-host-tests,
init-build, rt-build, `cargo test -p redoubt-bootfsd -p redoubt-keyd`: all 0.

## Notes

- abi.md "the runtime takes whichever transport is installed, the `ecall` by default": with the
  feature gone, nothing can be installed on the machine; left as ruled, flagged.
- sys.rs `syscall`'s panic doc back to "a bug in the test" (host only again).
- No QEMU, no whole bench.
