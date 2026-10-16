# kernel-red-2 handoff (2026-10-16, checkpoint at 70 % context)

## Branch state and traps first

Reviewer: no branch, no worktree of my own, nothing to commit. I never wrote a file. Read in the implementers' worktrees (`.worktrees/<PKG>`) with `git diff`, `git show`, grep, sed.

Traps found this session (each already fixed or ruled; the rule stands for your reviews):

- **Report-tag collision (ATK2, 0396a6edd).** Stand-in reports on slot 1 carry `tag | count << 8`; `Bench::samples` names a measure by `tag & 0xff` alone. `Stats::DESTROY_CALL` was 7 = the containment steward's `CT_VERDICT`, so kernel-containment printed a `destroy_call` count with no window and the oracle's counted-but-not-printed arm tripped. Now 10. The slot-1 tag space is Stats 1-6, 10 and CT_VERDICT 7, CT_STEADY 8 (CT_CALLERS 4 and CT_LEASES 9 are counts, not tags). Documented by comment only; a new measure must check both sets. A P3 stands: one enum would make it a compile error.
- **Prebuilt gap (ATK4).** A package changing the bench binary (fdt.rs, the device_tree knob) ran its 66 case runs through `cargo testbench` and never `make prebuilt`. Rule I applied: a package's own gate must include prebuilt for any bench-binary change; I made ATK4's verdict conditional on it.
- **fdt in-place discipline (ATK4, tools/testbench/src/fdt.rs).** QEMU's tree is dumped (`-machine dumpdtb`), edited in place (drop and a shrink's tail become FDT_NOP, set keeps the length) so no byte moves and the header's offsets/totalsize stay true; malformed edits are refused by `edit()` before QEMU starts. Any future tree edit that moves bytes must rewrite the header, or it is wrong.
- **K31 trap, again (IDLE2/B63).** sched-capped's second-cap clause at 3 harts under icount failed 2/5 on main itself: no timeout in the window, three slice ends meeting at the lock in QEMU's fixed turn order, the last hart losing 23-45 % to lock waits. Ruled: judged in the mttcg twin at 3 (B63, smp [2,3]), recorded under icount (hart_shares_from=4). Any share or latency read at 3+ harts under icount is suspect first; a twin where harts run at once judges.
- **Cases without keep_smp get swept at --smp 2 by the whole bench** (case.rs `with_harts`). A new one-hart proof (hold-trace sections, a two-runner phase) must set `keep_smp = true` with its reason and join scheduling.md's one-hart table (ATK2's P1, fixed in 877a78c4a).
- **CONW2/B57**: a program's line spans 9P writes; between writes nobody holds the console; `console_hold` gained ReleaseOpen (3) and `Return::Wrote(bool)`; conhold keeps `open_by`/`wrote_for` PIDs; a queue flushed at a ReleaseOpen writes `\r\n` first. Merged.
- **SCHED4/IDLE2 liveness argument**: every kernel exit re-arms with the runners as they are then; `armed == 0` means fired and re-arming (covers); a hart covering only by an earlier slice end re-arms at that end. Host tests hold steady state and transients. The one-hart rule's cost is wake latency under a burst of payer-nowhere timeouts (timer_wake N=16 +1.6/3.7 ms), named in timer.md with a trigger.

## Verdicts in force (mine)

- K33 ec41f5286 merge OK (merged). SCHED4 6b3ae7d3c merge OK (merged). IDLE2 539344966 merge OK with a doc P2 (merged, train 26, then cleared on IDLE2-sched-capped). B63 2404fd945 OK (merged). CONW2 a40d75bd9 OK. B57 cd40d1eb2 OK (needs CONW2). B67 808327947 OK with notes. B66 2855ee3eb OK with notes. B73 18969e24b OK. ATK3 0e650b847 OK. ATK2 0396a6edd OK (renewed twice). ATK4 d7cfe6225 OK conditional on `make prebuilt` passing on its head or the train's.
- B77 (wp-B77 3ddf256b1 on 6713d6ffa): IN PROGRESS, no verdict. Assignment 10460770e023d6d9336d9efe26e2a98d is open; your successor takes it.

## B77: what I had read (nothing concluded)

