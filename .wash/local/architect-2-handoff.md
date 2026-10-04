# Architect handoff (architect-2, 2026-10-01)

Read the first handoff too (`.wash/local/architect-handoff.md`): its rulings table, briefs and
fragile pages still hold, except where this page says otherwise. Same working rules: pages on
main, staged by path, `./dev.sh bash -c 'cd /work && cargo run -q -p redoubt-doccheck'` before
every commit, no thread names or dates on pages, QA bodies at most 2000 bytes. QA `reply` takes
no `decision_refs`; put the commit hash in the body.

## Rulings and commits today (architect-2)

| Ref | What |
| --- | --- |
| 41b459ab1 | `docs/todo/wall-clock-flakes.md`: fourth case, process-review's `zero_entries` window (entry-0 threads run if a tick lands before `process_exit(43)`); title now "Four cases that race the host's clock". B4 builds it and deletes the page. |
| 908fb83c4 | QA INIT1-own-budget, ruled A from settled rules (resolved): testbench.md "Starting a case's programs": every program the tester starts gets slot 1 boot endpoint, slot 2 log endpoint (badged with its place), slot 3 its own budget, slot 4 on the budgets its `programs` line names. Under `init`, R33 stands: no server, a servers' case's programs included, holds a budget handle. |
| 9d10b2d85 | `docs/todo/parked-write-clock.md`: libs/rt `parked_write.rs` failed 14/20 in a loaded loop; not characterised (capture the panic first); two candidate windows named. Outlives B4. |

Earlier refs (first Architect): cc6b697f2, fba3bc92e (owner), dd862a390 (owner), 4a24c6f8b,
a9eed8300, 39ccb59a0, 378a67877, 8074ecf59, 4ace1722d, bad3803c6, 9f0b47f38.

## Open design state per package

- **INIT1** (active, on wp-init1; branch tip seen 6cb07ee56, WIP D3).
  - D3: the tester's slot layout is now 908fb83c4. The branch's `log-server.rs` header still says
    named budgets "from slot 3 on"; it must say slot 4, and the 8 programs that read their own
    charges through `usage(system)` (e.g. budget-table-attack's "system paid 63 pages") read
    slot 3 instead. Kernel lines that print a PID need patterns in `expect`.
  - D3: a program may destroy its own budget; the tester treats the `killed` notice as any exit.
  - D4: `bench-bundle-file` becomes a read-back.
  - At merge: devices.md stale lines (`map_device` ~58, `device_info`'s "once it is built"
    ~86-87); boot.md built sections vs planned; budgets.md "Root, system and users" (built)
    vs "The tree from the boot manifest" (planned) reconcile in the commit that builds the split.
    Model: `device_info`'s arm in `model/src/kernel.rs` rebases onto IPC2.
- **INIT2** (todo). Must state how `init` checks its own usage against `INIT_PAGES` (1,024) and
  refuses the boot; owns the `/boot` half of data entries, the manifest, consoled's `[con N]`
  prefixes, bucket counts, `./mkimage`. R33 unchanged: servers get no budget handle. Nothing
  ruled on the init-usage check yet; it is an open item for its brief.
- **K13** (was parked behind IPC2, which has now merged: 4d42be45f; it may start). Brief `.wash/local/K13-implementer.md`. Open: the phase-record
  instrument for remeasuring `process_ending` is not in the tree; if asked, rule test-only phase
  records under `sched-trace`, like K15's audit records, never in a default build.
- **K15** merged (a678d8928; latency targets judged net of audits; its todo page is gone). The
  first handoff's K15 section is history.
- **GATE1** (K15 has merged, so it can resume, unchanged). Acceptance: the full case on both widths with
  targets met net of audits, the 16-seed sweep and pinned seed, status lines, whole bench, review.
  Evidence in `.wash/local/GATE1-notice-late-architect.md` and siblings. INIT1 changes how its
  program starts (it is the first program in `init`'s place); its hostile agents are untouched by
  the slot layout (the steward stand-in carves their leases).
- **B4** (active). Closes wall-clock-flakes.md's four cases (uaf-lent-page, touch-beyond-ram,
  rt `parked.rs`, process-review `zero_entries`) and deletes the page at merge; the page and
  plan node agree (plan.toml B4 body). `parked-write-clock.md` is NOT B4's and must survive its
  merge; check SUMMARY.md keeps its line when B4 removes the flakes line.
- **IPC2** merged (4d42be45f). ipc.md's residual for the R2 single cursor should now be gone;
  check it was removed in the merge.

## Pages stale or fragile

- scheduling.md Responsiveness: sweep tables read as history; cleanup candidate (first handoff).
- devices.md, boot.md, budgets.md: as above, at INIT1's merge.
- ipc.md Residual risks: the R2 single cursor should have left with IPC2's merge (check); the
  ending process's pump stays until K13.
- steward.md R37: long, owner-approved residual.
- testbench.md "Starting a case's programs": planned; now has the slot list; must go to built
  with INIT1 D3, with its tested line. The "Gifts" paragraph (TAKE_GIFTS) goes with it.
- SUMMARY.md todo list: B4 removes one line, leaves `parked-write-clock.md`.
