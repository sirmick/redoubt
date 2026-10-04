INIT1 handoff (init1-implementer-2). Full detail: /home/mcloonan/redoubt/.wash/local/INIT1-progress.md, section "Handoff from init1-implementer-2". Branch wp-init1 on main 4ace1722d, clean tree.

Commits, oldest first:
- b529e547f D1 device_info; 939ba3f77 D1 case.
- 5ac9f1851 WIP model work: keep it LAST and reword at the fold.
- f73d9ef42 WIP D2. The loader loads the kernel and PID 2 only. The bundle is mapped read-only at 0x2000_0000, owned by PID 2; a0/a1 travel in InitialProcess and ProcessState::Setup. IniE, grants, boot_endpoint and boot_log_endpoint are gone. root keeps init's frames + 1 thread + INIT_PAGES=1024; system gets rest/4 and 15 processes, users the rest and 47. The pages are moved. bundle-mapped passes on both widths. The loader-rejects cases pack the corrupt image alone. The loader size ceiling is now 868, and the reason must go in the folded message.
- 6cb07ee56 WIP D3 in progress. log-server is the tester: it reads `programs` and starts each program through the stub in an equal share of system, weight 1,000. Handles: [boot, log badge=place, named budgets from slot 3 (rd::GIVEN)]. TAKE_GIFTS is gone; case.rs has `budgets` and Bin{bin,budgets}; the builder writes `programs`. rng passes on both widths.

Next:
- Answer pending on the own-budget slot (I proposed slot 3 = own budget, named budgets from slot 4).
- Move the 17 single-program cases behind the tester (orchestrator: option A plus conditions; the table is in the file). Diagnose `process`. Rebase onto a678d8928 before sched-latency.
- Fix the 8 attack cases' numbers and comments.
- Add the hostile `programs` refusal cases.
- D4: the bench-bundle-file read-back.
- Gates, then fold commits so the bench stays green.

Traps: K14's conflict in setup_loader_process is mechanical; launch needs redoubt_rt, so it lives in the log-server bin only; PIDs are random but 2; the bench prints only at the end. A `cargo testbench budget` run was in progress: /tmp/init1-d3-budget.log.
