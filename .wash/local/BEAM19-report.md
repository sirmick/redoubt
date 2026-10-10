# BEAM19: where the time goes (measurement, 2026-10-09)

Base a72e774b8 (wp-BEAM19 fresh from main; no code changed yet). rv64, `launch --system --smp 1`,
QEMU 10.2.1 with `-icount shift=3,sleep=on -rtc clock=vm` added by a PATH wrapper (so 1 guest
instruction = 8 ns of guest time), alice's console session driven over the serial console.
Tools and raw data: /home/mcloonan/redoubt/.tmp/BEAM19/ (tools/, r5 ... r10/).

## Method

1. Guest PC samples through QEMU's gdb stub (one attach per sample), symbolised with an
   unstripped beamlet from the same build (`CARGO_PROFILE_RELEASE_STRIP=false`, which only
   touches userland/otp's profile).
2. Exact instruction profiles: QEMU's `log exec,nochain` switched on and off through the monitor,
   each executed translation block weighted by its length from the disassembly; kernel blocks
   symbolised with the run's kernel ELF.
3. In-guest: `:timer.tc` of `Enum.reduce(1..N, 0, &+/2)`, per-chunk times (1,024 elements per
   chunk), and per-process reductions and stacks (`Process.info`) while a line runs.

Real-time PC samples are misleading under icount (they overweight QEMU-slow instructions and
idle); the TB-weighted counts are the numbers below.

## Results

- B46 reproduced: `Enum.reduce(1..262144, 0, &+/2)` typed as one long line, 21.9 s; typed as a
  short line, 13.6 s.
- **The hart never idles during a loop.** A whole timed run traced end to end: 4.57 s of executed
  instructions for ~4.6 s of guest time; beamlet's text 78 %, kernel 22 %, other processes 0.5 %.
  No ecall storm, no clock reads, no hub spinning, no idle.
- **Half of it is the shell's driver, not the loop.** Per-chunk times are bimodal: 48.1 ms per
  1,024 elements when the loop runs alone, ~100 ms while the session's driver process (<0.2.17>)
  runs beside it, round-robin in the same VM. The driver is busy for seconds to tens of seconds
  after every typed line: always in `Redoubt.Shell.Driver.draw/2 -> Redoubt.Term.request/2 ->
  Redoubt.Term.insert/3 -> cursor/1 -> advance/3 -> width/1 -> Width.columns/1 ->
  String.graphemes/1, :unicode_util`, with 2-4 messages queued. `insert/3` at the line's end
  calls `cursor(term)`, which re-measures every grapheme already typed (graphemes,
  `characters_to_list`, `is_wide`, `Text.visible`), so each piece of echoed input costs O(line)
  and a line costs O(n^2): ~100 reductions per grapheme per insert. 8.8 M driver reductions since
  boot by the end of the session. During `Process.sleep(3000)` after a ~500-character line the
  driver took 122,000 reductions; 30 s after an 85-character line it was quiet (0).
- **The loop alone: ~2,900 guest instructions per element** (~1,400 per reduction; ~7.8 BEAM
  instructions per element), ~23 us; 262,144 elements ~6 s. Of that, beamlet 89 %, kernel 11 %.
  Host beamlet: 70-200 ns per element (tail loop / foldl).
  - `interp::call_native`: `let mut args = [Term::Nil; 255]` initialises 255 terms (765 guest
    instructions, a scalar loop on rv64imac) on every native call, for 2 arguments: 16 % of the
    loop's instructions by itself.
  - `interp::step` + `run` + operand access (`get`, `dst`): ~55 %: the per-instruction dispatch
    (a 3 KB frame, 12 saved registers per `step` call, decode through `InstrView`).
  - `Sched::resolve` (BTreeMap per external call), `as_fun`, `refresh`, allocator `alloc/dealloc`
    of the args `Vec` in `call_fun`: ~8 %.
  - kernel 11 % of the loop (22 % with the driver): `kframe::at`, `map_run`, `memset`, `memcpy`,
    `MemoryManager::budget/store`. First read as the GC's fresh heap mapped and zeroed per
    collection; the later trace (below, "Where the rest goes") shows most of it is the ~12,000
    instructions the kernel spends on each interrupt, the GC's share under ~3 %.
- **memcmp** (59-85 % of real-time samples in the first runs) was the driver and erl_eval
  comparing binaries: `Heap::eq_exact` on two bitstrings memcmps min(len) bytes even when the
  lengths differ; 2.6 % of instructions in the traced run. Equality can check the lengths first.

## Hand-off: the driver's echo (to B46, per the orchestrator)

Kept as the record: the shell driver's `Term.insert/3 -> cursor/1` re-measures the whole line for
each piece of echoed input (O(n^2) per line). Measured on main a72e774b8, rv64 icount: during a
line's evaluation the loop's 1,024-element chunks take ~100 ms against 48 ms alone (the driver
takes half the hart); 122,000 driver reductions in 3 s after a ~500-character line; a 50-character
line costs the loop ~1.9 s of a 4.8 s run; 8.8 M driver reductions in a short session. B46's
`wp-B46` keeps the cursor (its "Typing cost" test).

# BEAM19: the fix (completion)

Base a72e774b8 (the assigned base; main has moved on, none of its later commits touch these
files). Head 80642e4fd on wp-BEAM19:

- f1abcf10d beamlet: bitstrings of different lengths are unequal without comparing their bytes
  (`vm/src/term/cmp.rs`; differential test `tests/erlang/compare.erl` extended)
- da9f0ad66 beamlet: a native call fills only its arguments, a fun's stay in the registers, and
  an instruction saves no registers (`vm/src/interp.rs`)
- 80642e4fd tests, docs: a floor on how fast beamlet runs compiled code on the machine
  (`tests/beamlet-reduction-rate.toml`, `docs/userland/beamlet.md`)

## Numbers (bench:beamlet-reduction-rate, icount shift=3, seed 1, fastest of 30 runs)

`Enum.reduce(1..65536, 0, &max/2)`, 98,723 reductions:

| | rv64 | rv32 |
| --- | ---: | ---: |
| before (a72e774b8's interp.rs/cmp.rs), reductions/s | 34,041 | 31,215 |
| after, reductions/s | 52,647 (52,647 again in the gate run) | 47,052 (47,064 in the gate run) |
| guest instructions / loop step, before -> after | 5,531 -> 3,577 | 6,033 -> 4,002 |
| guest instructions / reduction, after | 2,374 | 2,657 |

Floor 42,000 (slower width less a tenth, rounded down); the before-tree fails it on both widths
(ran it: both "timed out waiting" at 34,041 / 31,215; the case now also forbids a low rate so it
fails at once). Per 1,024-element chunk of `&+/2` on rv64: 48.1 ms -> 37.8 ms (call_native and
call_fun) -> 31.6 ms (step inlined).

Where the rest goes (rv64, TB-exact, after): beamlet ~80 %, ~207 guest instructions per BEAM
instruction (dispatch now ~50); kernel ~20 %: ~12,000 instructions per interrupt, 1,223 of 1,498
traps in 0.71 s were interrupts, 275 were system calls (`kframe::at`, `MemoryManager::budget`,
`store`, `map_run`, page walks). Kernel follow-up suggested (not changed here: kernel red's
hotspot).

Not changed, with the evidence:
- GC fresh heap map/zero per collection: in the loop's traced window the GC's kernel work
  (`map_run`, page walks, zeroing) is under ~3 % of instructions, and the runtime allocator maps
  and unmaps every block over 2 KiB by design (libs/rt, every consumer). Keeping a spare to-space
  would hold a second heap per process and move beamlet-footprint; not worth it at 3 %.
- `Sched::resolve` (a BTreeMap per external call): 2.5 %.

Yields: none removed or changed; a process still yields after TIME_SLICE (2,000) reductions or
MAX_INSTRUCTIONS_PER_SLICE (200,000) instructions; the kernel's slice end still preempts the VM's
thread (said on beamlet.md).

B46's cases: `shell-long-output` passed in 48.9 s (rv64) and 37.6 s (rv32) on this branch,
against 167 / 186 s B46 measured at B45's main (host clock, SSH; not rerun on a72e774b8 here).
`shell-output-rate` is B46's branch's case, not on this base: not run.

## Gates (all exit 0)

- `q run --cores 8 -- cargo test` in userland/otp: rc 0, every suite ok.
- `tools/difftest` (q, 8 cores): 527/527 passed, 21 skipped by design, rc 0.
- `./test-shell`: rc 0, every stage passed.
- `make -f scripts/jobs.mk prebuilt` rc 0, then `make -k -f scripts/jobs.mk set CASES="beamlet-reduction-rate
  userland-boot beamlet-footprint docs size-budget formatting shell-long-output"` rc 0: every
  case PASS on rv64 and rv32 (beamlet-reduction-rate 52.6 / 50.0 s, userland-boot 12.4 / 10.5 s,
  beamlet-footprint 6.5 / 7.6 s, shell-long-output 48.9 / 37.6 s), docs, size-budget, formatting.
- Not run: the whole bench (train's), rv32 kernel builds beyond the cases' own.

## Summaries checked

- docs/userland/beamlet.md: "beamlet on Redoubt" status line names the case; a "How fast compiled
  code runs" paragraph and table; the "Why" line "slower (up to about five times, measured on the
  host)" is the host's and stays; "CPU" limits bullet unchanged and true.
- README.md, GETTING-STARTED.md, docs/plan/m2-usable-shell.md, userland/otp/README.md and
  DESIGN.md: no speed claims about beamlet on the machine; no change.
- docs/testbench.md "Which cases run in guest time" counts ("145 of the 226") are already stale
  on main (256 boot cases now); the new case follows boot-profile's precedent (a disk, `[[input]]`
  under icount with sleep on, short lines); not updated here (docs hotspot; counts drifted before).
- docs/userland/shell.md: nothing about the VM's speed on this base (B46's branch adds a note that
  its floors are re-set when beamlet's per-step cost falls: B46/orchestrator to reconcile).
