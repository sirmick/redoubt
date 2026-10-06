# B11 report (2026-10-06)

Branch wp-B11, head c03a93549 on main d1872a5cd, one commit:
`testbench: the memory scan waits for every server's heap record`. Not pushed.

## Delivered (the Architect's second ruling, .wash/local/B11-ruling.md: (D) as a readiness condition)

- `tools/testbench/src/memory.rs`: `Measurement.missing` (servers whose record the dump lacks);
  `until_started(deadline, dump, run_on)`: dump and scan; while a record is missing and the
  deadline has not passed, run the guest on for min(1 s, time left) and dump again. The verdict is
  the last dump's. A scan error (duplicate, out-of-range, unreadable) ends the wait at once.
  A record still missing at the deadline fails as before ("NAME: no heap record found"). With more
  than one dump the first line is `memory: dumped twice|N times, waited S s for NAMES`
  (S = guest run-on time, not dump time).
- `tools/testbench/src/qemu.rs`: `measure_stacks` takes the console and the case deadline; between
  dumps QMP `cont`, console read with `forbid` (a forbidden line or guest exit fails the case),
  QMP `stop`. The dump file is overwritten each time; kept only on a scan error, as before.
- `docs/testbench.md` "The memory budget", first paragraph: the ruled sentence verbatim, plus one
  sentence naming the wait line. `tests/memory-host-tests.toml` description updated.
- init-boot's case unchanged (stop line stays, per the ruling).

## Host tests (new, memory.rs)

- `scanner_waits_for_every_server_to_start`: first dump lacks beamlet's record (stack 904 B touched,
  the stub's), second has it: one run-on, no failures, lines from the second dump, wait line.
- `scanner_fails_a_server_not_started_by_the_deadline`: runs on until a 20 ms deadline, then fails;
  past the deadline already: one dump, no run-on.
- `scanner_stops_waiting_at_an_error`: a duplicate record in the second dump errors, no third dump.

## Gates (worktree, main's 10 ms slice unless noted)

- `jobserver bounded cargo test -p testbench`: 103 passed, rc 0.
- jobs.mk: rv64/memory-host-tests rc 0; docs rc 0; rv64/formatting rc 0; rv64/no-cruft rc 0;
  rv64/size-budget rc 0; rv64/unsafe-budget rc 0 (no unsafe added).
- init-boot: 10/10 PASS rv32 and 10/10 PASS rv64, run beside the four userland cases (no wait
  needed at 10 ms).
- userland-boot rv64/rv32 PASS (101.6 s / 103.7 s); userland-read-only rv64/rv32 PASS (114.0 s /
  95.0 s); no wait needed.
- Wait path on the box: kernel/src/sched.rs SLICE_US set to 1_000 temporarily (SCHED1's slice; reverted,
  never committed): rv64 init-boot PASS with no wait; rv32 init-boot PASS 4/4 with
  `memory: dumped twice, waited 1.0 s for beamlet` (SCHED1's own worktree fails it today).
- Not run: whole bench, alone-class cases (host-tests).

## Documentation check

- docs/testbench.md "The memory budget": updated (ruled sentence). Second paragraph ("fails a
  missing or duplicated record") still true: missing at the deadline.
- docs/testbench.md case-field table line 154 (`memory = false # ... painted stacks`): unchanged,
  the field's meaning did not change.
- docs/servers/init.md Stacks/Heaps: no change; they cite the measurement, not when it is taken.
- docs/todo/qmp-socket-private-dir.md: no change (socket handling untouched).
- README.md, GETTING-STARTED.md: no mention of the memory scan.

## Open risks

- Each extra dump of a 1 GiB guest costs a pmemsave and a scan (seconds on a loaded host); only on
  the slow path, and inside the case's deadline.
- Peaks measured after a wait are later than the stop line (the page now says so); the table's
  numbers were not re-measured (not this package's).
