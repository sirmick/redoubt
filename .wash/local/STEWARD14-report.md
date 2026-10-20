# STEWARD14 report (steward14-implementer)

Branch `wp-STEWARD14`, worktree `/home/mcloonan/redoubt/.worktrees/STEWARD14`, two commits on
origin/main dc75a30eb (train 85 published: STEWARD13's launch thread in):
- `6ffd7503a` steward: the gathered burst, `Steward::exits`/`events`/`decided`, the probe's
  per-lease sends and burst line, the order host test; `Size budget: servers/steward: 4039 ->
  4121`; the reviewer's folds: `Pending::owner` (dead since the sponsored carve fix on main) deleted, the
  burst-order host test named on steward.md's Failure-and-restart status line, beamlet.md's forwards-queue figures
  dated to when the queue landed with a pointer to steward.md.
- `d8ef4443a` testbench, steward, scheduling: the -16 case's bound 6 s -> 3 s, both cases'
  text, steward.md (Leases, Failure and restart, Residual risks), scheduling.md ("With the real
  steward"), steward-latency.toml's comment.
Tier A (the steward's loop), reviewer steward-red-5. Investigation-first by the node: measured
before and after, landed because the derived bound shrank by three seconds at sixteen.

## What was found and built

The brief's premise held: with beamlet's forwards queue in (WAKE1) the serial cost of a burst of
exits was the steward's own work per exit between the sends (its decision, end batch, record,
console lines). The notices-first shape (STEWARD12's 57bff2058, re-derived against STEWARD13's
loop) is: the loop takes every exit report already waiting with the first (`own.receive(0, 0)`
until empty, other events parked in a `pending` deque for the next turn), the drive decides each
exit in order and sends its `lease_ended` as it is decided (`decided` retains every output but
the notices, which go at once), then runs each exit's effects in order with the events that
follow from them (`events`: its batches' `Done`s, a channel's `Pending`), the core seeing the
rows it saw one exit at a time. STEWARD13's rules kept: a head of an object whose batch waits at
its launch is held and its caller parked exactly as `event` did (`holds` per head); the reply
tokens' `awaiting.remove`; `run_from`/`suspend` for batches. A second process of an object already
in the burst gets no event. Host test `a_bursts_notices_all_go_before_any_exits_bookkeeping`:
three notices, each once and in order, all before any `Release`/`Hand`/`Destroy`; every
`LeaseEnded` record; one exit through `exits` makes the calls `event` makes.

Probe (latency-probe only): `notified` is a map by lease; a burst's samples are grouped by each
exit's place in the burst (D down to 1, `others - exits gathered`, the group a serial dequeue gave,
so the cases' N=k expects hold); a line `steward: burst of N exits: notices X us, effects Y us,
lines Z us` once per burst of more than one exit (the three phases' cost on the loop).

## Measurement (seed 3; `.tmp/STEWARD14/baseline/logs`, `.tmp/STEWARD14/after/logs`; `burst.py` reads them)

Before = origin/main 74a83e2f9 (WAKE1 in); after = the shape on STEWARD13's head (steward content
identical to the landed commits). Per lease: gross = `lease_ended` send returned - that lease's
deadline; net = the oracle's, less the checked build's audits. "Worst" = the worst lease of the boot.

| boot | before: first / worst gross (net) | before gaps ms | after: first / worst gross (net) | after gaps ms | lines |
| --- | --- | --- | --- | --- | --- |
| D=6 rv64 1 hart | 0.553 / 1.133 s (0.176) | 40-93 | 0.578 / 1.075 (0.159) | 22-63 | 6/6 both |
| D=6 rv64 2 harts | 0.790 / 1.284 (0.336); a 2nd run 0.446 / 1.431 (0.370) | 94-137; 91-120 | 0.742 / 0.914 (0.125) | 18-31 | 6/6 both |
| D=6 rv32 1 hart | 0.633 / 1.244 (0.175) | 51-98 | 0.655 / 1.121 (0.140) | 15-52 | 6/6 both |
| D=16 rv64 1 hart | 0.830 / 1.786 (0.452) | 38-95 | 0.920 / 1.416 (0.328) | 13-77 | 16/16 both |
| D=16 rv64 2 harts | 0.840 / 2.355 (0.917) | 76-145 | 0.962 / 1.447 (0.414) | 15-66 | 16/16 both |

First send to last send: before 273 / 583 / 311 ms (six), 943 / 1,606 (sixteen); after 209 / 128
/ 165, 481 / 502. The burst line (after): notices phase (first report taken -> last send) 248 /
427 / 206 ms at six, 591 / 642 at sixteen; effects 17 / 124 / 26 ms, 35 / 297; the console lines'
turn at the loop's next top 170 / 310 / 182 ms, 403 / 777 ms. The first notice of sixteen is
~0.1 s later than before (the take and inspection of sixteen reports before the first decision);
at six within noise. No notice lost in ten boots.

**Decision by the house rule** (twice the worst lease rounded up to a second): six 1.43 -> 3 s
before, 1.12 -> 3 s after (unchanged); sixteen 2.36 (2.51 in WAKE1's run) -> 6 s before, 1.45 ->
3 s after: a whole second or more, so the shape lands with the -16 toml at 3 s (digits
`[0-9]{1,6}|[12][0-9]{6}`). Gate figures below are a second sample at the same seed.

## Pages (the documentation check)

- `docs/servers/steward.md`: Leases bullet (every notice ahead of any exit's bookkeeping; late by
  the core's decisions before it); Failure-and-restart burst bullet rewritten (the shape, the held
  exits, the figures before and after, the bookkeeping that follows the sends, the history: built,
  withdrawn under the forward's gate, kept now; the shell's print figures attributed to WAKE1's
  measurement with the sends 37-131 ms apart); Residual-risks bullet (the decision per exit as the
  pace, the bookkeeping after the sends as what a call or event among the burst waits behind, the
  sixteen-report take before the first notice).
- `docs/kernel/scheduling.md` "With the real steward": the burst passage with the same content.
- `tests/steward-latency.toml` "Why one at a time": figures.
- Checked, unchanged: `docs/SECURITY.md` (no rule's tests change; the burst cases stay under
  the Responsiveness status line, same names), `docs/testbench.md:810` (`deadline_notice_p99_us`
  is the latency cases' bound), `docs/userland/agents.md` "Leases" and `GETTING-STARTED.md:134`
  (the notice exists and is printed, no latency claim), `docs/userland/beamlet.md` "The forwards
  queue" (states the steward's per-exit work as the pace then measured: it describes the queue's
  own measurement and names the steward page for the steward's side; left as is — a reviewer may
  want its "37 to 131 ms" clause pointed at the new figures), README.md, `docs/plan/m3-agents.md`
  (no claim on notice latency). No dates, no package ids on pages or in case files.

## Gates

Every command through `scripts/jobs.mk` under q; logs `.tmp/STEWARD14/gates/` (`make.log`, `logs/`,
`release-checks.log`, `host-gates-*.log`).

- On the final code (7a943a98f, the same steward content as the head's d8ef4443a: the recreation
  changed only the pages', tomls' and messages' figures, `git diff` of the code files empty):
  `make -k -j8 set CASES="formatting docs size-budget unsafe-budget <41 steward-*> steward-deadline-burst
  steward-deadline-burst-16 steward-latency steward-latency-n4 steward-latency-n16"`: **set: 50
  cases, 95 targets rc=0, make rc=0, 98 PASS lines, no FAIL** — the 41 steward cases
  (`steward-set-cases.txt`: every `tests/steward-*.toml` but the latency and burst cases,
  `steward-host-tests` and `steward-model-host-tests` and `steward-launch-overlap` among them)
  both widths; the burst cases at their gated configurations (rv64 one and two harts, rv32 one;
  -16 rv64 one and two) under the 3 s bounds with every line; the three latency cases as gated
  (steward-latency 547.7 / 623.4 / 694.8 s, n4 343.7 / 430.6 / 388.8 s, n16 196.0 / 246.2 s);
  formatting, docs, size-budget, unsafe-budget rc=0.
- Second burst sample from that run (the oracle's lines; the rv64 console logs were pruned from
  the run dirs before the copy, the rv32 one kept): six worst gross 0.991 / 0.926 / 1.100 s (rv64 1,
  rv64 2, rv32 1), worst net 0.127 / 0.128 / 0.129 s, first as early as 0.432 s at two harts;
  sixteen worst 1.455 / 1.463 s, net 0.343 / 0.427 s; rv32 gaps 14-50 ms, its burst line notices
  187 ms / effects 21 / lines 199. The pages and tomls carry the widened figures (first 0.43 s,
  sixteen 1.42 to 1.47 s, 0.33 to 0.43 s net); 2 x 1.463 rounds up to 3 s.
- On the head's text (d8ef4443a): `set CASES="docs formatting size-budget"` rc=0 each
  (`host-gates-final.log`).
- Host: `cargo test -q -p redoubt-steward-server`: 77 passed (the burst-order test among them);
  `cargo +nightly fmt -p redoubt-steward-server -- --check` clean; `cargo check --release` for
  riscv64gc (with `latency-probe`) and riscv32imac: rc=0 (`release-checks.log`; the probe form on
  rv32 checked earlier on the same code).
- Not run: the whole bench (the train's).

Reading: the two burst tomls, steward-latency.toml's comment region and size-budget.toml's entry
in full; steward.rs, drive.rs and tests/steward.rs by the whole diff of each commit and the
regions it touches (the EXITS arm, the loop's top, `notify`, the Probe; `event`/`exits`/`events`/
`decided`); steward.md and scheduling.md by their changed sections and the surrounding bullets,
not the whole pages.

## Findings beyond the node

1. The burst's console lines at the loop's next turn cost 0.17 to 0.78 s (sixteen at two harts)
   after the sends: a call or event arriving among the burst waits behind them; stated as the
   residual. If it matters, the lines could be said across turns or the per-exit lines dropped
   under the probe.
2. The first notice of sixteen is ~0.1 s later than with the serial loop (the take of sixteen
   reports and their inspection before the first decision): stated; the per-lease worst still fell
   by 0.4 to 0.9 s.
3. `Pending::owner` in drive.rs is dead code on 5fbcf52bf itself (STEWARD13's `carves_from` fix
   dropped its use): a `dead_code` warning the published tree carries; not touched here.
4. The first baseline boot (rv64 D=6, 1 hart) lost its session to `File.mkdir_p` ->
   `{:error, :etimedout}` on the data volume at the staging step, beside train 83's bench and
   STEWARD13's latency cases (the "walfsd timed steps under load" condition); the case then idles
   to its 5400 s timeout (STEWARD12's finding 5). Killed and rerun clean.
