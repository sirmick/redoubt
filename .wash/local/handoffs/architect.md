# Architect handoff (architect-15, 2026-10-06)

## Working rules that held

- Messages to the orchestrator and on QA threads: at most 2000 bytes, counted strictly; aim for 1,700. Detail goes in a file under `.wash/local/` and the message names it. Several of mine bounced on length; trim before sending.
- Review-only checks: never Write, Edit or redirect into any file.
- Page edits I make land in the root working tree (main) uncommitted, or as a patch under `.wash/local/` when the root tree must stay clean for merges; the orchestrator commits them as docs after an editor pass. Pages carry no dates, no package IDs, no decision IDs; a planned section has exactly one `**Open:**` line; a built section none; `mutation:` names must exist in `model/src/mutation.rs` (the checker reads them); every `### R<n>` heading needs a SECURITY.md row (C7); links must resolve on the base they are committed to (C6). The checker: `cargo test -p redoubt-doccheck --test docs -- --skip the_book_builds` (mdbook is absent on this host: `the_book_builds` fails for that alone; the dev image has it). Running `cargo testbench docs` through the pool (`jobserver share`) hung 30 minutes once; the direct test takes a second.
- `plan_set` cannot edit an active node (the whole call is rejected); hand the orchestrator the body text instead. A QA thread needs an existing plan node; a `decision_request` needs an existing thread; decision_refs go on with a guarded action (`assign` with `expected_revision`), not `reply`.
- `member_update` with `assignment_results` is at most 2000 bytes too, and an assignment resolves once: later detail goes by message.
- A member key that handed off is inactive: an atomic multi-message send to it fails whole.

## Rulings in force, and where

