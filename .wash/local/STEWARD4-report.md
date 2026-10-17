# STEWARD4 report (implementer)

Branch `wp-STEWARD4` = 3497d27ca, one `steward:` commit on main 68c4b2904 (train 48), 65 files;
the commit message (.wash/local/steward4-commit.txt) is the account. Brief: `STEWARD4-implementer-2026-10-16.md`; rulings:
`STEWARD4-ruling.md` (Q1-Q7, Q9), thread `STEWARD4-crossing` (Q8 second disk; Q10 the confined
check and an exempt steward's handed endpoint, Q11 leases against the admission caps: both open).

## Delivered so far

- `servers/crossing` (new, one program, mode by argument; `libs/wire/tables/crossing.md`,
  opcodes 1-2, errors `too_big`, `failed`, `served`); host tests (4).
- The steward: `Kernel::cross/home_stat/home_read/home_write`, items by id beside the core's
  ids, the drive's `Read`/`Write` steps, the crossing budget stamped with the labelled side's
  account, crossings and requests forgotten by the store's state (the tables emit no `forget`
  for either), the screen's path lines, `submit` kinds 2 and 3 (`what` = path; a push's
  `source\ntarget`), `in_use`/`too_big`/`unknown` before the core, `PUSH_MAX` 61,440 bytes,
  `~/out/<name>`, the `confined` own line and slot 1 unbound for a labelled session, the
  `served <op> in N us` lines, the crossing's exit line; host tests (41, 7 new).
- The steward's `heap_pages` 32 -> 128 in the image's manifest and the six steward manifests
  under tests/data: the runtime heap is a cap, and a push's `try_reserve(PUSH_MAX + 1)` failed
  it on rv64 (`a step failed (OutOfMemory): Read`); `PENDING_CAP` (4) pushes' snapshots of
  fifteen pages each beside a read's sixteen never fit 32. steward.md says so beside `PUSH_MAX`.
- init: the volume rule counts the principal's unlabelled domain alone; the `confined` line;
  `fuzz::ENTRIES` gains `crossing`; host tests (68, 2 new).
- The shell: one line for a labelled session told no home; its ExUnit case extended.
- The oracle: the steward's console prefix before `LATENCY-SAMPLE`/`LATENCY-COUNT` (test); Q9:
  `LATENCY-END <time>` ends the window: no destruction ending after it and no sample printed
  after it is judged, their count reported (`destructions_after_the_marker_are_recorded_not_judged`).
- `latency-probe` (feature, off by default) in the steward's binary, printing `LATENCY-END`
  with its clock after the third round's counts; `steward-latency` case and manifest; the bench
  host test `only_steward_latency_builds_the_latency_probe`.
