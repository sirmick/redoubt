# SCHED4 report (k33-implementer-3, resident; worktree .worktrees/SCHED4, wp-SCHED4 from 92b84a5b8)

## Design

Rule. A timer interrupt for a timeout or a deadline is taken on the hart that runs its payer; a
hart that runs no payer of an item comes by it only when no hart runs that payer. Form, as the
Architect amended: `time.rs` keeps, per kind, the global earliest item and its payer's budget id
(`threads`/`threads_for`, `budgets`/`budgets_for`), `None` when two payers tie at one instant;
the payer of a timeout is the blocked thread's budget, of a deadline the dying budget's parent
(the bill's payer at destruction may be an ancestor with free weight; the placement reads the
parent). `lowered((hint, by), at, payer)` keeps the pair as the WIP does.

Where decided: only `rearm()`, which arms the earliest of the hart's slice end and each kind's
item unless another hart runs that item's payer (`sched::runner_ids()`, O(harts), reading
`Harts.runners[].cur`) and is armed at or before it; with `None` or a payer that runs nowhere,
every hart arms it, as today. `rearm` runs where it runs today: the return to user mode
(`sched::leave`, `pid != 1`), before `kmain` idles, and at a timer interrupt taken in the idle.
`note_timeout`/`note_budget_deadline` only record (the WIP's extra `rearm()` calls are dropped:
the entry's own return re-arms, one SBI call per exit stays); a cancel (a wait ended early, a
budget destroyed by hand) leaves the hint early, as today. No scan of the deadlines on the switch
path: `expire_due` alone recomputes both pairs, from `collect_due`'s walk (`Due::next_for`) and the
deadline list (parent id), as the WIP does.

Payer runs nowhere: every hart arms the item, so a one-thread budget's own timeout (it blocks, so it
runs nowhere) is unplaced by construction; that is the residual the timeout counts measure
(exit-churn 0.15-0.23 %). The gate is for deadlines, whose payer (the creator) runs. An idle hart
arms an item only when its payer runs nowhere, so a deadline whose creator runs waits for the
creator's hart (its section, if in one): the notice latency cost, measured below.

K33: both slice ends return through `leave`, which re-arms with the runner set as it is after the
switch: in place the runner is unchanged; the direct switch arms the pick's items on this hart,
and the budget switched from, if it now runs nowhere, is armed everywhere. `repick`'s `due()`
reads the global hints as today, so an item due that another hart is armed for still sends this
slice end through `kmain`, whose expiry serves it here: the placement is of the interrupt, not an
exclusion; a passed item is served by the first entry anywhere (I13, R10 ordering unchanged).
Hence the gate is a bound, not zero.