### Scheduling (SCHED1, RECON1, LIFT1)
- `.wash/local/SCHED1-coverage-ruling.md`: the positive-lead category is structural at 1 ms (a weight-1000 waker clears at most one or two spinners); the category became lead > 0 with `ahead` a reported witness; the 10 ms control's non-vacuity needs `ahead >= 8` for 25 wakes.
- `.wash/local/SCHED1-control-ruling.md`: the 10 ms control that cannot take its samples counts as the negative when the failing line is `CLUSTER-FAIL branch=2 result=0` with index >= 1 and N net of audits > 50,000 µs, trace required; the toml's `\[cluster\] FAIL` forbid removed so the ring is dumped.
- `.wash/local/SCHED1-five-cases-ruling.md`, sections in order: (1) the slice is SCHED1's mechanism on trial, no R-rule names its length; only `sched-ties`' guest order was an accident (rebuild as one kernel entry, or oracle only); the share cases judge by ratio of counts (R12's relative claim) with useful work vs 10 ms reported beside; (2) addendum: the per-switch cost is a finding against a page gap (no stated slice-end cost); (3) after the release measurement: cost inherent, the reconcile's; (4) scope and sequence: the fix is its own package, the slice-end billing to the descheduled budget is per Charging (:203, :228), the page sentence for "Charging"; (5) debt-lift: option (c), the page's "at most one round" softened to the oracle-proven arithmetic (full replacement sentence in point 2), `sched-debt-lift` judges S's first wake by no-other-budget-picked-twice, LIFT1 is the follow-up node.
- Owner's decision (IPC3-wake-latency, 4b03db73): keep 1 ms; merge SCHED1 now with the loss a stated residual; RECON1 next.
- `.wash/local/RECON1-brief.md` (node RECON1, needs SCHED1): the cost is `Queue::raise_floor`'s unconditional walks reading each queued budget's 7-word State through checked frame reads (4-9 walks per slice end, ~28 µs per queued budget in release); fix: `raise_floor` only when the floor can rise, `(pass, tie)` cached in the queue's slots and audited against the frame in the checked build; gate under 3 µs per queued budget per slice end, the fixed L=0 term (100-155 µs: traps, two SBI ecalls) excluded; fixture `sched-large-weight` useful work within 0.85 of 10 ms. The attribution section is filled from `reconcile-finding.md`.
- LIFT1 (node, needs SCHED1): a construction that puts a fresh lift of about a round on a small shared parent; may find the one-round claim false.

### Steward (STEWARD2/3/4), `.wash/local/STEWARD2-implementer.md` dated sections
- Q1 (a): sub-budgets an equal share of top per label set (the core's carves); `label_sets[].budget` dropped from the manifest.
- Q2 (a): `keyd []`; the core's keyd-key guard is vacuous on the box (init's and sshd's holds checks are live): a residual.
- Q3: a binding table in the server over fixed slots (6 after Q4: 0 bootfsd, 1 home fsd, 2 labelled fsd, 3 ipd, 4 console, 5 system volume read-only); a slot bound to nothing for a domain class is produced without a call.
- Q5 and Q5' : init appends the steward's own lines after the core's manifest lines: `label "NAME" id=N`, `home "P" handle=fsd:data path=/home/alice`, `vault "P" labels=[7] handle=fsd:alice-secrets`, `net "P" PREFIX:PORTS...`, `console "PRINCIPAL"` (last, at most one, absent = no console session). Write `fsd:` until FSN1.
- The R41 departure: connections carry no revocation scope (`new_connection` has none); accepted for sessions, written as R41's Open item; the mechanism is STEWARD3's checkpoint question (its brief has it).
- The image is streamed 64 pages at a time (INIT5), never held.
- `ended`: consol opcode 18, no fields, a send; the steward sends it on the login's console connection when the session ends; sshd treats it as the session's end; consoled releases the minted connection; page sentences in the brief.
- The restart, corrected: init cannot remake `users` (class is inherited); a restarted steward finding `users` not empty exits; five restarts reboot; residual on steward.md "Failure and restart" plus a new `docs/todo/empty-a-budget.md` (a kernel ABI follow-up, not STEWARD3's). Case 5's session-badge login is host-only.
- Point-1 rulings: `libs/sha256` (keyd's reviewed SHA-256 shared by keyd, init, the core; `sha2` out of `libs/steward`); init refuses a key in two roles across principals.
- Owner's decision (QA steward-console-principal, commit edd953085): the manifest's `console` names the principal; init refuses a name that is no principal.
- Not mine: the orchestrator's handoff instruction names `expect_after` and bootfsd `BUDGET` among STEWARD2's items; I did not rule them. Look in STEWARD2's report and the orchestrator's messages before assuming a ruling exists.
- STEWARD3's brief: `.wash/local/STEWARD3-implementer.md` (plus the R41 design question appended); STEWARD4's: `.wash/local/STEWARD4-implementer.md`. Sessions are M1's (sessions.md).

### Volumes and file servers
- VOL1 merged (main bd6f768f6 and later). `VOL1-absent-vs-refused` resolved: `ClientError::NotFound`, `Error::Rerror(Name)`.
- Owner's decisions (QA pack-writable-volumes, resolved; pages committed 9a3dfcc89): the read-only system volume moves to EROFS (uncompressed subset) served by `erofsd`; littlefs stays for every writable volume; `fsd` is renamed `littlefsd` (FSN1); a writable SSD file system is later. Pages: `docs/servers/erofsd.md`, README.md "Naming" (Open: `fsd` still carries its role's name until FSN1), fsd.md's Purpose paragraph.
- `.wash/local/FSN1-implementer.md` (node FSN1, needs VOL1 and MEM2, both done: launchable): the mechanical rename, three commits; STEWARD2 rebases over it.
- `.wash/local/EROFS1-implementer.md` (node EROFS1, needs VOL1, BOOT1, FSN1): libs/erofs + servers/erofsd on bootfsd's pattern, Redoubt's own writer with erofs-utils 1.9 as a host oracle (on `scripts/setup.sh`'s one prerequisites list; oracle cases skip as the bench's named `Unusable`, which fails unless `--allow-skip`); it carries BOOT1's step 2 (verityd's LRU sized from the EROFS profile, the boot-time target 1.5x rounded up to 10 s gated in `userland-boot`, the before/after table: littlefs 1016.7 | 534.7 s guest; the per-read cost beside BOOT1's 0.85 M / 1.62 M instructions).
- `.wash/local/BOOT1-implementer.md`: now measurement only (its step 1; BOOT1's report found 96.7 % of littlefs reads are root-directory metadata).
- `.wash/local/VOL2-implementer.md` (node VOL2, needs VOL1): the signed root, VOL1's step 2.

### Process and tooling
- Shared-host rule on docs/testbench.md "On a shared host" (commits 7760bb18d, 46cb2f0ca, plus the loopback refinement): verdicts by the clock a case measures with; `bench-ssh-loopback-deadlock` and `bench-ssh-guest` alone; 13 loopback cases class (ii); the probe is preflight.
- B10 done (model thread bound; the keeper's deadline wait).
- Integration trains: committed by the orchestrator as 7dbbb1c70 from `.wash/local/trains-rule.patch` (short gate, trains of three or six hours, no revert on an unpublished train, a conflicting rebase returns to review). The smoke set named: userland-boot, init-boot, bench-net-peer, ipc-outcomes; its ~15 min bound to be measured at the first train (the net cases' 0.5 s records looked like skips).
- BENCH-parallel-verdicts and BENCH-shared-host-classes resolved.

### Hardware direction and SMP
- `docs/beyond/fpga-platform.md` rewritten card-first (commit 6d0ce2090): XC7K480T first, 4 cores scaled by measurement, barrel a gated experiment, SpinalHDL TileLink, rust-vmm/VFIO backend, measurement gates, Open items (ROM-key custody, the in-flight-frame invariant beside R11).
- R78 (fair kernel entry) planned on main (6d0ce2090); its built text for SMP1's docs commit is `.wash/local/R78-built-text.md` (section, diagnostic clause `sched-test-and-set-entry`, SECURITY row, residual bullet). SMP1's brief has dated sections: Q1 (a) libs/stride gains a running set and per-hart runners, the model's set is SMP2's; Q2 the context pointer and current PID/TID into the per-hart block (memory-layout.md page lines); Q4 the `HART_STACKS` window (slot 0 the boot hart's, `MAX_HARTS` 8); the ticket lock note. m2-usable-shell.md "Several harts" steps 3, 5-7 rewritten.

## Open questions I expect next
- SCHED1 red round 6; the IPC3/SCHED1 merge order (SCHED1 first, IPC3 rebases) and the Charging sentence landing; RECON1's launch after SCHED1 (its gate and fixture fixed; its attribution filled).
- SMP1's rebase onto main (R78 planned → built with the same words) and the `--smp N` flag (B7 owns main.rs: ask before adding; per-case `smp = [2]` is step 1's form).
- STEWARD2 points 3-4 reviews (the wire table `libs/wire/tables/steward.md`, the session batch, the sshd box binary); then STEWARD3's R41 mechanism checkpoint.
- FSN1 is launchable now; EROFS1 after FSN1 and BOOT1's merge; BOOT1's step-1 merge after VOL1 (done) in a train.
- K19's brief (after IPC3 merges; `destroy-simplify.md`) and BEAM6's brief (when MEM2 is near: MEM2 is done, so soon) are owed by the Architect; ASID1's line references recheck was never done.
- The trains' first run: measure the smoke set.

## What a successor must not redo
- Do not re-derive the SCHED1 rulings or re-open the slice: the owner decided (b). Do not re-ask the console principal, the writable volumes, or the naming: decided. Do not re-cut STEWARD2/3/4, VOL2, BOOT1, EROFS1, FSN1, RECON1, LIFT1, B10: cut and briefed. Do not rewrite the shared-host or trains rules: committed. Do not restate R78: its built text exists.
- Do not touch `docs/testbench.md`'s root-tree line about trains if it is still there: it is the orchestrator's.

## What consumed my context
Reading whole evidence files and long page sections; the plan tree printed in full by every `plan_set`/`plan_get` (unavoidable); re-sending messages that bounced on length (trim first).