- Case tomls: the submitting session's notice and reply in either order (one `(?s)` pattern);
  the crossing's ending `exited 0|was killed N` (the program exits after its one call, or the
  destruction kills it first); the push's source staged before the vault session's push names
  it (the steward stats the source before the core: `unknown` otherwise); the screen's two item
  lines in either order (ids are random); the latency case's end-of-run lines moved from
  `expect` (which the sessions wait out) to `expect_after`, its manifest entry given
  `verity = "image/userland.toml"` (without it `erofsd:system` serves nothing and the console
  login's slot-5 connect is refused).
- Pages: steward.md (the section rewritten, R42, the protocol, the manifest lines, the console
  line with both endings, the two probes incl. `LATENCY-END`, the heap sentence), init.md (the
  confinement check, R34, the own-lines row, a residual), SECURITY.md (R42, R34), sessions.md,
  shell.md, servers/README.md, walfsd.md, littlefsd.md, testbench.md (bootfsd's row, the
  `LATENCY-END` sentence), plan/m3-agents.md.
- tests/data/steward/confined.manifest.json (B79's disk2 names: `blkd:vault` labelled on disk2,
  the vault volume on it, alice's `net` removed); the case toml waits for the rebase onto B79.

## Deviations from the brief, with reasons

- The crossing's connection is rooted at the vault's root, not the item's directory: a mint
  below the root walks the path with the caller's labels (R25, `ninep.rs:855-866`), which the
  unlabelled steward has not; the labelled program walks to the item.
- `PUSH_MAX` is 61,440 bytes, not 64 KiB: the item's path and the message's lengths share the
  steward's sixteen-page lend with the bytes.
- A push's `what` carries `source\ntarget`: the wire's `source` is a `u64`.
- The crossing budget is 128 pages (`sizes.crossing`, image and the steward manifests), not 64:
  the receive maps the steward's sixteen-page lend into the program (R3) beside the stub, the
  image, its stack and heap; the relay has 128 for the same reason.
- Case 3 (`steward-confined-push`) is host-tested only until the rebase onto B79 (Q8), and its
  manifest needs Q10: init's check counts an exempt steward handed `walfsd:alice-secrets` as an
  unlabelled user of a labelled endpoint.

## Gates (2026-10-17; every command through `scripts/q run` / `scripts/jobs.mk`)

| Gate | Command | Result |
| --- | --- | --- |
| steward-declassify rv64 / rv32 | `make -f scripts/jobs.mk set CASES="steward-declassify"` | PASS 11.1 s / 12.4 s |
| steward-push rv64 / rv32 | `make -f scripts/jobs.mk set CASES="steward-push"` | PASS 20.2 s / 18.9 s |
| steward-confined-push rv64 / rv32 | `make -f scripts/jobs.mk set CASES="steward-confined-push"` | PASS 16.4 s / 14.6 s (B79's `[vault]`) |
| the steward set (33 cases, both widths, latency aside) | `make -f scripts/jobs.mk -k set CASES="..."` (list in target/jobs/steward-set.list) | 64/64 PASS: 46 first, the nine refused cases 17/18 after their programs lists gained `redoubt-crossing`, steward-server-restart rv32 PASS alone (its one fail: bob's lockout login succeeded beside nine running cases) |
| steward-latency rv64 one hart | `make -f scripts/jobs.mk rv64/steward-latency` | the full run: all counts 16, LATENCY-END, 6 exits, reboot, trace 2,705,994 records dropped 0, ~6 min wall; the verdict failed on the counts' order in expect_after (fixed); the oracle's figures from the one-hart rerun pending |
| steward-latency rv64 two harts | same | FAIL early in round 1: `Launch` of a lease `NotPermitted` after the kernel killed the previous deadline lease's VM on the other hart (kernel/src/process.rs `p.started() or !p.alive()`): asked the Architect whether the case gates at one hart |
| steward-latency rv32, the sweep | | not yet run (the shape settled first) |
| host: steward server 47, init 69, crossing 4, testbench case:: and sched_oracle 63 | `cargo test -p ...` through q | ok |
| elixir-oracles | `make -f scripts/jobs.mk rv64/elixir-oracles` | PASS 17.6 s |
| shell ExUnit | `q run -- bash -c 'cd userland/shell && mix test'` (toolchains on PATH) | 314 passed, 70 skipped |
| docs | `make -f scripts/jobs.mk docs` | PASS (after the register's R34 row was aligned) |
| unsafe-budget rv64 | `make -f scripts/jobs.mk rv64/unsafe-budget` | PASS |
| size-budget | `make -f scripts/jobs.mk rv64/size-budget` | ceilings set to the measured lines: libs/wire 4153 (raised, the generated crossing codec), servers/steward 3082 (2527 on main + the package), servers/crossing 185, servers/init 2785; to re-run on the final tree |
| fmt | `q run -- cargo +nightly fmt --check` | clean |

Steward work measured (the `served` lines): `submit` at most 96 ms (declassify rv64), 100 ms (push rv64), 95 ms (rv32); `end_lease` of a VM lease at most 589 ms (rv64, one hart, checked build under icount); on steward.md's residual.

## Since the first checkpoint (2026-10-16 ~23:50Z)

- Q10 built: `confine.rs` skips an exempt server (steward, sshd) as a user of an endpoint, in
  `users()` and the endpoint rule; init.md carries the ruling's bounding sentence verbatim and
  the host test `confined_does_not_count_an_exempt_server_handed_a_labelled_endpoint` (passes
  with the vault handed to the steward, refused when handed to an unlabelled non-exempt server);
  init's host tests 69.
- Q11 (c) as far as the servers allow: the first attempt with six principals was refused at
  boot three times (home quotas over the data volume's bytes; `contexts.max` 3 over a 32,768-page
  share; `buckets` below the domains declared) and then died at `consoled`'s start: its client
  pool is a constant (`BUDGET` 2 MiB, five buckets at `MAX_THREADS` 255), so nine buckets cannot
  fit and the boot rebooted five restarts later with no console. The shape that fits without
  moving any server constant: four principals (alice unlabelled only, bob, carol, dave; the
  sponsored agent and alice's vault domain leave the latency manifest), which declare the
  image's four domains, so every shared server keeps the image's bucket count. N = 16 is then
  alice 2 + bob 5 + carol 5 + dave 4 (one session and five leases per account is the admission
  to test; N falls to 14 with the reason on the page if the fifth lease is refused). The probe
  groups samples by every principal's live leases. Test keys carol and dave added
  (tests/keys), stage homes for them.
- Rebased twice: onto 69aa50dd3 (B79, clean) and onto 68c4b2904 (3Y): conflicts in
  bin/steward.rs (3Y's `disconnect` beside my `cross_through`; the loop's `due` beside the
  probe's wait and timeout arm, both kept), tests/steward.rs (both fake-kernel fields kept),
  size-budget.toml (2527 + 563 = 3090 for now), m3-agents.md (merged). Host tests after: steward
  47, init 69, crossing 4; fmt clean.
- Case 3 toml written (`steward-confined-push`: programs listed, `[vault]` 8 MiB, the shell's
  line, a push landing); first boots reached the push and the vault's read-back on both widths
  (`:equal`), the verdict waiting only on the `expect_after` order (the crossing's exit line
  prints after `Pushed`), fixed with the push case's.

## Q12 detail: the steward's share at the system volume (2026-10-17 ~00:05Z)

- Six principals: `consoled` refuses nine buckets at its start (`BUDGET` 2 MiB holds five at
  `MAX_THREADS` 255: servers/consoled/src/server.rs:67-72); init restarts it five times within
  the minute and reboots, nothing on the console (consoled is the console). Log:
  `.tmp/steward4-logs/latency-six-principals-smp1.log` (the run dir's, kept by hand).
- Four principals (alice unlabelled, bob, carol, dave; the image's bucket counts): the boot runs,
  alice's console session, her ssh session, dave's and bob's log in; carol's login is refused its
  slot-5 connection: `a step failed (Refused): Connect { token: Token { owner: Object { domain:
  Domain { account: 1004, labels: Labels([]) }, kind: Session, .. }, slot: 5 }, scope: .. slot:
  1 .., server: Shared(1), badge: .. }` (erofsd:system). Log: `target/jobs/rv64-steward-latency.smp1.log`.
- The rule: libs/rt/src/server/admit.rs:13-14 ("the 9P skeleton counts a connection a client
  minted for itself in the share of the connection it minted it from"); serving.md "`admit`":
  "It is admitted as a `State` of the caller's share". erofsd's `limits`: `state: 8`
  (servers/erofsd/src/server.rs:20-22). The steward mints every session's two system-volume
  connections (the shell's and the relay's) and every lease's one from its root connection, so
  they are States of the steward's share there: 2 x 2 + 3 + 1 = 8 refused the fourth lease in the
  alice-only run, 4 x 2 = 8 refused the fifth session here. walfsd's `in_flight` 6 per account is
  not the binding cap; the steward's share's `state` 8 at erofsd is.
- Consequence beyond the case: on the image, a fifth session or a fourth lease beside two
  sessions is refused the system volume. steward-three-contexts (console + three contexts = 8)
  sits at the cap.

## Q12 ruled (a) and applied (2026-10-17 ~00:30Z)

- steward-latency is alice alone at N = 1, 2, 3 (the probe's or the deadline's lease among the
  N live leases, N - 1 spinners), D = 16 each; the manifest is the image's principals with the
  steward's heap raise; carol's and dave's keys and homes dropped (erin's and mallory's too), no
  part of the six-principal shape remains in the package. The probe's group is the count of live
  leases, itself included. Rounds are closed by a per-round marker file the session writes
  (`/home/alice/latency-round-N`): at N = 1 the leases are gone after every probe lease, so
  "leases all gone" alone counted a round per lease (seen on smp2). serving.md Residual risks
  gains "A root badge's share caps everything minted from it", consoled.md "The pool is a
  constant", both naming the follow-up without an id.
- Steward set both widths: 46 PASS; 18 FAIL from one cause, the nine cases listing their
  programs lacked `redoubt-crossing` while the image's `public` names it (`public[3]: names
  nothing the manifest or the bundle holds`); fixed, rerunning.
- Finding on the first N = 1, 2, 3 run (rv64, smp1, round 3): `steward: disconnect ... at
  erofsd:system dropped after 4 attempts: the server did not take it`, then the next lease's
  slot-5 connect refused: 3Y's deferred cleanup timed out four times under two spinners' load,
  dropped the disconnect, and the lease's State leaked at erofsd, so the steward's share hit its
  eighth State at 4 + 2 + 1 + 1. N = 3 sits one leaked State from refusal.

## The leak is ruled STEWARD3W (2026-10-17 ~00:40Z)

The dropped disconnect (`TRIES` 4 / `RETRY_US` 1 s calibrated for a recovering server, failing
for a busy one) is STEWARD3W's: the fourth timeout parks the step, retried when the server
answers; the 100 ms bound stays; nothing of it is built here; 3W starts after this package's
drive.rs lands. The nine refused cases rerun: 17 PASS; steward-server-restart rv32 failed its
`bob-locked-out` login (the login succeeded) while nine cases ran beside it, rerun alone. The
first marker-round latency run exited `RECEIVE_FAILED` at the poll's first timeout (no arm took
a `Timeout` with no timer due); the poll has its own arm, rerunning.

## Q13 and Q14 (2026-10-17 ~01:00Z)

- Q13: the two-hart failure (`Launch` of a lease `NotPermitted` right after the kernel killed a
  sibling lease's VM on the other hart, twice at the same point) is kernel finding B82; the case
  gates at one hart (`smp = [1]`, `gate_harts=1`) with the toml's comment and the page's sentence
  that the two-hart row is recorded when the launch failure is fixed. Logs kept in
  `.wash/local/steward4-logs/` (latency-rv64-smp2-notpermitted.log).
- Q14: the wakes are measured at the page's workload first: the latency manifest's steward takes
  the stand-in's weight, 1000 under `system` (the image's is 100); both weights' figures go to the
  Architect, who raises the image's weight with the owner. The deadline notice and the destroy
  call are a different quantity (a beamlet VM lease of 5,248 pages destroyed): the page states
  the columns as what they are, with the pages destroyed per lease beside.
- The full one-hart run at weight 100 (the first with the marker rounds) ran whole in about six
  minutes; its oracle figures and the weight-1000 run's are the next step, then rv32 and the
  sweep (once, when the case lands).

## Q15 (2026-10-17 ~01:20Z): the probe arms before its receive

- The weight-100 figures (net, N = 1/2/3): decision wake p50/p99 10.7/16.2, 14.8/17.2, 19.3/29.7
  ms met; timer wake 50.3/50.5, 31.7/31.9, 39.5/206 ms missed at every N; deadline notice p99
  32.2 met, 40.8 and 41.1 missed by about a millisecond (gross 371-578 ms: 5-8 s of the checked
  build's audits inside the windows); destroy_call net p99 18.7/21.8/27.2 ms (gross 295-498);
  R10 99 destructions p99 9.7 ms met; lease end 29.7 + 9.7 = 39.4 ms met; 16 destructions after
  LATENCY-END not judged; 2,615,142 records, 0 dropped.
- Q15: the timer was armed right after the core answered, before the reply and the steward's
  audit/usage console lines, so a 20 ms timer read 50 ms of the steward's own loop. Now the probe
  records the reply's time, arms at the last point before the receive that waits on it, and
  prints the gap as a `reply_wait` sample (the oracle's sixth measure, recorded, no target). The
  N = 3 timer p99 of 206 ms is attributed: between the arm and the sample the loop ran a cleanup
  retry whose 100 ms bound timed out (`disconnect ... dropped after 4 attempts`), the loop's own
  work, not a wait; excluded by the new arming.
- What `reply_wait` is made of (servers/steward/src/bin/steward.rs `say`): each console line is
  a fresh one-page `Lend`, then a 9P attach, open, write and close at consoled, four round trips;
  a `start_agent` writes two lines after its reply (the `AgentStarted` audit record and the
  domain's usage line), so the gap is about eight consoled calls and two lend maps, bounded only
  by consoled's turn (weight 1000 under `system`).
- Weight 1000, old arming (net, N = 1/2/3): decision wake p50/p99 4.7/4.8, 4.7/5.6, 5.5/13.6 ms
  met; deadline notice p99 28.8/28.1/32.0 ms met at every N; destroy_call net p99 6.1/8.4/7.6 ms
  (gross 268-447); timer wake p50 48.1/27.3/33.0 ms missed (the loop's post-reply work, as at
  weight 100); R10 p99 9.68 ms; lease end 13.6 + 9.7 = 23.3 ms met; 2,697,957 records, 0 dropped.
  So the weight moves the decision wake, the deadline notice and the destroy call (the steward
  waiting out VM slices at weight 100); the timer wake it does not.
- Q16 (ruled): the loop's console writes are STEWARD3U's (one console fid for the steward's
  life, a line one write, a queue drained at the loop's turn within the 100 ms bound; measured
  by the re-armed probe's `reply_wait`): STEWARD4 reports the gap at weights 100 and 1000 with
  what it is made of and names 3U as the fix here, not on the page. The image's steward weight
  (100 today; the page's workload is 1000) goes to the owner by the Architect's decision_request.
  scheduling.md's table is measured at 1000 with the re-armed probe (the old-arm figures stay in
  this report); if the image keeps 100, one sentence beside the table says so. The 206 ms
  attribution (a cleanup retry's 100 ms bound inside a window) is 3W's evidence under ordinary
  load.
- The leak is deterministic at the pinned seed: two re-armed runs at weight 1000 dropped the
  disconnect of the same connection (16095599158017907573) at erofsd:system in round 3 (about
  the 70th lease, two spinners live), and the next lease's slot-5 connect was refused on the
  eighth State; the old-arm weight-1000 run dropped two and survived by its timing. erofsd is
  busy past four 100 ms bounds a second apart at that point (a spinner VM's code reads of the
  system volume are the likely cause: 3W's evidence). A first sequenced rerun was spoiled by my
  own `sed` on the budget line's text, which also moved consoled (to 100 during the weight-100
  run) and erofsd/sshd (to 1000 on restore): discarded; the manifest restored from the commit and
  the reruns edit the steward's entry by name.
- B82 resolved (b82-implementer, .wash/local/B82-report.md): not a kernel bug. The case's
  kernel-deadline leases were `start_agent(c, 2_000_000)`; a lease's deadline is set at its
  batch's start (carve_lease) while the Launch step takes about 1.25 s of guest time at one hart
  and 2.46 s at two (icount counts both harts), so at two harts lease 17's deadline fell between
  process_create and process_start, R10 destroyed the budget, and map/start answered
  NotPermitted (reproduced at one hart with 100 ms leases). The case's deadline leases are now
  5 s (twice the two-hart launch; the toml's comment carries the arithmetic), `smp = [1, 2]` and
  `gate_harts=2` are back, the two-hart row is recorded. The steward's semantics for a lease
  shorter than its launch is a design question B82's implementer puts to the Architect; nothing
  here waits on it.
- Weight 1000, re-armed, clean manifest, one hart (net, N = 1/2/3): timer wake p50/p99
  23.4/24.1, 25.8/27.3, 26.6/28.4 ms (p99 met, p50 missed at every N: the wake pays for the
  loop's console work on the steward's pass); decision wake 4.1/7.3, 5.9/6.3, 6.4/10.6 ms met;
  deadline notice p99 28.9/28.4/31.6 ms met; destroy_call net p99 7.2/9.5/9.6 ms (gross
  269-453); reply_wait p50/p99 55.9/56.1, 34.3/37.4, 41.1/146.5 ms (3U's quantity); R10 p99
  9.68 ms; lease end 10.6 + 9.7 = 20.3 ms met; 2,589,421 records, 0 dropped; round 3's leak did
  not strike this run.
- The weight-100 clean rerun (it had picked up the five-second leases and both harts) failed
  round 3 on both harts the same way (the same connection's disconnect dropped, the next lease's
  slot-5 connect refused): three of four re-armed runs at seed 3 die in round 3. Q17 asked: the
  case's rounds N = 1, 2 until 3W (recommended), headroom by dropping the console session, or 3W
  first.
- The final-shape run (weight 1000, both harts, five-second leases) failed round 3 on both
  harts the same way (N = 1 and 2 counted out, the dropped disconnect, the refusal): four of
  five re-armed runs at seed 3. Q17 pending.
- That run's gross figures (µs p50/p99, no trace so no net; N = 1 / N = 2), one hart: decision
  wake 10891/19882, 14773/33132; timer wake 44377/47709, 46584/48907; deadline notice
  342719/365013, 434891/469761; destroy_call 248118/267329, 338047/352909; reply_wait
  105841/106067, 87622/104889. Two harts: decision wake 59822/85385, 36263/62290; timer wake
  78038/89192, 62835/86594; deadline notice 384840/399122, 467637/499900; destroy_call
  257369/278479, 350744/367128; reply_wait 167207/171564, 149231/258500: under icount two harts
  count both harts' instructions, so every gross figure grows as the stand-in's two-hart gross
  did; the net needs the trace of a run that reaches its reset.
- Q17 ruled by the orchestrator (within the Architect's words): the case runs N = 1 and 2 as
  its verdict; N = 3 is not run, the table's N = 3 row reads "not reached: a cleanup step dropped
  at a busy server leaks a State and the eighth is refused", 3W's gate regains N = 3. Kept: the
  5 s leases, `smp = [1, 2]`, `gate_harts=2`, the steward at 1000 in the latency manifest. The
  toml and the probe (LATENCY-END after the second round's counts, the exit check at two rounds)
  are cut to two rounds; the confirming rv64 run (both harts) and the rv32 run are in flight.
- Two rounds, rv64, both harts reached the oracle (weight 1000, re-armed, 5 s leases): one hart
  met everything but the timer wake p50 (23.4/25.9 ms); two harts missed the timer wake p50
  (42.7/39.2), the decision wake p50 at N = 1 (37.2 ms) and the deadline notice p99 (49.7/51.3
  ms), reply_wait doubling to 116/94 ms p50 (icount counts both harts): reported to architect-2
  as 3U's quantity. rv32 booted to nothing for 90 minutes: `memory_mib = 2048` at RAM base
  0x8000_0000 wraps the 32-bit address space; now 1536 MiB for both widths (the comment carries
  the arithmetic); rv32 then rv64 rerun on the final toml.
- Final shape, all four boots to the oracle (seed 3; net µs p50/p99 at N = 1 / N = 2); the table
  is on scheduling.md. reply_wait (the reply to the next receive: eight consoled calls and two
  lend maps per start_agent; STEWARD3U's quantity, the fix named here, not on the page): rv64 one
  hart 55934/56179, 34375/37311; rv64 two harts 115672/120059, 95162/113787; rv32 one hart
  34588/34815, 36823/39169; rv32 two harts 157016/160713, 163609/170030. The misses: the timer
  wake's p50 at every width and hart count (the loop's console work on the steward's pass); at
  two harts also the decision wake's p50 (rv64 N = 1; rv32 both N) and the deadline notice's p99
  (both widths): findings against the steward's loop, 3U's. Memory 1536 MiB (2048 wrapped rv32's
  address space). The rv64 sweep (seeds 1..20, both harts, two boots at a time) runs; rv32's
  follows; each is "once, when the case lands".
- Runs queued: the weight-1000 run with the old arming (for the record), then the re-armed probe
  at weight 1000 (the case's) and at weight 100 (the comparison the Architect raises with the
  owner), verdicts and logs under .wash/local/steward4-logs/.

## State (2026-10-16 ~23:55Z, head a2eb892e6 = three WIP commits on 68c4b2904, uncommitted:
the four-principal latency manifest and toml, the two push tomls' order, erin's key and two
homes removed)

Running: steward-latency rv64, steward-push and steward-confined-push both widths. Then: the
latency figures and the first run's length/ring check on the thread, scheduling.md's paragraph
and table, the pages' confined status to built with case 3, size-budget exact ceilings (libs/wire
raised for the generated codec), the steward set both widths, the fold into one `steward:`
commit, the report. Console logs kept in `.tmp/steward4-logs/`.
