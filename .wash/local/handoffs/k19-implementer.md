# K19 implementer handoff (final: stopped past 75% context)

## Next step for the successor

1. **Rebase onto main.** `git rebase --signoff fa08fe2c8` (SMP1 and BEAM6 are in). Use the
   recount recipe below for the ceilings, and name the hunks you resolve in the report.
2. **Rerun the gates:**
   - per commit: `git rebase -q fa08fe2c8 --exec /tmp/k19-check.sh`, which runs the four kernel
     builds, ipclist tests, docs checker, size-budget and fmt, and logs to /tmp/k19-check.log;
   - head: the 21 cases below on both widths, plus `rv64/smp-evict`;
   - worst-walk on both widths, detached;
   - the full model suite: `jobserver bounded cargo test -q -p redoubt-model --release`, about
     45 min.
3. **Report** the head and results to the orchestrator; the red, editor and simplifier renew on
   it.

## Branch state

- Worktree /home/mcloonan/redoubt/.worktrees/K19, branch wp-K19. NEVER pushed; implementers never
  push.
- Current head 46ac921d8, rebased with `--signoff` onto 14aceaa63 (SMP1). Four commits, worktree
  clean:
  1. 8dff8d0b8 model: a destruction delivers nothing until its end (trace contract +
     R10DeliveredMidDestruction)
  2. 8dff0c9d6 kernel: a destruction delivers nothing until its end (Part A)
  3. 4bad6a9b9 kernel: a destruction's kills, frees and PID moves follow the dying subtree
     (Part B)
  4. 46ac921d8 tests, docs: a destruction at full occupancy is within R10's 30 ms
- Report: /home/mcloonan/redoubt/.wash/local/K19-report.md. Brief:
  .wash/local/K19-implementer.md, with the Architect's "Checkpoint 1 ruling" at its end.
- All review rounds are folded in:
  - red: P1 fixed (check_lists exempts a no-thread process, the rollback's child) with the
    budget-table-attack regression, now a checked build;
  - editor: 6 points;
  - simplifier: 6 P2s.

## Verified on 46ac921d8 (on 14aceaa63)

- Each commit on its own: the four kernel builds (release and checked, rv64 and rv32, no
  warnings), ipclist tests, docs checker, size-budget and `cargo +nightly fmt --all -- --check`,
  all 0.
- 43/43 case runs rc 0:
  - both widths: endpoint-destroy-full, endpoint-destroy-open-calls, budget-destroy-kills,
    ending-pumps-once, destroy-keeps-notices, destroy-keeps-notices-creator, process-lifecycle,
    redoubt-dead, sched-destroy-billing, pid-pinning-attack, handle-chain-attack,
    handle-chain-fault, process-chain-fault, budget-deadline, timeouts, userland-boot,
    init-boot, bench-net-peer, ipc-outcomes, budget-table-attack, smp-boot (smp 2);
  - and rv64 smp-evict.
- worst-walk and the model suite on 46ac921d8 were started in my session and had not finished
  when I stopped; treat them as not run.

## Results on the pre-rebase head (a9f54ffcb base)

- 52/52 cases passed on both widths.
- worst-walk as committed passed on both widths, R10 at full occupancy (p50 / p99, threads'
  teardown):
  - rv64: 16,227 / 16,236 µs, teardown 12,995;
  - rv32: 17,244 / 17,281 µs, teardown 13,709;
  - was 53.5 / 58.3 ms.

## Rebase recipe (as done onto 14aceaa63)

- Write a todo that picks the four commits with `exec /tmp/k19-size.sh` after Part A and after
  Part B, and run it with `GIT_SEQUENCE_EDITOR="cp <todo>" git rebase -i --signoff <base>`.
- On a tests/size-budget.toml conflict, keep main's side; the script raises each crate to its
  count and amends.
- Ceilings on 14aceaa63:
  - Part A: kernel 9190 -> 9198, ipclist 499 -> 515;
  - Part B: kernel -> 9290, ipclist -> 531;
  - model 10244.
- docs/kernel/scheduling.md: keep main's residual bullets, drop only "A destruction walks every
  process".

## Design (accepted; do not reopen)

- **Part A.** ipclist `List::pumps()`: K_PUMP = kernel words 2,3, K_TIMED 4, E_PPREV/E_PNEXT
  endpoint list words 12,13 (ENDPOINT_WORDS 14), Member::Endpoint.
  `message::pump_endpoint(ss, mm, e)` lists while `objects.deferring`, else pumps; its sites are
  fail_wait, process_ending and settle_notice. The owner walk unlists dying endpoints.
  `pump_listed` drains after destroy_marked and end_destruction, before the bill. `pump` asserts
  it never runs while deferring. The five dying/doomed checks and process_is_doomed are gone, and
  the refutation is a paragraph on budgets.md under R10.
- **Part B.** Charged chain at process list words 2,3 (CHARGED_WORD 107) and counted chain at
  words 4,5 (COUNTED_WORD 108); PROCESS_WORDS 6; one `List::processes` constructor. Step 2 walks
  the counted chains, root alone walking live_pids. Step 3 is process::budgets_dying(ss, top).
  Step 8 is migrate_held_pids(top). runs_in_dying and find_process are gone.

## Traps

- **Environment:** `. /tmp/k19env.sh` before every shell. Recreate it if /tmp was cleared:
  - `eval "$(/home/mcloonan/redoubt/.wash/local/jobserver env)"`;
  - `export PATH=$HOME/.cargo/bin:/home/mcloonan/redoubt/.wash/local:$PATH`;
  - RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper;
  - RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper;
  - BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains;
  - `cd /home/mcloonan/redoubt/.worktrees/K19`.
- **Helper scripts in /tmp** (recreate them if missing):
  - k19-size.sh: run size-budget, raise each over-ceiling crate in tests/size-budget.toml to the
    count it reports, repeat, then `git add` and `commit --amend --no-edit`;
  - k19-check.sh: the four builds, ipclist tests, docs, size-budget, fmt.
- **Cases:** `make -k -f /home/mcloonan/redoubt/.wash/local/jobs.mk -C <worktree> rv64/<case>
  rv32/<case> RUSTSBI_PROTOTYPER=... RUSTSBI_PROTOTYPER_RV32=... BEAMLET_TOOLCHAINS=...`. A name
  filters by substring.
- **worst-walk:** detached via setsid and nohup. Numbers come from
  `grep -a -o 'R10 2 destructions[^;]*;[^;]*' target/jobs/rv{64,32}-worst-walk.log`.
- **formatting case:** fails in the pool environment (no nightly rustfmt); direct `cargo +nightly
  fmt` is clean, so report it as not run.
- **Rules:** never git add -A or stash; stage by path. A size raise needs a `Size budget:` line in
  the commit that raises it.
