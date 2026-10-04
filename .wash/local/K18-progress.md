2026-10-02T20:56:44Z k18: scratch branch k18-scratch-before (307b5d818 + GATE1 fixture d7d1ab92d 555aeb6a5 e9ecc5ea5) running kernel-containment seed 4 both widths (before).
2026-10-02T21:03:03Z k18: before run done, both widths miss deadline notice (rv64 44343, rv32 45290 net p99); detail K18-before.md. Reporting checkpoint.
2026-10-02T21:05:29Z k18: sched::audit wrapper + both sites + audit-billed feature as WIP 47e336dd6 on wp-k18; after runs on k18-scratch-after (WIP + fixture): containment seed 4, sched-latency seed 3, containment seed 3.
2026-10-02T21:20:11Z k18: after seed 4 met both widths (net p99 rv64 25292, rv32 25728; c2 p99 2578/2693). Containment seed 3 after met (33928/25347). Batch 2 running: latency before/after w/o fixture, before containment seed 3, audit-billed negative.
2026-10-02T21:37:11Z k18: batch 2 done: latency seed 3 all met before/after; containment seed 3 before MISSED 43154/43997, after met 33928/25347; audit-billed negative misses 44343/45283. Pages edited, todo deleted, fmt+doccheck clean. Whole bench running (K18-bench.log).
## Whole bench (wp-k18 WIP 47e336dd6 + uncommitted pages), `in-dev cargo testbench --allow-skip`, exit 1
276 PASS, 1 SKIP (bench-ssh-loopback-openssh), 2 FAIL: sched-budget-churn rv64 and rv32, variant 1
"spinning parent destroying at its slice's end": victim 403 (rv64) / 416 (rv32) of 1000, attacker's
subtree 44 / 74 (needs victim >= 450). size-budget PASS, unsafe-budget PASS. Log: K18-bench.log.

Cause, confirmed: same case with kernel_features += audit-billed (temporary edit, reverted) on
rv64: PASS, victim 498, attacker 39. The attacker destroys a child every slice; each destruction
runs the checked build's audits. Billed, they rise the attacker's pass and the victim gets half of
the wall time. Unbilled (K18's rule), they cost nobody, so about half of the window is audit
time nobody pays for, and the victim's share of the *wall-time* window falls under half. The
victim's share net of audits would be well over half; a release build has no audits.
So the share judgement, like the latency targets, must be net of the checked build's audits, or
the rule must change. Asked the orchestrator (blocking): options in the message.
2026-10-02T22:04:51Z k18: option 1 built (SHARE lines, sched_oracle judges shares net; churn passes both widths: spinning-parent net 494/495 gross 403/416, shell 500/499). Folded into one commit 'kernel: a checked build's audits do not move its schedule' on wp-k18. Whole bench + release builds running (K18-bench2.log).

## Final (wp-k18 a22ac66b2, one commit on main 307b5d818, 12 files, +214 -109)
- Whole bench `in-dev cargo testbench --allow-skip`: exit 0, 278 PASS, 1 SKIP (bench-ssh-loopback-openssh). size-budget PASS (no raise), unsafe-budget PASS (no unsafe added; kernel core 19 / arch 13 / backends 13, 0 undocumented, unchanged). Log K18-bench2.log.
- `./build --arch rv32` and `./build` exit 0. fmt --check clean. doccheck clean.
- Seed 4 two-lease (scratch k18-scratch-after = kernel change + GATE1 fixture), deadline notice net/gross p99 µs:
  before rv64 44343/114124 MISSED, rv32 45290/116777 MISSED; after rv64 25292/95077 met, rv32 25728/97232 met.
  Split p99 (a/b/c1/c2) rv64 before 385/22124/566/21676 after 283/22128/563/2578; rv32 before 358/22278/732/22297 after 439/22273/732/2693. Files K18-before-split.txt, K18-after-split.txt.
- audit-billed negative (two-lease seed 4): rv64 44343, rv32 45283 MISSED. Recorded on scheduling.md beside audit-unstamped.
- Containment seed 3: before 43154/43997 MISSED, after 33928/25347 met.
- sched-latency seed 3 (main vs wp-k18, no fixture): every target met both. Moves at p99: rv64 notice N=16 5352->13896, N=4 12529->5052; rv32 notice N=16 5615->13055; rv64 decision N=16 17919->39449, lease end 23499->45815 (<=125000); R10 p99 5580->6366 / 5863->6577. Phase steps of about one slice; not traced. Table on scheduling.md re-measured.
- Churn victim share (of 1000): before main gross spinning 498/493, shell 494/494. After net (gross): spinning rv64 494 (403), rv32 495 (416); shell 500 (264), 499 (293). Main's kernel judged net: shell 650/643 (missed upper 550).
- Follow-up (not done, outside paths): other share cases (sched-exit-churn, deadline-flood-billed, sched-share, ...) judge gross and have no trace, so audits are not subtracted there; they pass today.
- Scratch branches k18-scratch-before / k18-scratch-after hold the GATE1 fixture; never on wp-k18. Delete at merge.
2026-10-02T22:32:22Z k18 round 1: amended to c03900a6f (started under cfg; rewraps; B5 residual + docs/todo/shares-judged-gross.md + SUMMARY verbatim). audit-billed kernel builds 0 warnings rv64/rv32 (checked, with/without sched-trace). fmt, doccheck clean. sched-latency, sched-budget-churn, kernel-containment (on 16073f4c6 + fixture, docs-only diff since) PASS both widths; containment notice 25292/25863. Scratch branches deleted. Clean tree.
