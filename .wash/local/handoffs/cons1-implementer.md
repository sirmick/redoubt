cons1-implementer handoff (after CONS1 and CONW1, both merged)

## State and traps first
- No open package. CONS1 merged as 3206c43b4; CONW1 merged as b2f1a1bb9. Nothing uncommitted of mine.
- Worktrees left: .worktrees/CONS1 (wp-CONS1, merged) and .worktrees/CONW1 (wp-CONW1, merged). The orchestrator may remove them; neither holds unmerged work. Scratch: .worktrees/CONW1/.tmp/CONW1 (logs, gate scripts).
- Reports: .wash/local/CONS1-report.md, .wash/local/CONW1-report.md, design .wash/local/CONW1-design.md.

## Traps learned (both packages)
- Export RUSTSBI_PROTOTYPER{,_RV32}, BEAMLET_TOOLCHAINS and PATH in the same shell as `make -f scripts/jobs.mk`. A fresh shell without them fails every boot case in 0.0 s ("RustSBI Prototyper not found").
- A prebuilt made before any edit is refused ("built from this tree before it changed"). Remake it, or delete target/prebuilt so cases fall back to cargo testbench.
- The model generator (model/src/gen.rs): never add RNG draws for a new call. Pick from a value already drawn, or from k.now's parity. Extra draws shift every seed, and mutation catches slow past model-mutations' 25 s per-job deadline (R2NoWaitCap went from 7 s to 38 s). Picking by handle parity made console_hold never succeed (the coverage test catches that). Saved as a memory.
- size-budget refuses a raised ceiling until it is committed with a `Size budget: <crate>: <reason>` line. Commit, then rerun.
- rustfmt per crate edition: model and libs/conhold are edition 2021 (`gen` is a keyword in 2024).
- The bench expect list has no repeat count: N identical lines need N patterns (console-hold-stuck lists 122).
- A question to the orchestrator is at most 2000 bytes.
- A whole bench is single-flight: check `scripts/q ls` before starting one; leave it to the train if another runs.
- "Read every committed file in full" was impractical for model/src/kernel.rs (4,200 lines). I reviewed the whole diff and said so in the report; the orchestrator accepted it.

## What CONW1 built (for anyone touching the console)
- console_hold (call 29, 0x11d) on the console's MMIO handle. The console is the Devs MMIO entry with flag bit 1 (the loader marks /chosen/stdout-path); a second marked entry stops the boot; with none, every hold is WrongObject.
- Kernel lines queue whole in a 4 KiB queue (libs/conhold) while held:
  - out at release; a full queue prints at once;
  - died() ends a holder's line before the queue;
  - system_reset ends the hold and flushes; panics never wait.
- debug/console.rs guards OUTPUT with the PRINTER hart word, so println! is safe outside KERNEL_LOCK (SMP4 relies on this for the fault report).
- Writers: consoled holds per 256-byte chunk (HOLD_CHUNK); init per line; tests/programs console.rs per line (Console from a line's first piece to its newline, LINE_HELD).
- Cases: console-one-writer (TCG, 2 harts; fails without the hold), console-hold-stuck (attack). sched-latency-tcg now runs at smp=[2].

## What CONS1 built
- Typed parking in rt; consol size/resize on consoled and sshd; beamlet's resize thread; the shell's re-layout.
- consoled answers size only when its manifest names size= (option B). sshd refuses consol on an ended channel.

## Open items noticed, not mine
- docs/testbench.md "145 of the 226" icount counts were stale on main before CONW1 (151 of 238 then).
- beamlet-redoubt's console test input_nobody_reads_holds_no_idle_and_waits_for_the_next_reader failed once under heavy load and passes alone.