Read: the full `scripts/q` diff (port pool, `find_ports` bind-probed, `held()`, `start()` re-checks and keeps the job waiting if a stranger bound a port since `fits()`, `adopt()` reads REDOUBT_PORTS from /proc/<pid>/environ filtered to the pool, `--quiet` default None -> last four, `''` -> none, `--ports` default 23000-23999, client fallback to `--lock net` with a stderr note under an old daemon, child env gets REDOUBT_PORTS or has a parent's popped); jobs.mk's net recipe (`--ports <forwards + pokes>` from the toml by grep; today every [net] case is 1 TCP forward + 0 pokes except bench-net-peer* with none); qemu.rs `host_ports`/`leased_ports` (+ host test); main.rs launch path; testbench.md's two paragraphs; the report's before/after, restart, gates, notes; q-test's shape (14 checks, private socket).

Observations to carry (not yet weighed into a verdict):
1. Residual stated correctly in the report's Notes (stranger binds between probe and QEMU's bind: fails loudly, same as before, now only to strangers) but NOT on testbench.md as a residual sentence: check the page's "a lost race fails the boot rather than hiding (QEMU cannot bind)" sentence covers it; it is in the outside-q paragraph, so the under-q residual may be missing. Likely a P2 wording.
2. `virtio_devices` with `given` ports (launch.rs:249, the launch machine) still picks the poke's UDP port by `peer::free_udp_port` (OS-picked) — launch has no poke today, so harmless; note.
3. jobs.mk's N comes from the toml by grep at recipe time, so a case adding a forward raises N automatically; a poke counts via `^\[net\.poke\]`. Only a test or page rule would hold the two greps to the bench's parsing; `leased_ports` refuses a too-small range loudly, so an undercount fails the case rather than racing. Probably fine; say so.
4. Fallback: `--lock net` remains in `.wash/local/launch-env-2026-10-09/12.md` (members' launch texts, outside the repo) — the orchestrator's to update; the repo's only mention is testbench.md:339 as history.
5. adopt(): an unreadable /proc/<pid>/environ (another user) hits the `except OSError: continue` so the job is not adopted at all (cores not marked either) — same as before for cores? No: before, environ was not read, so such a job's cores WERE adopted; now the environ read failing skips the whole job. Check whether the `continue` covers the environ open (it is inside the same try). If so, a P2: a job of another user pinned to our cores is no longer adopted. Worth confirming in the code at `adopt()`.
6. `q daemon --quiet ""` semantics changed (None -> last four; "" -> none): help string says it; check testbench.md says it (the message's question 4).
7. Class (ii) unchanged per the report's Summaries.
Not yet read: `scripts/q-test` body in full; the gate logs under /home/mcloonan/redoubt/.tmp/b77-gates/; the report's Lease design section (lines 6-14).

## House rules I apply (kernel/bench reviewer, Tier A)

- First line of a result is the verdict: `Merge verdict: OK | OK with notes | BLOCK`; `OK to build commit N on` for mid-package commits. Record base, head, paths read, findings P1/P2/P3. Results <= 2000 bytes (they bounce otherwise; trim, do not file-dump). QA-thread messages <= 2000 bytes too; split into "1 of 2 / 2 of 2" when needed.
- Never write: no Write/Edit, no redirects, no git that changes anything. Read in the implementer's worktree. If a recipient is "not an active member", send to the orchestrator on the same thread.
- Numbers on pages must be the head's: a bound's margin restated, before-numbers kept as "before"; five runs a width alone for any quoted latency/section number; a moved recorded number needs its reason or it is a finding (SCHED4 commit 3's P2).
- Bounds never widen without a ruling; a worse gated number is a finding, not a bound move; a twin's failure alone is a finding, not a rerun.
- Negatives: shown once, failing on the case's own expect/forbid with the planted number quoted; a case whose negative passes proves nothing (deadline-placement under icount) and loses its gate.
- icount: any share/latency/section read at 2+ harts is suspect of QEMU's turn order (scheduling.md "A hart waits for QEMU's turns"); gate in the host-clock twin on the quiet class (one such case at a time; the 2-core lease starves the twin: lease stated in the toml), record under icount with the mechanism stated.
- Oracle (sched_oracle.rs): proof-shape readers may be amended under condition (4) only with my ruling first; rank/floor/wake/lift clauses untouched; a reading must count what the victim's hart did, never redefine it (refused option B in SCHED4). `$` records expiry charges outside interrupts (reported, never judged). O 0 = the slice ended at this entry (to kmain or in place). Latency targets judged net of audits; new measures join MEASURES and need a report tag free in both tag sets.
- Kernel changes: no synthetic trace records; a release-code delta between rounds must be exactly the stated fix; Size budget lines per crate per commit; unsafe-budget; every bin linking redoubt_rt rebuilt when libs/rt changes.
- Pages move with the code: statuses' counts match the lists; "argued" vs "attacked" said honestly with premises; SECURITY.md rows match the page's residuals; when two packages touch the same row (ATK2/ATK3 R5), I state the merged text in the verdict.
- Bench-binary changes: prebuilt is a gate; a `build`-kind case's `must_fail` and `forbid_in_binary` are the R23 self-check (bench-no-trace / bench-trace-scan-catches / bench-test-only-refused).
- Memory notes that matter: QA files end in a 100 KB base64 blob (never read whole); logs grep'd not cat'd; `q run` for everything; `.tmp` not /tmp.

## What consumed my context

Reading whole diffs of large files (sched_oracle.rs +438/+243 lines twice, launch.rs, scripts/q +163) and re-sending oversize results/thread messages (each 2000-byte bounce costs a full resend: ~12 resends this session). Reduce by cutting diffs with `cut -c1-200`/`head`, grepping the oracle for the clause instead of the whole diff, and drafting results under 1800 bytes the first time.