Negative. Where the deadline falls while the creator makes calls, its own entries serve it (the
"91 of 96 on the creator" runs) and `timer-any-hart` lands on the victim only by phase (5 of 96,
~1 %): not a negative. Construction `deadline-placement`: creator C (one thread) and spinner V, equal
weight, two harts; C creates its round of deadline budgets then spins in user mode making no call
until the window ends. Any-hart: both timers fire at each deadline, icount's turn decides, a
substantial share lands on V (~0.19 % of V's pool per landing; 8+ landings fail 1 %). B1: only C's
hart is armed, C is in user mode, every destruction lands there. Case `deadline-placement`
(checked, `sched-trace`, smp 2) gates the count; the recorded negative is the same with
`kernel_features += timer-any-hart` (the WIP's feature). deadline-flood-billed-traced and
sched-exit-churn also gate the count at 2 harts (their any-hart outcome is timing-bound, reported).

Oracle. New arg `foreign_timer_in=<share>:<per_mille>`: inside the share's window, timer
interrupts from user mode that interrupted the judged budget (the mark's, or one lifted into it)
and charged a budget outside its marks; their ticks over the judged budget's charged ticks (the
share's `its`) at most per_mille. Judged at 2+ harts, recorded at 1 (at one hart every interrupt
is on the victim's hart). Report-only count unchanged. Gate: 10 per mille in the three cases.

Latency. `sched-latency` and `kernel-containment` at 2 harts, both widths, before (92b84a5b8) and
after: deadline_notice net p99 at N = 1, 4, 16 (baseline 4.6/5.1/4.6 ms rv64, 6.2/6.1/5.2 ms rv32);
after <= before + one kernel section, the longest the same run's `hold-trace` twin or
kernel-containment's `kernel sections` line records. Pages: timer.md "The hart timer" (the rule,
payers in the figure's three times), "R12 (scheduling) for timer work" (the residual becomes the
rule; residuals: ties, payers running nowhere, an entry's expiry on another hart, device
interrupts stay IRQ1's); scheduling.md residual risks' pointer; testbench.md the arg. Model: no
rule changes (the model has no per-hart timer; its bills are unchanged), so no mutation; the
kernel feature `timer-any-hart` is the negative. Commits: (1) kernel + pages, (2) oracle arg +
host tests + the three tomls + `deadline-placement` program and case, (3) measurements into docs.

Ruling (kernel-red-2, QA SCHED4-design, 2026-10-14): build as designed; bindings: (1) liveness
invariant stated in timer.md and host-tested on the pure decision (fallbacks part of the rule; a
tie is two different payers); (2) due() keeps the global hints; testbench.md says foreign_timer_in
counts timer interrupts from user mode only, and the oracle reports (not gates) foreign expiry
ticks reached on the judged budget's hart through kmain/its own calls; (3) gate 10 per mille at
2+ harts; timer-any-hart must fail deadline-placement on both widths, quoted; (4) latency after <=
before + the run's longest section, both widths, N = 1/4/16, in timer.md; (5) K33's cases
re-judged (sched-timer-entry p99 delta; wake-no-preempt-harts and twin, condition-5 style; floors
unmoved without a ruling; sched-latency, kernel-containment at 2 harts); (6) three commits, each
reviewed on the thread; no model rule change.

## Commit 1: b2f930729 on 92b84a5b8 (2026-10-14)

kernel/src/time.rs: `Timer.threads`/`budgets` are `redoubt_stride::Hint` (at, payer); note_* take
the payer and only record (no rearm added); `rearm` decides with `Hint::due_on(h, runner_ids(),
armed)` per kind, `timer-any-hart` falling back to the plain `at`; `due()` reads `.at`;
`expire_due` recomputes both hints with payers (the walk's `Due::next` is a Hint; the deadline
fold uses the parent's id). message.rs `settle`/`collect_due` pass `budget_of(pid)`'s id;
budget.rs passes the parent's; sched.rs `runner_ids()`; Cargo.toml feature `timer-any-hart`.
libs/stride: `Hint` (`NONE`, `lowered`, `due_on`) + three host tests (lowering and ties; liveness
and placement over 20,000 random runner/item sets iterated to the harts' re-arm fixpoint; a hart
covers an item only when armed at or before it, `armed 0` = fired and re-arming counts).
Pages: timer.md hart-timer status (tested 5 -> 8: the three host tests), the rule paragraph, the
figure's hints and decision box, "R12 for timer work" rewritten as the rule + what still lands
elsewhere (with TBD-NEG-*, TBD-GATES, TBD-LAT-* for commits 2-3), a residual-risks bullet and the
"kernel work is not interrupted" bullet's new cost; scheduling.md's residual bullet points at it.
Checks: `cargo test -p redoubt-stride` 28 pass (.tmp/SCHED4/c1-stride.log); kernel builds rv64 and
rv32: release, checked+sched-trace, checked+sched-trace+timer-any-hart all rc 0
(.tmp/SCHED4/c1-build.log); rustfmt clean; size-budget PASS (kernel 10757 -> 10772, stride 929 ->
953, two budget lines), docs PASS, formatting PASS.

## Commit 1 amended: 789b5e83f (the P3s folded, 2026-10-14)

kernel-red-2's P3 (a)-(c) folded into commit 1 (no re-review asked): (a) libs/stride/src/tests.rs
`an_item_stays_covered_when_the_hart_that_covered_it_switches_away` (the covering hart switches
to another budget and re-arms: it arms the item itself while the payer runs nowhere, keeps it
while a third hart runs the payer armed later, leaves it once that hart is armed at or before it;
the invariant checked at each step); timer.md status tested 8 -> 9. (b) "R12 for timer work"
first sentence has the third clause ("or the payer's hart is not yet armed by then"). (c) the
"What still lands elsewhere" paragraph and the residual-risks bullet name a destroyed payer whose
id is reused before the expiry recomputed the hint (placement only, never the bill). (d) is
commit 3's. Code, message and Size budget lines unchanged (tests are not counted: stride 953 of
953). Checks on 789b5e83f: stride 29 pass (.tmp/SCHED4/c1b-stride.log), rustfmt clean, docs PASS,
size-budget PASS (.tmp/SCHED4/c1b-gate-*.log). Commit 2 rebased onto it.

## Commit 2: e7f4744a8 on 789b5e83f (2026-10-14)

tools/testbench/src/sched_oracle.rs: arg `foreign_timer_in=<share>:<per_mille>` (refused
malformed or naming no share); `Summary.timer_foreign_entries` (each foreign interrupt: at,
interrupted, charges) and `Summary.expiry_charges` (`$`); `check_hart_share` returns `HartJudged`
(line, met, judged budget, its ticks); `foreign_timer()` sums, inside the share's window, the ticks
of timer interrupts from user mode that interrupted a budget under the judged one's marks
(`root_of`) and charged a budget outside them, per mille of the judged budget's charged ticks:
gated at harts >= 2, "recorded at 1 hart, not gated" below, and reports the `$` ticks charged to
others at the judged budget's own entries (runner under the marks) and in `kmain` after it left
(runner None, last runner under the marks); the share's line carries it. Parse string gains `$`;
a `$` inside an interrupt is an error. Module and `run` docs name the arg.
Host test `timer_work_billed_elsewhere_in_a_budgets_time_is_bounded_inside_its_share`: round-robin
fixture + an inserted `I 44 125, E 9 1, B 9 500, O 1` (500 of 44's 10,000 ticks = 50 per mille):
`v:10` fails and `v:60` passes at F=2, recorded at F=1; an interrupt that interrupted 9, or
charged 44 itself, counts 0; two `$` records (payer 9, 500 ticks) while 44 runs and after its slice
ended read "500 ticks at its own entries and 500 in `kmain` after it left" and a `$` for 44 does
not count; `$` inside an interrupt refused; four malformed args and an unknown share refused.
kernel: `trace::EXPIRY_CHARGE` (`$`: payer id; pass = ticks | now_us << 32) written by
`trace::charge` when `RING.expiry && !interrupt`; `trace::expiry(bool)` set by `time::expire_due`
around an expiry that found something due (cfg sched-trace both). +12 kernel lines, ceiling 10772
-> 10784 with the message's Size budget line.
tests/programs: `Role::DeadlineFlood` p1 = the deadlines' offset in µs (200 when 0; the two
existing callers pass one param, unchanged); bin `deadline-placement` (creator DeadlineFlood
[16, 600] weight 100 slot 3 + Spin victim weight 100 marked 2, window 2 s, `HART-SHARE placement
... 50+ 2:1 100:1`). tests/deadline-placement.toml (checked, sched-trace, smp [2] keep_smp,
`foreign_timer_in=placement:10`; the toml says the round's children share the creator as payer:
no tie, per binding 1; TBD-NEG-RV64/RV32 for the negative's per mille). deadline-flood-billed-traced
(`16-deadlines`, `64-deadlines` at 10; TBD-GATE) and sched-exit-churn (its three shares at 10;
before-numbers from the K33 runs in the comment) gate the bound; both have no smp list (the bench's
default), so the gate judges only where the bench runs them at 2 harts (`--smp 2`, machine step).
Pages: testbench.md oracle paragraph (the arg counts timer interrupts from user mode only; `$`
reported, never judged: binding 2), oracle status 29 -> 30 (the host test), checked-builds status
16 -> 17 (deadline-placement); timer.md hart-timer status + bench:deadline-placement (10).
Checks on e7f4744a8: `cargo test -p testbench sched_oracle` 58 pass (.tmp/SCHED4/c2-oracle-test.log);
stride 29 pass; kernel rv64 and rv32: release, checked+sched-trace, +timer-any-hart all rc 0, the
same 48 pre-existing dead-code warnings as commit 1 (.tmp/SCHED4/c2-kernel-build.log);
test-programs deadline-placement + deadline-flood-billed built for both targets
(.tmp/SCHED4/c2-programs-build.log); rustfmt clean; size-budget PASS (kernel 10784 of 10784), docs
PASS, formatting PASS (.tmp/SCHED4/c2-gate-*.log). No case has run on the machine.
Summaries checked: docs/kernel/README.md (timer row: a pointer, no claim moved), README.md,
GETTING-STARTED.md, SECURITY.md (R12 row lists cases; the hart timer has no row; unchanged, the
new case is timer.md's), scheduling.md (the residual bullet is commit 1's; its "482 to 877, by
where the destructions land" at ~641 and the two-hart table rows 664-665 are measurements commit 3
re-states with the gate's numbers).
Review (kernel-red-2, SCHED4-design): OK to build commit 3 on. P3s for commit 3: (a) say `$`'s
time (now_us in 32 bits) wraps at 2^32 µs, 71 min, longer than any case; (b) quote in
deadline-placement.toml the `$` ticks at the creator's own entries when measured (early deadlines
the checked build's 16 creates reach). Binding for commit 3: TBD-GATE / TBD-NEG-* / TBD-LAT-*
filled from runs; the negative on both widths with the per mille; binding 5's re-judging reported.
## Machine step (2026-10-14, logs .tmp/SCHED4/meas/)

Prebuilt at e7f4744a8 rc 0 (rv64 292 / rv32 278 cases). Before tree .tmp/SCHED4/before at
92b84a5b8, prebuilt rc 0. Negative scratch .tmp/SCHED4/neg at e7f4744a8 + tests/*-tmp tomls.

deadline-placement (icount, smp 2): PASS both; victim share 924 / 918 (rv64 / rv32); foreign
10,993 ticks = 0 per mille / 14,752 = 1; `$` 0 at its entries, 1,676 / 0 in kmain
(rv64-deadline-placement.log, rv32-...). Negative (timer-any-hart, deadline-placement-any-hart in
neg/): PASS both: 5 foreign interrupts, 21,975 ticks = 1 per mille / 14,745 = 1 (neg-rv64.out,
neg-rv32.out): NOT a negative under icount. Its trace (neg-last/): the victim's hart takes one
destruction a round (3.7k ticks each, 415 ms apart = 16 destructions of 26 ms), i.e. it wins
the race for the round's first deadline, destroys one, and the rest stay on the creator's hart.
deadline-flood-billed-traced --smp 2: rv32 PASS (16-deadlines 919, 64-deadlines 920; foreign 0/1
per mille); rv64 FAIL twice with different modes: run 1 16-deadlines 924 / 0 per mille,
64-deadlines 530 / 235 per mille (70 foreign interrupts, 259,174 ticks); run 2 (flood-last/,
console kept) 16-deadlines 507 / 114 per mille, 64-deadlines 526 / 241 per mille: not
deterministic across runs with the same bundle. Mechanism from the trace: the creator refills each
slot a destruction frees with a deadline 200 µs ahead; the victim's hart (never armed for a
deadline: the creator's hart is) reaches it first at its own 1 ms slice end's interrupt, under
icount's turn order, every 26 ms cycle (gap 25.9 ms between its foreign interrupts, each 3.8k
ticks, each returning O 2 to kmain; the creator's hart's slice end 0.7 ms earlier found nothing
due; then its own deadline interrupt finds it gone): the "passed item at some other entry"
residual through a slice-end timer interrupt (K33's in-interrupt expiry), locked into one phase or
the other per run. sched-exit-churn --smp 2: rv32 PASS (0 foreign interrupts in all three
shares); rv64 FAIL threads-exit 20 per mille (164 interrupts, 323,661 ticks; payer-nowhere
timeouts, the designed residual, at a 50/50 phase, not the 0.15-0.23 % the design assumed from
one phase); processes-exit/-fault 0. Both failures carry the launch-env trap's signature (moves
deterministically by phase, 0 on one width): mttcg twins requested before any bound moves.
mttcg twins (neg/tests/*-mttcg-tmp.toml = the toml without `icount`, smp [2]; `q run --quiet`,
one at a time on the quiet cores, with the before-latency job and B48's prebuilt beside on the
general cores; logs meas/mttcg/): deadline-placement B1 PASS both, victim 726 / 715, foreign 1
interrupt 7,029 / 6,492 ticks = 0 per mille; any-hart FAIL both: 74 interrupts 251,881 ticks = 21
per mille (rv64), 89 / 364,375 = 29 (rv32), victim 713 / 701. deadline-flood-billed-traced PASS
both: 16-deadlines 773 / 766, foreign 34,259 / 36,105 ticks = 2 per mille; 64-deadlines 802 / 788,
0 per mille. sched-exit-churn PASS both: threads-exit 511 / 523, 25,113 / 76,380 ticks = 1 / 4 per
mille; the other two shares 0. So where the harts run at once the rule holds at the bound in all
three and the negative fails in both widths; under icount the same cases read turn order.
Ruling A (kernel-red-2): host-clock twins judge at 10; icount cases record; the residuals'
mechanisms and icount costs into timer.md; five runs a width per twin alone on the quiet cores.
Five runs (meas/mttcg5/, all PASS; per mille of the victim's charges, share): deadline-placement
rv64 0,0,1,0,0 (0-13,366 ticks; share 703-734), rv32 0,1,0,0,0 (0-14,410; 698-710);
deadline-flood-billed-traced 16-deadlines rv64 2,1,4,3,3 (24,606-55,657; 772-776), rv32
2,3,2,2,3 (31,321-45,146; 762-765); 64-deadlines rv64 0,0,0,1,0 (3,917-16,293; 798-802), rv32 all
0 (1,522-10,881; 788-791); sched-exit-churn threads-exit rv64 all 0 (6,023-12,887; 508-509), rv32
5,4,2,1,4 (35,908-111,457; 513-530); processes-exit / -fault 0 everywhere (one 1,113-tick run).
Any-hart negative on the host-clock deadline-placement: 21 / 29 per mille (one run a width).
Latency BEFORE (92b84a5b8, meas/before/; all PASS): sched-latency deadline_notice net p99 µs at
N = 1 / 4 / 16: 1 hart rv64 2,237 / 2,252 / 2,347, rv32 2,317 / 2,309 / 2,513; 2 harts rv64 3,297 /
3,409 / 3,764, rv32 3,985 / 4,079 / 5,093. kernel-containment smp 2: deadline_notice (18) net p99
24,027 (rv64), 27,349 (rv32). kernel-containment has no hold-trace, so the "longest kernel
section of the same run" comes from sched-latency-hold-tmp (sched-latency + hold-trace, smp 2,
neg/; meas/hold/).
Latency AFTER (e7f4744a8, meas/after/; all PASS): sched-latency deadline_notice net p99 µs at N =
1 / 4 / 16: 1 hart rv64 2,241 / 2,249 / 2,374, rv32 2,316 / 2,314 / 2,494; 2 harts rv64 3,535 /
3,428 / 3,758 (before 3,297 / 3,409 / 3,764: +238 / +19 / -6), rv32 3,933 / 4,104 / 5,133 (before
3,985 / 4,079 / 5,093: -52 / +25 / +40). Kernel sections of the same program at 2 harts
(sched-latency-hold-tmp, meas/hold/, PASS both): net of audits p50/p99/max ticks rv64 2,772 /
10,167 / 717,103, rv32 4,675 / 11,053 / 459,639; at 10 ticks/µs the largest rise, 238 µs = 2,380
ticks, is within the p99 section (1.0-1.1 ms), far within the longest (46-72 ms). Its own
deadline_notice p99 (2 harts): rv64 3,295 / 3,423 / 3,765, rv32 3,995 / 4,104 / 4,993.
Binding 5: sched-timer-entry five runs a width alone from the head's prebuilt (meas/te5/, all
PASS): 1 hart timer sections p50/p99/max 943/945/3,540-3,548 (rv64), 1,416/1,441/4,421-4,446
(rv32), from 928/930/3,461 and 1,388/1,415/4,265: +15 / +26 ticks, the runner_ids() read per
re-arm; bound 2,000 unmoved (1.39x). 2 harts recorded p99 5,445 / 7,375 (from 5,433 / 7,354; p50
2,940 / 5,368). CORRECTION: the "9,405 / 8,655" I reported earlier (and kernel-red-2's P2 quoted)
was the 1-hart hold-trace line for ALL kernel sections ("kernel sections: 436 held ... p99 9406",
the launcher's calls included), misread as the 2-hart timer line; the timer line at 2 harts
barely moved. sched-wake-no-preempt-harts PASS: rv64 35 mid-slice + 67 at another entry = 102
witnesses (was 94-107), rv32 0 + 74 = 74 (was 65-70).

## Commit 3: c5577bf30 on e7f4744a8 (2026-10-14)

Ruling A. tests/deadline-placement.toml: no icount (host clock; quiet class: scripts/jobs.mk
`quiet +=` the three cases), description + comments (five runs, the negative 21 / 29, the `$`
totals 236 / 153 at a call's entry / 83 in kmain / 100,388 ticks rv64 and 128 / 74 / 54 / 76,299
rv32 from the final run, the K31 wording), timeout 300. New tests/deadline-flood-billed-traced-mttcg.toml,
tests/sched-exit-churn-mttcg.toml (smp [2], keep_smp, foreign_timer_in=10 for every share, the
five runs and the icount numbers in comments). tests/deadline-flood-billed-traced.toml and
sched-exit-churn.toml: `post_check = "sched_oracle"` (report only), comments with the icount
modes and the mechanism. tools/testbench/src/sched_oracle.rs: summary head adds the `$` totals
(count, at a call's entry, in kmain, ticks); the `$` host test asserts "3 expiry charges outside
an interrupt (2 at a call's entry, 1 in `kmain`), 1500 ticks". kernel/src/sched.rs: EXPIRY_CHARGE
doc says the time wraps at 2^32 µs (doc only; size-budget 10784 unchanged). tests/programs
deadline-placement.rs header corrected (a round's first at a timer interrupt, the rest at
whichever entry comes first). docs: timer.md R12-for-timer-work status + the three cases, the
prose with the five-run numbers, the negative, the icount modes, the two residual mechanisms and
their icount costs, latency before/after; testbench.md `$` wrap, the summary totals, where the
bound is judged, checked-builds status 19; scheduling.md the flood victim's modes (641-645), the
two table rows, the residual bullet, wake witness ranges (103-104 65-74 rv32 at a call; 108 26-35
mid-slice rv64; 112 69-75 beside other cases; 1458 57-75); SECURITY.md R12 row + the three cases.
Final tomls once each (meas/final/, quiet, PASS): deadline-placement 0 / 0 per mille (722 / 708);
flood 2, 0 / 3, 0 (775, 801 / 765, 788); churn 0, 0, 0 / 2, 0, 0 (510-571 / 521-557).
Gates on c5577bf30: oracle 58 pass (meas/final/c3-oracle-test.log), fmt check clean, size-budget
PASS, docs PASS, formatting PASS (meas/final/c3-gate-*.log). Then prebuilt + the sched set (all
sched-* but sched-cluster-old-control) + deadline-*, irq-boot-hart-only, beamlet-reduction-rate,
beamlet-schedulers-mttcg, kernel-containment, smp-*, asid-reuse-stale, lend-untouched-page
(meas/sets/).
Summaries checked (commit 3): README.md, GETTING-STARTED.md (no timer/case claims), docs/README.md
and kernel/README.md (pointers only), SECURITY.md R12 row updated, testbench.md (oracle
paragraph + statuses), scheduling.md (measurements re-stated), timer.md (owner page).

Sets at c5577bf30's tree (meas/sets/, prebuilt rc 0): `make -k -j 4 set CASES="<29 sched-*>
deadline-flood-billed deadline-flood-billed-traced deadline-flood-billed-traced-mttcg
deadline-placement irq-boot-hart-only beamlet-reduction-rate beamlet-schedulers-mttcg
kernel-containment smp-boot smp-shootdown smp-evict smp-fence smp-inflight-race smp-lock-wait
smp-evict-mttcg smp-shootdown-mttcg asid-reuse-stale lend-untouched-page"`: set rc 0, 99 targets
rc 0, 110 PASS lines, no FAIL (B48's prebuilt and cases ran beside on other cores).

Commit 3 amended -> 6b3ae7d3c (kernel-red-2's P2 + P3 a, b): tests/sched-timer-entry.toml and
scheduling.md 147-150 quote the head's five alone runs (1-hart p50/p99 943/945 and 1,416/1,441,
cold max 3,548 and 4,446, bound 2,000 = 1.39x; 2-hart recorded p99 5,445 and 7,375 from 5,433 and
7,354, the steady section's rise) and name the placement's cost (15 / 26 ticks); the icount flood
toml says a share failure in its 507-530 phase is the phase, judged by the twin; the table rows
read "507 or 924 by phase". Message amended accordingly. Gates on 6b3ae7d3c: size-budget, docs,
formatting PASS (meas/final/c3b-gate-*.log); no TBD anywhere. Branch wp-SCHED4 = 789b5e83f,
e7f4744a8, 6b3ae7d3c on 92b84a5b8; tree clean. Scratch worktrees .tmp/SCHED4/neg (e7f4744a8 +
tmp tomls) and .tmp/SCHED4/before (92b84a5b8) still registered: `git worktree remove` them at
the merge.

VERDICT (kernel-red-2, SCHED4-design, 2026-10-14): SCHED4 merge OK at 6b3ae7d3c (789b5e83f,
e7f4744a8, 6b3ae7d3c on 92b84a5b8), all bindings met; merges whole, in its train. Frozen.

Commit 3 plan was: deadline-placement both widths + the timer-any-hart negative in a scratch
worktree; deadline-flood-billed-traced, sched-exit-churn --smp 2 both widths; sched-latency and
kernel-containment at 2 harts before/after (N = 1/4/16 deadline_notice p99, longest kernel section);
binding 5's re-judging; the TBD-* and scheduling.md's numbers.
