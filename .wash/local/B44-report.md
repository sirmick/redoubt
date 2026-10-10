# B44 report: the session's budget back to 11,008 pages

Branch wp-B44 (worktree .worktrees/B44) off origin/main 012900114; one commit, b10dcc0e2.

## Measured on main 012900114 (beamlet-footprint)
Scan: rv64 heap 5,459 / cap 11,885, stack 35,880 B; rv32 5,277, stack 28,920 B.
The VM's report at the prompt (bytes held, rounded each to the nearest page; the rows sum to the
VM's own total): instructions 526/526, operands 2,131/2,131, module literals 37/37, literal
table 690/688, module tables (tables, strings, attributes, compile info) 233/204, atoms 95/66,
process heaps collected 57/57 (22 processes), the rest 117/111, ETS and binaries 1/1; accounted
3,887/3,821 (= `footprint total ... pages=`); runtime heap held 4,110/3,939, peak 4,510/4,343;
not accounted 223/118 (= the report's `unaccounted=`).

## The rule
B32's: twice the largest peak plus the 18-page stack, rounded up to 128: 2 x 5,459 + 18 = 10,936
-> 11,008. Heap cap = budget - 19 (as 11,904 -> 11,885): 10,989, 71 pages over twice the peak; an
rv64 peak above 5,494 puts the cap under twice the peak (the step up).

## Changed
- sizes.session 11904 -> 11008: image/manifest.json, tests/data/boot-profile/manifest-unverified.json,
  tests/data/steward/vault-launch.manifest.json.
- tests/data/beamlet-footprint/manifest.json: budget 11008, heap_pages 10989, budget_pages=11008;
  tests/beamlet-footprint.toml: budget_pages=11008.
- docs/kernel/budgets.md: the size, its peaks, the old size's history, the step-up threshold
  5,494; the residual gone.
- docs/userland/beamlet.md: the table re-measured (the Not accounted rows 223/118, consistent with
  held less accounted; they were consistent before too, at 222/117, for the old measurement);
  the budget paragraph without its residual; a session's sixteenth 688 pages, 2,818,048 bytes.
- docs/testbench.md: cap 10,989 of 11,008, 71 pages over twice the peak 5,459; limits 688 pages.
- image/README.md: Alice's 47,624 pages, set for sessions of 11,905, leave each label set 1,794
  pages over two sessions of 11,009.

## What else derives from the session size (checked; not changed)
- Alice's top budget, 47,624 = 2 label sets x 2 sessions x 11,905 + 4: still room for two
  sessions per label set (not three: 3 x 11,009 = 33,027 > 23,812). Left as it is, with the
  README saying so; lowering it to 44,040 (2 x 2 x 11,009 + 4) is an option for the steward red.
- Bob's 32,768 pages: two sessions fit (22,018), three do not (33,027): unchanged behaviour.
- The steward's sub-budgets per label set are carved from the principal's budget at boot, not
  from the session size: unchanged. A session's heap cap on Redoubt is what the steward gives
  (budget less stack); beamlet's limits are a sixteenth of budget_pages, computed in code
  (userland/otp/redoubt/src/lib.rs LIMIT_SHARE): 688 pages now.
- CTX3 (plan body; .wash/local/CTX1-brief.md): no arithmetic on the session size; its cap per
  label set counts contexts, and the number of contexts that fit Alice's or Bob's budget is the
  two above.
- HOME1: its plan body names no session size.
- userland/shell/test/redoubt/shell/resources_test.exs uses 11_904 in a self-contained @usage
  fixture, not a manifest copy: left as it is.

## Gates on b10dcc0e2
make prebuilt rc=0. set of 45 cases (every steward-*, sshd-*, userland-* case, init-host-tests,
and the worktree's ./scripts/shell-cases origin/main, which picks beamlet's set of 30 since two
steward/boot-profile manifests changed), rv64 and rv32 where they run: 80 PASS, 0 FAIL, set rc=0
(the full list in .worktrees/B44/.tmp/cases). docs PASS rc=0. beamlet-footprint: rv64 heap 5,459
of 10,989 pages, stack 35,880 B; rv32 5,277 of 10,989, stack 28,920 B.
Not pushed.
