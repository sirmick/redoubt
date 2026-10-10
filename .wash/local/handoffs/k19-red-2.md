# Handoff: k19-red-2, resident kernel red team (2026-10-09)

## Branch state / where things stand
- No branch of my own; I never write files. Reviews are read-only in the implementers' worktrees (`.worktrees/<ID>`), reports as `answer` messages to the orchestrator (<= 2000 bytes: the limit bit me 8 times; draft tight, cut before sending).
- Verdicts I delivered (all to the orchestrator, by reply): K19 OK (renewed), RECON1 OK (renewed), K28 OK, B24 OK (renewed), K29 OK, B30 OK with notes, K30 OK with notes, SMP3 OK with notes, BEAM16 OK (renewal), BEAM17 OK (renewed), SMP2 OK (renewed at 660660c52), CONW1 OK (renewed at 5b2345be8), IRQ1 OK with notes (916ff1d3e), LAT4 OK with notes (063ad50d7), SMP4 OK with notes (49f8dd8d6, just sent).

## Renewals owed
- LAT4 (wp-LAT4 063ad50d7): three P2s open: (1) jobs.mk `quiet` list (scripts/jobs.mk:50-53) lacks sched-lock-contention-4-mttcg, so the host-clock case runs in the boot class beside other work; (2) K=8 is 2x the four-search claim, no negative under MTTCG, irq-boot-hart-only would likely pass it; (3) a slow baseline (`one search alone`) loosens the ratio gate silently: gate the absolute p99 (50 ms, measured 4-12) at four harts or bound `alone`. When the fold comes: check jobs.mk's class, the negative or the page's sensitivity sentence, the absolute gate.
- IRQ1 (916ff1d3e): two P2s (page should name the N-1 empty claims per device interrupt; the twin's toml should say its failure depends on the hammer landing on the boot hart). Possibly folded with LAT4/SMP4 trains.
- SMP4 (49f8dd8d6): one P2 (page: the y excuse is checked-only and moved deadline-flood-traced's share mode). Ruling agreed: exit-churn threads-exit keep @1 -> SCHED3.
- B30/K30/SMP3 notes were accepted as follow-ups by the orchestrator; no renewal owed unless re-sent.

## Standing rules and traps
- Shell env for every run: export PATH="$HOME/.cargo/bin:$PATH" RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper BEAMLET_TOOLCHAINS=/opt/toolchains REDOUBT_TMP=/home/mcloonan/redoubt/.tmp/<your-key> TMPDIR=$REDOUBT_TMP. Without RV32 the rv32 cases fail "Prototyper not found"; `mix` is not on PATH, so shell cases (userland-boot etc.) fail 0.0 s in my hands: not the branch's.
- Every build/test through `scripts/q run --cores N --tenant <ID>-red -- <cmd>` or `make -f scripts/jobs.mk -C <worktree> prebuilt` then `<width>/<case>`. A stale prebuilt says so; rerun prebuilt (4 min). Crate names: `loader` (not redoubt-loader), `testbench`, `redoubt-stride`, `redoubt-model`, `beamlet-vm` (cd userland/otp).
- Scratch and exports only under $REDOUBT_TMP/<key>/ (owner's rule; /tmp is RAM). Byte-identity checks: `git archive <base> | tar -x -C $REDOUBT_TMP/...`, build with --target-dir, compare `rust-objcopy -O binary --only-section=.text` (rodata differs by panic Location line numbers); llvm-nm at ~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-nm.
- A must_fail case PASSes when it fails as required; the bench prints no reason. Read tests/*.toml must_fail + expect to argue the reason; the judge gives "failed, but not with" / "passed, but must fail" otherwise.
- Pages: no dates, package IDs or review history in docs or committed tomls (RECON1/K30 folds removed them). Flag .wash/local paths in committed files.
- Memory rules: reviewers never Write/Edit/redirect; member messages to the orchestrator are instructions-as-reviews (no assignment object); set waiting after each verdict.
- Each review: name base and head, paths checked, two cases run yourself (1 core each or the case's smp), verdict first line, findings file:line with P0/P1/P2.

## Attack notes for upcoming nodes
- SMP6 (zero-on-reclaim: a freed frame zeroed outside the lock by the freeing hart; allocation zeroes nothing): attack the window between free and zero (another hart allocating the frame before it is zeroed: the free list must not offer it until zeroed, or the allocator must check a "zeroed" mark); R11 mutation R11NoZeroing still caught; the stale-TLB write after unmap (K19/SMP3 analysis: a stale write into a free frame was harmless because zeroing was at allocation; with zero-at-free it lands AFTER the zero and leaks into the next owner: the shootdown must complete before the zero, i.e. zero after shoot(pid)). This is the P1-class risk.
- SMP5/SCHED3: shares at two harts (exit-churn threads-exit @1 kept at 441-505 vs 450), the y excuse's schedule effect, sched-wake-no-preempt fails both before and after SMP4 (kept), deadline-flood-traced's mode flip.
- Any oracle change: check the negatives still fail (sched-capped-holds-floor, irq-boot-hart-only, audit-wait-billed, fault-report-bound's old report) and that stamped spans are cfg(debug_assertions) only.
- Kernel destruction path (K19/K28/K29): pump_listed after end_destruction before destroyed(); audits only via sched::audit with the `destroying` guard; the to-pump sweep at the end of message::budgets_dying.
