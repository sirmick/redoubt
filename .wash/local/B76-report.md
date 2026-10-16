# B76 report (b76-implementer, 2026-10-15)

Branch `wp-B76` from main c1eb9aa3c, head 306dc5705, clean. Worktree
`/home/mcloonan/redoubt/.worktrees/B76`. Two commits: the bench step (d88f23f91) and the fix
(306dc5705). Files: tools/testbench/src/main.rs, scripts/jobs.mk, docs/testbench.md,
tests/littlefsd-programs/src/bin/aio-reader.rs. libs/client and tests/aio-many-reads-two.toml
unchanged.

## The wait, named: there is none

The failing transcript (`.tmp/B76/before-rv64/fail-1/rv64-aio-many-reads-two.smp1.log`, kept
by step 0) has every expected line:

```
42: [con 358d44d61af08691] aio-reader burst: 64 of 64 reads answered on 2 connection(s); the second program read during the burst; 1 thread, 2 waiters
43: [con 358d44d61af08691] aio-reader TEST PASSED
44: [con ab79fc403c9ccc3f] aio-reader second: read answered during the burst
```

The passing rv32 transcript (`.tmp/train-39/aio-alone/aio-many-reads-two-rv32-smp1.log`) has
the same three lines with the `second:` line first. The case's `expect` list is ordered:
`second: read answered…`, then `burst: 64 of 64…`, then `TEST PASSED`
(tests/aio-many-reads-two.toml:17-19). The bench matches `expect` in order, so in the failing
run it consumed both burst lines while waiting for the second's, matched the second's at line
44, and then waited 90 s for a burst line that had already gone by.

Why the order flips: `second()` (tests/littlefsd-programs/src/bin/aio-reader.rs) sends
`READ_OK` to the burst *before* it returns its verdict, which `run` prints afterwards. On
`READ_OK` the burst takes the completions its waiters already handed over, prints its two lines
and parks. The second's print is one `Twrite` call to consoled, after a send that woke the
burst; which of the two reaches consoled first is scheduling. The hub (libs/client/src/aio.rs)
never waited: all 64 reads were answered, both waiters handed over, the burst's line says so.
Not the waiter-to-reader hand-over, not a wake-up send not taken, not an empty completion call,
not the second program's read parked behind the burst: a console ordering race in the case's
own programs, and a 90 s deadline spent on a line already printed. B51's and B62's rules are
not touched.

Why rv64 and not rv32 (and why one run in three in a bench against about one in ten alone):
not established beyond "scheduling": rv32's slower second program loses the race to the console
every time; the fix removes the race rather than explaining the odds.

## The fix (the case's ordering)

`second()` says its verdict on its own console first (a `Twrite` call to consoled, which serves
9P on one thread and writes lines in the order its calls arrive) and only then sends `READ_OK`.
The burst cannot learn the result, so cannot print, before the second's line is on the console:
the `expect` order is now the only order. `run` prints nothing more for a second that
succeeded; a failing second still prints `TEST FAILED` through `run`. The module doc says so.
No hub change, so no consumer sweep: `redoubt_client` is unchanged.

## Step 1: failure rate before the fix (the case alone, `make -f scripts/jobs.mk <w>/aio-many-reads-two`, 50 runs each width)

- rv32: 50/50 PASS, 0.7 s each (`.tmp/B76/before-rv32/results.txt`).
- rv64: 48/50 PASS at 0.8 s; runs 1 and 12 FAIL at 90.0 s, "timed out waiting for /…burst: 64
  of 64…/" (`.tmp/B76/before-rv64/results.txt`; both transcripts under `fail-1/` and `fail-12/`,
  both with the burst's two lines at 42-43 and the second's at 44). About one in 25 alone here
  against one in three in train 39's bench: the console race favours the burst more under load.

After the fix (306dc5705), the same loop: rv64 50/50 PASS (0.7-0.8 s), rv32 50/50 PASS
(`.tmp/B76/after-rv64/results.txt`, `.tmp/B76/after-rv32/results.txt`).

## Step 0: the bench keeps a failing boot's logs (d88f23f91)

`cargo testbench --keep-failed DIR` copies a failing boot's console log, its second boot's and
its sessions' logs into DIR as `<arch>-<case>.smp<N>…`; `scripts/jobs.mk` passes `target/jobs`,
so they sit beside the job's `<arch>-<case>.log`. Host test
`testbench::tests::a_failing_boots_logs_are_kept_under_its_jobs_name` (a run directory with a
disk image, another width's and another case's logs: only the failing boot's three logs are
kept, under the job's name). The first failing run of this package was kept by it
(`.tmp/B76/before-rv64/fail-1/rv64-aio-many-reads-two.smp1.log`).

## Gates (all through `scripts/q`; logs under `/home/mcloonan/redoubt/.tmp/B76/`)

- 50 runs alone after the fix, both widths: rv64 50/50, rv32 50/50 (`after-rv64/`, `after-rv32/`,
  `loop.sh` is the loop: `make -k -j1 -f scripts/jobs.mk <w>/aio-many-reads-two` each time).
- `make -f scripts/jobs.mk prebuilt`: rv64 324 cases, rv32 312 cases, 0 failed (`gate-prebuilt.log`).
- `make -k -f scripts/jobs.mk set CASES="aio-many-reads bench-net-peer"`: PASS aio-many-reads
  rv64 0.8 s, rv32 0.7 s; PASS bench-net-peer rv64 0.4 s, rv32 0.4 s (`gate-cases.log`).
- `cargo testbench --exact client-host-tests`: PASS 16.4 s (`gate-client-host-tests.log`).
- `cargo testbench --exact docs`: PASS 4.2 s (`gate-docs.log`).
- `cargo testbench --exact formatting`: PASS 19.3 s (`gate-fmt.log`).
- `cargo test -p testbench`: 212 passed, the new host test among them (step 0).
- Not run: size-budget (no native crate grew: the only program change is in a test program's
  ordering, and the bench is host code); the whole bench (the orchestrator's train).

Nothing unsafe added; rv32 compiles (the prebuilt builds both widths).

## Summaries checked

- docs/testbench.md: the run-directory paragraph and the `jobs.mk` paragraph say `--keep-failed`;
  the status line names the host test (changed).
- docs/userland/native.md "Many requests at once": status and the hub's description unchanged;
  the case's behaviour on the machine is what it was. No change needed.
- tests/aio-many-reads-two.toml: description and `expect` unchanged; the order they state is now
  the order the programs guarantee.
- README.md, GETTING-STARTED.md, docs/README.md, the M2 progress: nothing names this case, the
  hub's status or the job logs. No change needed (grep `aio-many-reads`, `keep-failed`, `target/jobs`).
- libs/client/README (if any) and the hub's module doc: the hub is unchanged.
