# STEWARD12 report (steward12-implementer)

Branch `wp-STEWARD12`, worktree `/home/mcloonan/redoubt/.worktrees/STEWARD12`, two commits on
origin/main `4f381d216` (train 78 published): `8c939b233` steward + testbench (the probe's sample
point, the two cases, the size ceiling), `ccc52b075` docs; head `ccc52b075`. Tier A (the steward's loop; a
responsiveness quantity under a burst), reviewer steward-red-5. Rulings: Architect on thread
STEWARD8-bound (two answers), the orchestrator's call to revert notices-first.

## What the node found, and what it delivers

The brief's rule was: on a burst of `Exited` events send the `lease_ended` notices first, then
the per-exit bookkeeping, so the last notice is ~N × one send rather than ~N × 70 ms. It was
built (the loop gathering the queued exit reports with a zero-wait receive, the drive deciding
every exit and sending its notice as decided, then each exit's effects in order; a host test
that every notice precedes any exit's bookkeeping; commit 57bff2058, kept in the reflog) and
measured on two new cases before and after. **It reorders the burst but does not cut it**: the
~70 ms per exit STEWARD9 saw is the `lease_ended` **send**, a rendezvous with the sponsor's shell
VM, which takes the next notice only once it has printed the previous `[lease NAME ended]`
through its relay; the exits' bookkeeping on the loop is 16–35 ms for the whole burst. The
Architect ruled (1): the burst cases record the quantity with no bound, the residual is stated,
the real fix is the shell's listener taking a notice before it prints the one before (a Tier B
shell node the Architect is drafting); the orchestrator ruled notices-first reverted (16 ms of a
581 ms cost). So the tree carries:

- `servers/steward/src/bin/steward.rs` (`latency-probe` only): `Machine::notified`, the last
  `lease_ended` send's lease and return time; the loop's `deadline_notice` sample is that
  return less the deadline (the sponsor told), where it was the dequeue before, which
  under-counted the send. Nothing outside the probe changes; `drive.rs` is untouched.
- `tests/steward-deadline-burst.toml` (D = 6, both widths, `smp = { rv64 = [1, 2], rv32 = [1] }`),
  `tests/steward-deadline-burst-16.toml` (D = 16, rv64, `smp = [1, 2]`): alice's session starts D
  spinners, each asked for a shared deadline less the time since the first start (guest clock),
  so the kernel takes them at one entry; a sleep of the deadline + 3 s, the round marker, the
  done marker. `post_check = "sched_oracle gate_harts=2 r10_p99_us=30000"`: `deadline_notice`
  recorded, not bounded; one `LATENCY-COUNT N=k deadline_notice 1` per exit (the groups are the
  leases live at each dequeue, D down to 1). Deadline 100 s / 120 s = twice the worst serial
  launch total. The image's realistic burst (alice's unlabelled half-share of 40,959 pages: 3
  leases beside two sessions, 5 beside one) is in the description; six is above it.
- `tools/testbench/src/case.rs`: the probe's five builders. `tests/size-budget.toml`:
  servers/steward 3553 → 3580 (`Size budget:` line in the commit).
- Pages: `docs/servers/steward.md` "Failure and restart" (a new bullet: the burst's cost, the
  send as a rendezvous, the lost notice, notices-first built and not kept, the shell's side as
  the fix, the N destructions before the first notice; the status line names the two cases; the
  probe paragraph: five builders, the sample at the send), "Residual risks" (the burst told at
  the sponsor's rate), "Leases" (one clause); `docs/kernel/scheduling.md` "With the real
  steward" (`deadline_notice` defined at the send, the burst's serial-dequeue residual replaced
  by the measurement; the Responsiveness status 23 → 25 with the two cases);
  `tests/steward-latency.toml`'s "why one at a time" comment.

## The measurement (seed 3; logs `.worktrees/STEWARD12/.tmp/STEWARD12/{before,after,final}/`)

Before = base steward (sampled at the dequeue), after = notices-first (sampled at the send);
gross µs from the deadline; net = the oracle's, less the checked build's audits.

| boot | before first / last | after first / last | after net p50 / p99 | after: notices / effects / lines on the loop |
| --- | --- | --- | --- | --- |
| D=6 rv64 1 hart | 523,564 / 1,431,975 | 595,093 / 1,424,085 | 166,349 / 303,079 | 580,852 / 15,543 / 173,251 |
| D=6 rv64 2 harts | (login ssh 255, not run) | 488,810 / 1,450,583 | 174,233 / 387,545 | 607,358 / 124,471 / 318,234 |
| D=6 rv32 1 hart | 625,019 / 1,580,232 | 640,248 / 1,543,325 | 158,862 / 297,286 | 619,449 / 28,358 / 191,933 |
| D=16 rv64 1 hart | 803,113 / 2,120,585 | 673,343 / 1,842,096 | 267,753 / 441,054 | 980,657 / 35,427 / 394,477 |
| D=16 rv64 2 harts | not run | 971,861 / 1,924,564 | 411,264 / 618,467 | 1,099,539 / 320,016 / 792,394 |

Per send (rv64 1 hart, D = 6, after): the first exit decided at 146,247,871, its send returned
146,258,268 (10.4 ms, the shell idle); the next five returned +38, +208, +114, +64, +119 ms, each
decided within a millisecond of the previous return. First send with the shell idle: 10.4 /
10.9 / 14.1 / 10.7 / 9.9 ms over the five boots — the shift of a single-exit `deadline_notice`
from the dequeue to the send. R10 per destruction 9.3–10.1 ms (D = 6), 4.7–5.0 (D = 16), met.
**Lost notice**: rv64 1 hart D = 6 after, the ssh log has 5 `[lease alice/agent-N ended]` of 6
(`after/rv64-steward-deadline-burst.smp1-alice.ssh.log`); every other boot has all. The final
run's figures (the reverted steward, sampled at the send) are below.

