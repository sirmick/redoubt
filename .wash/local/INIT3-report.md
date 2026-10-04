# INIT3 report

Branch wp-init3 on main 2ab6dc81c, tip 444644996:
- 0a3afb057 init: restarts and the reboot rule
- f74967aee init-restart
- fb9e60633 init-rollback
- e230bc025 driver cases
- 2906222e4 launcher-orphan
- 57639dd7e consoled fids
- 444644996 init-handed-revoked

Red's fixes, folded into 0a3afb057:
- P1: handed badges are minted stamped with the instance's budget. The new case
  init-handed-revoked (both widths) fails without the fix.
- P2: a failed start() during a restart reboots. init.md "Reboot" says so.

Editor: wire.md sentence narrowed; "hold among them" reworded; 2906222e4's
message says what it adds.

Simplifier. Taken: settle's todo stack, Restarts' len. Left: blame labels (rule 7
prints them, init-restart checks them); placements carrying manifest names (churn
in check.rs to save about 5 lines).

Rule 2 departure: a direct console disconnect, not Grants. Grants hides the id;
Job has no release without wait; a restarted consoled sweeps the parent handle
Grants records.

launcher-orphan: C's connection is minted through L's; C's budget survives L; C is
refused after Job::wait releases L's grants. Server count 2 -> 0.

consoled fids (as ruled): files = 2 x MAX_THREADS; BUDGET 2 MiB; 1024 pages in
every manifest; host test; workaround dropped.

Gates on the tip, all exit 0: nightly fmt --check, doccheck, testbench init (44
PASS on both widths), launcher-orphan, r4-host-tests, size-budget (init 1828,
consoled 343, reason lines in commits), unsafe-budget. Whole bench: held.

Open:
- The bound check's sum leaves out restarts during the boot.
- The steps re-run when keyd, consoled or bootfsd restart are read from the code.
- netd's restart has no case.

Detail: INIT3-report-detail.md
Red re-review notes, folded into 444644996: keeper accepts only BadHandle as
"gone" (Dead retries, other errors fail); the poll has no try bound, only
the case's timeout.
