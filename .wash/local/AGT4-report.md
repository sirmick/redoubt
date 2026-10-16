# AGT4 report

Base: main 3a80e5af0; branch wp-AGT4 (worktree .worktrees/AGT4). Head: c90196606 (one commit on main 0b487325b, train 36; review fold P1-P3 in it: budget_pages dropped, the session cap restated at 11,220, AGT2's shell.md residual replaced). Logs .tmp/AGT4/train36/.
Gates (all on the committed tree, from `make -f scripts/jobs.mk prebuilt`, 302 rv64 + 288 rv32
pieces, 0 failed; logs .tmp/AGT4/final/ and alone/): beamlet's set from `scripts/shell-cases` (60
cases) + size-budget, both widths: 119 PASS 0 FAIL (steward-ssh-two-principals, beamlet-footprint,
beamlet-compile-footprint, beamlet-compile-flood among them); shell-compile-module x5 both widths:
10 PASS; docs PASS; host: beamlet-vm 47+18+17+3+... all pass (zlib_codec 3), testbench memory 14,
redoubt-vendor-check vendored 8/8 (+4 +8 others); rustfmt +nightly --check on every changed .rs;
git diff --check clean. No unsafe added (0 in the patch). Not run: the whole bench (the train's).

## The frame

- rv64, AGT2's run (stripped ELF): `Store page fault of 0x7ffecae8 at 0x002e2b70`. Rebuilt
  unstripped from the same tree (.tmp/AGT4/sym64; identical .text address 0xdc478 and size
  0x2316c0): `__rust_alloc_zeroed` (libs/rt/src/lib.rs:78), the first store of an allocation call
  made with the stack already below its lowest mapped page. This tree's run of the new case: the
  same address, PC 0x2e2b5a = `redoubt_rt::heap::Heap::realloc`.
- rv32 (new case, this tree): `Store page fault of 0x7ffedc9c at 0x003bf57a`, 74,596 bytes below
  the top (18.2 pages). Unstripped rebuild (.tmp/AGT4/sym32; .text 0x246 bytes shorter, so
  approximate): `RawVecInner::finish_grow` → allocate, a vector growing.
- The caller, from the host (gdb on a symbolised release host beamlet, the inferior's stack
  capped at 72 KiB through gdb's exec-wrapper; .tmp/AGT4/gdb-sym.txt):
  `miniz_oxide::deflate::compress_to_vec_inner` ← `compress_to_vec_zlib` ←
  `beamlet_vm::etf::encode_compressed` ← `bif::erlang::term_to_binary` ← `interp::run_native` ←
  `call_native` ← `call_mfa_with` ← `interp::run` ← `vm::schedule` ← `drive` ← `Vm::run` ← main.
  The frames above the compressor's total 7,520 bytes on x86-64. Nothing recurses: miniz_oxide
  0.9.1 keeps the whole `CompressorOxide` as a local of `compress_to_vec_inner`, and
  `size_of::<CompressorOxide>()` is 65,712 bytes (`lz.codes: [u8; 65536]` inline on every target
  but wasm32; the Huffman tables, the hash buffers and the output buffer are boxed). The Elixir
  compiler calls `term_to_binary(_, [compressed])` for a chunk.
- The same shape in `bif/zlib.rs:244`, `deflateInit`: `Box::new(CompressorOxide::new(flags))`
  builds the struct on the stack and moves it into the box. On the host `:zlib.deflateInit` at
  the prompt overflows 72 KiB where the plain prompt needs 48–64; on the machine (lever (c) round,
  below) rv32 faulted at `0x7ffedf0c` (73,972 bytes down, 244 past the 18 pages), PC 0x377264 in
  `deflate_init`, on 5 of 6 runs, rv64 never.

## The need, measured

Host (x86-64 release, `ulimit -s`): plain prompt 48–64 KiB; the defmodule 96–104 KiB.

Machine (`beamlet-footprint`'s manifest with the brief's defmodule typed after the breakdown and
the module called, one scheduler so the compile runs on the painted first thread, beside a whole
bench; stack and heap from the bench's RAM scan):

| | rv64 | rv32 |
| --- | ---: | ---: |
| stack peak, compressor as a stack local (64-page stack; round2) | 88,888 B | 81,056 B |
| stack peak, compressor on the heap, lever (c) (round3/5) | 34,488 B | 25,832 B |
| stack peak, the vendor patch, alone (alone/; quoted on the pages) | 34,280 B | 25,816 B |
| heap peak after the compile, alone (cap 11,092) | 9,122 pages | 8,783 pages |
| idle prompt, for comparison (`beamlet-footprint`, testbench.md) | 36,136 B / 5,492 pages | 5,308 pages |

With the compressor off the stack the compile's first-thread peak is under the idle prompt's, so
both constants stay at 18 pages (twice 36,136 B rounded up). The heap margin under the cap is
1,970 pages on rv64 and 2,309 on rv32; an eighth of the cap is 1,386.

Observation, not changed: `beamlet-footprint` alone on this tree reads rv64 stack 34,456 B and
heap 5,523 pages (rv32 25,912 / 5,343) where the pages quote 36,136 / 5,492 (5,308): the idle
heap peak has drifted up by 31 pages since those were taken, within the 5,546 the page says it may
reach before the share moves. Not this package's case or change (its manifest is untouched and
the patch allocates nothing at an idle prompt); the pages' idle numbers are for a re-measurement.

With two schedulers (round1) the compile ran on the second scheduler: rv32 completed (smaller
frames: 18 pages just hold it), rv64 died later with `Illegal instruction 0x00000000 at 0x00284de4`
(`Arc<Chunk>::clone` in a `Vec::clone`): a helper thread's stack has no guard page
(libs/rt/src/thread.rs:25), so its overflow writes over the heap below it. In the steward's session
the compile ran on the first thread both times (the page fault). **Finding for a node of its own
(the Architect's ruling says so too):** a guard page per spawned thread, with a case that overflows
a helper on purpose and expects the fault, not a later crash. native.md's `thread` row now states
the absence.

The flood after a compile (`beamlet-compile-flood`, round6 and the final set): one flooding
process after the compile reaches the budget's backstop, `memory allocation of 1236768 bytes
failed` (rv32: 2,473,536), the VM panics (exit 101) and `init` restarts it on a new console. The
residual is on beamlet.md "Limits inside one VM" as the ruling words it; the fix (limits derived
from the pages not held by code) is a follow-up node, not AGT4's.

## The fix: lever (d), after (c) proved fragile

Lever (c) (`Box::<CompressorOxide>::default()` + `set_format_and_level` in etf.rs, and the same in
zlib.rs) took the frame out of `term_to_binary` on both widths (round3/5 scan: 34,488 / 25,832 B)
but not out of `deflateInit` on rv32: whether the box is built in place is the optimiser's choice
per call site. So, as the orchestrator's fallback: `vendor/patches/miniz_oxide.patch` boxes
`LZOxide.codes` under `with-alloc` as the crate already does on wasm32 (the field, both `new`s and
the `vec` import's cfg: 4 hunks), recorded in vendor/README.md "miniz_oxide" and on testbench.md
"Patched crates". `CompressorOxide` is then a few hundred bytes wherever it is built, so etf.rs
and zlib.rs are main's text again. vendor-check: the patch reverses to the published bytes
(`vendored_files_are_the_published_bytes`), applies cleanly to a pristine copy (checked by hand
too), and `every_patch_is_a_vendored_crates_and_recorded` now accepts a crate of beamlet's
table (`all()`), where miniz_oxide is. zlib.rs's `DEFLATE_BOXED` gains the 64 KiB code buffer
(the stream-limit accounting; `a_deflaters_boxed_state_is_the_constant` measures it) and
`a_deflaters_inline_state_is_a_few_hundred_bytes` fails on an unpatched crate.

No constant moves: `SESSION_STACK_PAGES` and `SCHEDULER_STACK_PAGES` stay 18; their comments
state the two peaks (steward.rs said 35,288 where the page said 36,136).

The bench: `heap_fits = true` on a boot case (tools/testbench case.rs, memory.rs; needs `memory`)
judges a capped heap by an eighth-of-cap margin instead of twice the peak, for a case that drives
a workload within a server another case sizes; `heap_fits_holds_an_eighth_of_the_cap_over_the_peak`.

## Cases

- `tests/shell-compile-module.toml`: alice over SSH types the defmodule a line at a time, calls
  it, runs a `:zlib` deflate, and the session still answers; tagged answers; both widths.
- `tests/beamlet-compile-footprint.toml` (+ `tests/data/beamlet-compile-footprint/manifest.json`:
  footprint's with `schedulers=1`): compile at the prompt over the console, `memory = true`,
  `heap_fits = true`; the scan states the stack and heap peaks.
- `tests/beamlet-compile-flood.toml`: the residual's attack case; verdict from `init`'s exit (101
  or a fault) and restart lines on distinct consoles; the shell's `{:flood, ...}` answer alone on
  a line is forbidden (it would mean the process was killed alone).
- `beamlet-footprint` is unchanged (reverted after the measurement variants).

## Summaries checked

- README.md:44 "the Elixir and Erlang compilers run on beamlet, on the host": a module defined at
  a session's prompt now compiles on the machine; clause added. GETTING-STARTED.md: no claim.
- docs/plan/m5-self-hosted.md:83 "Built: the compilers run on beamlet, on the host": the same
  clause added, pointing at `shell-compile-module`. docs/plan/m3-agents.md Progress: AGT4 builds no
  M3 behaviour (the compile is M5's claim); no change. docs/plan/m2-usable-shell.md: no claim on
  compiling; no change.
- docs/userland/development.md:47 (the compiler suites: host only): still true, the differential
  suites run on the host; the brief says it stays until AGT3; no change.
- docs/userland/shell.md: no claim that a defmodule fails or is absent; no change.
- docs/servers/steward.md:491, docs/kernel/budgets.md:220 (44 pages outside the heap, the share):
  unchanged numbers; no change. docs/servers/init.md "Stacks": states no constant; no change.
- docs/userland/native.md `thread` row: gained the no-guard-page fact (beamlet.md links to it).
- vendor/README.md: section added (the test requires it). docs/testbench.md "Patched crates":
  sentence added.
- userland/otp READMEs: none state the compressor; no change.

## Docs changed

docs/userland/beamlet.md ("Limits inside one VM" status +3 tests, the compile paragraph after the
share arithmetic, the scheduler-stack bullet, "beamlet on Redoubt" status +shell-compile-module);
docs/testbench.md (memory-budget status +2 tests, the `heap_fits` rule, the stack-peak sentence,
"Patched crates"); docs/userland/native.md; README.md; docs/plan/m5-self-hosted.md; vendor/README.md.

## Logs

.tmp/AGT4/round1 (two schedulers, stack 64: the helper-thread crash), round2 (one scheduler,
stack 64, compressor on the stack), round3 (lever (c), stack 64), round4 (flood inside the
footprint case), round5 (compile-footprint at 18 pages), round6 (compile-flood), set.log +
set-logs/ + x5/ (lever (c): the rv32 `deflateInit` fault), final/ (the vendor patch: prebuilt,
set, x5), gdb-sym.txt, sym64/, sym32/, sym32b/, symhost/.