## Findings (beyond the node)

1. The burst's last notice is bounded by the sponsor's take, ~N × 40–210 ms: the shell node.
2. A notice can be lost under a burst (a send past `NOTICE_US`): fixed by the same node; stated
   on steward.md with its evidence.
3. The first notice of a burst waits for the kernel's N destructions at the deadline's entry
   (~0.1 s net at six; each within R10): stated as a residual on both pages.
4. Any event or wake arriving among a burst waits behind the sends (0.77 s at D = 6, 2.2 s at
   D = 16 two harts): to be measured after the shell fix, a notice thread only if it remains.
5. A burst case that misses a bound idles to its `timeout_secs` (the oracle's count line never
   comes), 5400 s wasted per boot in the before run: noted for the bench.

## Final figures (the tree's own steward, sampled at the send; `final/`)

| boot | first notice gross / net µs | last notice gross / net µs | per notice, average | R10 p99 µs | `[lease ended]` lines |
| --- | --- | --- | --- | --- | --- |
| D=6 rv64 1 hart | 545,659 / 21,626 | 1,525,811 / 338,415 | 196 ms | 9,739 | **5 of 6** |
| D=6 rv64 2 harts | 313,392 / 25,486 | 1,794,150 / 556,600 | 296 ms | 9,615 | 6 |
| D=6 rv32 1 hart | 624,722 / 34,509 | 1,674,378 / 341,511 | 210 ms | 9,726 | 6 |
| D=16 rv64 1 hart | 830,667 / 87,767 | 2,106,125 / 548,988 | 85 ms | 4,926 | 16 |
| D=16 rv64 2 harts | 358,190 / 37,621 | 2,762,471 / 996,594 | 160 ms | 5,079 | 16 |

So the first notice's cost is mostly the checked build's audits (22–88 ms net), and the burst
is the sends. The lost notice reproduced: rv64 one hart D = 6, 5 of 6 in both runs (the
reverted steward too), so it is not an artefact of notices-first.

The latency cases at the send (deadline_notice p99 µs, were 8–16 ms at the dequeue): rv64 1
hart 17,882 / 18,016 / 19,734 / 18,813 / 14,349 (N = 1/2/3/4/16); rv32 1 hart 16,303 / 17,365 /
15,288 / 17,349; rv64 2 harts 30,393 / 32,398 / 24,567 / 27,239 / 29,907 — met, the two-hart
N = 2 row 8 ms under the target; the table's row on scheduling.md carries them (the rv32
two-hart cell is the earlier dequeue measurement, not re-measured).

## Gates (every command through `scripts/jobs.mk` under q; `final/make.log`, the per-case logs beside it)

- One `make -f scripts/jobs.mk -k -j8 set CASES=...` on `a18685d19` (the head's content but for
  the final figures in page, toml-description and comment text; the steward binary identical):
  **49 cases, 92 targets, every rc=0, `set: 49 cases`** — the 40 steward cases
  (`steward-set-cases.txt`: every `tests/steward-*.toml` but the latency and burst cases,
  `steward-host-tests` and `steward-model-host-tests` among them) both widths, the two burst
  cases (rv64 one and two harts, rv32 one), the three latency cases both widths as gated
  (steward-latency 528.7 / 607.2 / 697.6 s; n4 342.8 / 422.0 / 520.3 s; n16 154.6 / 190.8 s),
  `docs`, `formatting`, `size-budget`, `unsafe-budget`.
- On the head's text: `set CASES="docs formatting size-budget"` rc=0 each
  (`gates/host-gates-3.log`); `unsafe-budget` PASS earlier on the same code (no `unsafe`
  touched).
- `cargo test -p redoubt-steward-server`: 67 passed; `cargo test -p testbench
  only_steward_latency`: ok (the five builders). `cargo check --release` of the steward on
  riscv64gc and riscv32imac, with and without `latency-probe`: rc=0 all four.
  `cargo +nightly fmt -p redoubt-steward-server -- --check`: clean.
- Not run: the whole bench (the train's).

## Documentation check

- steward.md, scheduling.md: updated as above. steward-latency.toml's comment: updated.
- docs/testbench.md line 810 (`deadline_notice_p99_us=40000` as the oracle's example): still
  the latency cases' bound; unchanged. docs/userland/agents.md "Leases" (the sponsor is told
  when a lease ends): still true, the delay is on steward.md; unchanged. docs/SECURITY.md: R12's
  row and status section unchanged (the two cases were added to the Responsiveness status, not
  R12); R39/R40 tests unchanged. README.md, GETTING-STARTED.md, docs/plan/m3-agents.md: no
  claim about notice latency; unchanged. No dates, no package ids on pages or in case files.
- Reading: the two burst tomls in full (authored); steward.rs, case.rs, size-budget.toml,
  steward-latency.toml, steward.md and scheduling.md by their changed regions and the whole
  diff of each commit.

## Open

- notices-first withdrawn by ruling; the commit 57bff2058 (gathered exits, `Steward::exits`, the
  order host test) is in the reflog if the shell fix makes the order matter again (with fast
  sends the per-exit console lines, ~30 ms each at the turn, would be the next serial cost).
- The rv64 two-hart D = 6 "before" boot failed at alice's login (ssh exit 255 at 58 s), a
  transient; not rerun since the before figures at one hart carry the comparison.
