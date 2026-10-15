# B51 report (b57-implementer-3, 2026-10-15)

Node B51: steward-home-quota's `File.rm("/home/alice/a")` answers `{:error, :eio}` under the
bench's load and the case waits to its 610 s deadline. Worktree
`/home/mcloonan/redoubt/.worktrees/B51`, branch `wp-B51` from main 9d680b781. Phase 1: reading
only, no code changed.

## Design

**The rm's path.** `File.rm` → beamlet's `Files::remove` (userland/otp/redoubt/src/files.rs):
`io.connect` (:560), a walk, `Tremove` through the aio hub (libs/client/src/aio.rs) → walfsd
(`servers/walfsd/src/server.rs:698 remove`) → walfs `ops.rs:201 remove` (a log write and a
sync) → the range client (libs/fileserver/src/range.rs) → blkd. `:eio` is `FileError::Eio`;
every place it can be minted on this path, and what tells them apart:

1. **walfsd's range client, a blkd answer that is not the reply asked for** (range.rs:105-127):
   a `busy` from blkd's serving library (over the connection's share under load), any typed
   error, a malformed reply — all are `Fault`; volume.rs maps `Fault` to `Error::Io`; walfs
   `fs.rs:170` poisons on `Io`; walfsd `with` (:249) drops the filesystem; from then on every
   request answers `corrupt` (`nine`, :505), and files.rs:901 maps `Name::Corrupt` to `Eio`.
   The call itself waits `FOREVER`, so slowness alone cannot do it; a refusal can. Nothing is
   said on the console: `is_corrupt` is printed only at start (bin/walfsd.rs:40). Tell-tale: once
   it has happened, *every* later request on that volume (alice's and bob's) is `corrupt`/eio,
   bob's log included if he does anything after; the console shows no walfsd restart.
2. **files.rs's busy retry (BEAM11)**: `Reply::Busy` → retry after `RETRY_US`; only an op
   abandoned while waiting to retry becomes `Eio` (:227). Tell-tale: the hub's `busy_answers`
   count; later ops succeed.
3. **`Reply::Lost`** (:298): the hub's `Outcome::Ended`/`Flushed` (:707) — the connection to
   walfsd ended with the rm outstanding: walfsd died (then `init: restarted walfsd:data` on the
   console, which the case forbids, so it would fail there, not wait) or the hub ended the
   connection (aio.rs `end`, on a send error other than Timeout/Busy; B47's Io release of an
   ended connection racing a late message). Tell-tale: `init: restarted`, or every later op on
   that connection `Lost`/eio while bob's connection works.
4. **`io.connect` refused** (:560, B48's P3-2): any refusal → `Eio` before a byte is sent
   (B61's `TooManyThreads`). Tell-tale: walfsd sees no `Tremove`; a retried `File.rm` from the
   same prompt fails the same way; bob unaffected.
5. **The 9P client's timeout**: the hub's collect call outlives `COLLECT_WAIT + margin`
   (aio.rs:531) → `[FAILED, 0]` → the connection is ended → 3. Tell-tale: as 3, without a restart.
6. **"Any other answer"** (:366): a `Tremove` answered by a success that is not `Rremove`. No
   known path; excluded unless the trace shows it.
   walfs's own `NoSpace` on a full log is `no_space` → `:enospc`, never eio: excluded.

**Most likely: 1.** Alone, blkd never answers `busy`; beside boots it can, exactly once is
enough, and the volume is then poisoned without a word — the fill's enospc and bob's 5 MiB
passed, the rm's own log write or sync met the refusal. 2 and 4 leave the volume healthy; 3
and 5 end a connection and are visible as `Lost` on everything after.

**How phase 2 tells them apart.** (a) A walfsd console line when the volume poisons, naming the
operation and the fault (`walfsd: volume poisoned: <op>: <reason>`), kept as the fix's own
witness (the page's "Failure and restart" says an I/O error poisons; it should say so aloud);
(b) in the alice session, after `{:rm, ...}`, a `File.stat("/home/alice/b")` and a `File.rm`
again: `:eio` on both = 1 or 3/5; `:ok` = 2 or 4; (c) bob's session does one `File.stat` after
`bob-done`: eio there too = 1 (shared volume); (d) the console's `init: restarted` lines for 3;
(e) blkd's busy answers: its serving library counts them; a line at exit or a bench-read counter.
Reproduce beside two or three boots through q with these in place, both widths.

**Fix, by candidate.** 1: the range client answers `busy` by waiting `RETRY_US` and asking
again (the serving library's own rule, as files.rs does), and only a genuine fault poisons; a
host test with a blkd that answers busy once. 2/4: files.rs maps the refusal to its own
error, not eio. 3/5: the race itself. **The case's wait**: the rm step gets its own deadline
(a step `within`, if the session steps have one; else the smallest change that gives it one),
so a wrong answer fails the step in seconds, not at 610 s; the three `:eio` probes above stay
as the case's diagnosis lines.

## Phase 2a: candidate 1's fix and witness, built without the machine

Head on wp-B51: see the commit `walfsd, fileserver, rt, beamlet, testbench, tests, docs: a call
to blkd the kernel answers Busy is made again ...` (one commit, 17 files).

- **The refusal, named.** The kernel answers `Busy` to a call whose sender's group at the
  endpoint already holds `WAIT_CAP` (32) messages (kernel/ipc.md:313); a `call` can carry it
  (libs/sys error set for `Call`). walfsd's range client took it as a `Fault` → `Error::Io` →
  poisoned. Alone, blkd's group never fills; beside boots it can, once is enough.
- **libs/fileserver `Blkd::call`**: a `Busy` is asked again after `RETRY_US` (10 ms) for up to
  `BUSY_WAIT_US` (2 s: a group of 32 block requests drains in far less); past that it is a
  `Fault` with `Why::Busy`, treated as a dead disk, as before. Every fault notes a `Why`
  (`Busy`, `Call(error)`, `Answered(code)`, `Malformed`, `Shape`, `Encode`), `Range::fault`
  tells it (default `None`: a range in memory). A successful call clears it.
- **walfsd**: `Blocks` shares the reason in an `Rc<Cell<Option<Why>>>` (walfs owns the device);
  `Walfsd::with(what, op)` names the operation at its 21 sites (stat, count, open, create,
  list, attach, mkdir, remove, rename, copy, set_attr, get_attr); `poison(what)` serves the
  volume as corrupt and says once `walfsd: volume poisoned: <what>: <why>` on the console init
  gave the server (`Walfsd::new(.., console)`, the bin finds `/dev/cons`; `start::say_on` is
  `say`'s half that has its console), keeping the line (`poison_line()`) for tests. With no
  range reason the line says "the device failed".
- **beamlet files.rs:560** (candidate 4, B48's note): a refused `Io::connect` is `emfile`
  (`TooManyThreads`, no waiter slot) or `enomem`, not `eio`.
- **The bench**: `Step::Within(f64)`: the next step must finish within N s, else the session
  fails there ("timed out waiting for ... within the step's own bound"); `pump`, `wait` and
  `idle` honour `min(within, deadline)`; cleared after the bounded step. steward-home-quota:
  `{ within = 120 }` before the rm's expect (a remove answers in seconds; a pass had it at
  114 s after ssh started, bob's fill included, so the bound is the step's, not the case's).
- **Fake kernel**: a refused `call` is shaped as the kernel's (status the error, lend returned,
  no reply) so a test can make a call answer `Busy`; before, `refuse(.., "call", ..)` made the
  runtime panic on an unknown buffer ownership.
- **Host tests**: `a_busy_call_is_made_again_and_a_fault_says_why` (fileserver tests/blkd.rs:
  one Busy then the read succeeds, `fault() == None`, one read reached blkd; 201 Busys →
  `Fault`, `Why::Busy`, nothing reached blkd; blkd's own refusal → `Why::Answered(OutOfRange)`;
  a success clears it; 2.0 s); `a_poisoned_volume_says_so_once` (walfsd server_tests: the
  memory disk fails, a walk is `corrupt`, the line is `walfsd: volume poisoned: stat: the
  device failed`, and the next corrupt request adds none).
- **Pages**: servers/walfsd.md "Failure and restart" (the line, the busy rule and its bound; the
  two host tests in its status); docs/testbench.md's steps example gains `within`.

Gates (q): `cargo test -p redoubt-fileserver -p redoubt-walfsd` ok (fileserver 10 + 2, walfsd
31 + 4); `cargo test -p testbench` 195 ok; consoled's tests (a user of the fake's refusals) 15
ok; `cargo check` of beamlet's redoubt crate clean; `./build --arch rv64 --programs` and rv32
rc=0; docs PASS; rustfmt clean; size-budget PASS after the commit (libs/rt 3924 → 3927,
libs/fileserver 537 → 588, servers/walfsd 821 → 856, with trailers).

Not run: steward-home-quota and the reproduction beside boots (phase 2b, on the orchestrator's
word). The probes (a)-(e) are for the reproduction's scratch copy of the case, not the case.

## Phase 2b: the reproduction (b51-implementer, 2026-10-14/15)

Machine: train 25's whole bench in `.worktrees/train-20` throughout (its boot phase first, then
its serialized `net` tail), plus my own load through q (noted per run). Every case through
`make -f scripts/jobs.mk set CASES="steward-home-quota"` (both widths, `--lock net`), run by
`$REDOUBT_TMP/B51/rerun.sh <tree> <tag> <rounds>`, which keeps each round's console and session
logs, the make output and a `q ls` every 30 s under `$REDOUBT_TMP/B51/runs/<tag>-<n>/`.

Trees (scratch, under `$REDOUBT_TMP/B51/`, each with its own prebuilt):
- `base`: main 9d680b781 + the case copy with the handoff's probes (after `{:rm, `: alice
  `File.stat("/home/alice/b")` as `{:probe, ..}`, `File.rm` again as `{:rm2, ..}`; bob
  `File.stat("/home/bob/x")` as `{:bobprobe, ..}` after `bob-done`; no `within`, which the base
  does not know).
- `probe`: the fix 03423ecf0 + the same case probes (the `within` kept before `{:rm, `) + two
  uncommitted witnesses, never for the branch: (1) libs/fileserver range.rs counts retried
  `Busy` answers in a static, and walfsd's `with()` says `walfsd: probe: <op>: blkd busy, asked
  again: N so far` when it rose; (2) every `Eio` minter in beamlet's files.rs gets its own errno,
  so the shell names the minter: abandoned-while-waiting-to-retry (:227) `etimedout`, abandoned
  (:270) `ehostunreach`, `Reply::Lost` (:301, the connection ended: candidates 3/5) `enotconn`,
  walk answer mismatch (:356) `enotsup`, "any other answer" (:366, candidate 6) `einval`, a
  write answered 0 (:469) `erofs`, (:513) `econnrefused`, a reply of the wrong body (:726)
  `efbig`. A poisoned volume (`Name::Corrupt`, candidate 1) stays `eio`, with the fix's console
  line naming the operation and the reason.
- the fix itself: `.worktrees/B51` at 03423ecf0, prebuilt.

Context from the logs that survive: the failure was seen once, in train 17b's bench
(`.tmp/train-17b/bench.log`: both widths, each 610.2 s at step 9, right after
`walfs-host-tests` 358 s and `kernel-containment [rv32]` 612 s, both widths' `all-*` targets
running at once); trains 15, 16, 17 (first run), 18b, 19, 20, 22, 23, 24 all passed it. No
commit between 98b866854 and 9d680b781 touches walfsd, walfs, fileserver, blkd, aio.rs,
files.rs or the kernel's message.rs.

### Runs

| # | tree | width | verdict | time | rm / probes | beside |
| --- | --- | --- | --- | --- | --- | --- |
| base-0 | base | rv64 | PASS | 189.7 s | rm `:ok` | bench boot phase + 3 extra boots |
| base-0 | base | rv32 | PASS | 166.5 s | rm `:ok`, rm2 `:enoent`, after_rm 4096 | same |
| base-1 | base | rv64 | **FAIL** | 610.1 s | console session: `{:wrote, {64, {:error, :eio}}}` (the 2nd 64 KiB write), then `File.stat!` `:eio`, read `:eio`; no `init: restarted`; sessions never reached | bench + 2× kernel-containment + init-boot/beamlet-files/launch-system/steward-boot; 0 free cores |
| base-1 | base | rv32 | PASS | 169.8 s | rm `:ok` | same, lighter |
| fix-1 | fix | rv64 | PASS | 187.0 s | rm `:ok`, after_rm 4096; no poison line | bench net tail + kernel-containment ×2 |
| fix-1 | fix | rv32 | PASS | 175.6 s | rm `:ok`; no poison line | same |
| probe-0 (first build, before the errno witness) | probe | rv64 | PASS | 194.8 s | no busy line, no poison line | same |
| probe-1 | probe | rv64 | PASS | 206.0 s | rm `:ok`, probe ok, rm2 `:enoent`, bobprobe ok; no busy line, no poison line | bench net tail + host-test load (0-7 free cores) |
| probe-1 | probe | rv32 | PASS | 171.5 s | same | same |
| probe-2 | probe | rv64 / rv32 | PASS / PASS | 191.0 / 172.8 s | same; no witness line | same (4-7 free cores) |
| probe-3 | probe | rv64 / rv32 | PASS / PASS | 203.5 / 192.8 s | same | same |
| probe-4 | probe | rv64 / rv32 | PASS / PASS | 199.2 / 197.7 s | same | same (5 free cores) |
| base2-1 (control, alternating with probe on the net lock) | base | rv64 / rv32 | PASS / PASS | 191.2 / 173.8 s | rm `:ok`, rm2 `:enoent`, after_rm 4096 | host-test + boot load only: train 25's bench ended about 04:45Z |
| probe-5 | probe | rv64 / rv32 | PASS / PASS | 246.3 / 175.1 s | same; no witness line | my load, 8 free cores |
| probe-6 | probe | rv64 / rv32 | PASS / PASS | 218.0 / 170.3 s | same | two load tenants (B51, base), 0 free cores |
| base2-2 | base | rv64 / rv32 | PASS / PASS | 204.7 / 172.2 s | same | same, 0 free cores |
| probe-7 | probe | rv64 / rv32 | PASS / PASS | 190.1 / 173.0 s | same; no witness line | same |
| probe-8 | probe | rv64 / rv32 | PASS / PASS | 189.4 / 175.9 s | same | same |
| base2-3 | base | rv64 / rv32 | PASS / PASS | 188.9 / 186.6 s | rv32 same; rv64's session logs pruned by my load cases in that tree (verdict in `make.log`) | same |
| base2-4 | base | rv64 / rv32 | PASS / PASS | 188.7 / 169.6 s | logs pruned, verdicts in `make.log` | same |

Tally: base 1 FAIL in 12 runs (6 rv64, 6 rv32; the one failure rv64); fix + probe 0 in 18
(9 + 9), with 0 `Busy` answers counted and no poison line. Stopped at 05:35Z (base2 rounds 5-6
not run; my load loops stopped with it).

Logs: `$REDOUBT_TMP/B51/runs/<tag>/` (`steward-home-quota-<w>-smp1{,-alice.ssh,-bob.ssh}.log`,
`beside.log`, `make.log`).

### What the runs say

- **The failure is real on the base and rare**: 1 in 8 base runs (base-1 rv64), none in 16
  probe and 2 fix runs. It struck earlier than train 17's step: alice's console session's second
  64 KiB write (`{:wrote, {64, {:error, :eio}}}`), and *every* later operation of that session
  (`File.stat!`, the read) was `:eio` too; `init: restarted` never printed; the ssh sessions had
  not started, so bob's probe says nothing about it. It came at the machine's heaviest moment
  (0 free cores: the bench's boot phase, two `kernel-containment`s and four more boots of mine).
- **Nothing on the fix tree ever met the condition**: in 18 runs no `Busy` was answered (the
  probe's counter never rose) and no volume poisoned, so those passes are not the retry at work;
  they are the condition not recurring. The `within` step and the probes behaved (rm `:ok`
  within seconds; rm2 `:enoent`; bob `:ok`).
- The kernel's drain lines (`mem: 1024 frames in flight` / `frames in flight 0, at most N`)
  bracket the fill at the same lines in failing and passing logs: not a tell.

### What the code says: candidate 1 cannot happen here; candidate 5 can

**Candidate 1 is unreachable on this image.** The kernel answers `Busy` to a `call` only when
the sender's *group* already has `WAIT_CAP` (32) messages queued at the endpoint
(kernel/src/message.rs:878-887; docs/kernel/ipc.md R2). A group is the sender's (account, label
set) plus, for account 0, its budget id (kernel/src/endpoint.rs:44-62); every boot server runs in
the `system` budget, so blkd's one endpoint sees one group. Its clients are walfsd:data,
walfsd:alice-secrets, littlefsd, erofsd (libs/fileserver/src/program.rs:51) and verityd
(servers/verityd/src/bin/verityd.rs:35); none spawns a thread (no `spawn`/`thread::` under any
of their `src`), and each makes one synchronous `call` at a time, so the group holds at most
five queued messages, ever. `Busy` cannot be minted; the fix's retry is dead code on this
image, and the walfsd page's new sentence ("beside boots the group can fill") is false.

**Candidate 5 is the mechanism by reading, and it fits both failures.** The completion call's
rule (docs/servers/serving.md "The completion call"; docs/userland/native.md "One connection
needs no other thread"): the server holds a parked completion call at most its `hold` (the
hub asks `COLLECT_WAIT`, 10 s, when it has nothing held: libs/client/src/aio.rs:532) and the
client times the call out at `hold + COLLECT_MARGIN_US` (1 s), "so a timeout means the server
broke its promise"; a timeout is `[FAILED, 0]` → `collected(c, None, ..)` → `end(c)`
(aio.rs:360-375, 461-475): every outstanding request comes back `Ended`, the connection is
over for good, and files.rs maps `Ended` to `Reply::Lost` → `Eio` (:713, :298-301), with the
fid "in use for good". The server keeps the promise only *between* requests: `run_around`
(libs/rt/src/server/ninep_mux.rs:485-507) expires deadlines, then `receive`s, then serves the
request synchronously; walfsd's `remove`/`write` run walfs's log write and sync as blkd
calls (`FOREVER`) inside that dispatch, and a blkd `flush` is QEMU's `fdatasync` of the raw image
(tools/testbench/src/qemu.rs:278, default writeback cache) on the host disk the bench's builds
and host tests share. Under `icount` with `sleep=on` the guest clock follows the host clock while
the hart halts, so a host stall of a few seconds in one flush is a few seconds of guest time:
a request dispatched with less than `stall − 1 s` of a parked call's hold left loses that
client's session. Train 17: alice's ssh session sat idle (`wait = "bob-done"`) with a parked
call while walfsd served bob's 5 MiB of writes and syncs; her next request, the rm, was
`Lost` → `:eio`, bob unaffected, no restart — exactly the log. base-1: the console session's
own second write, during the two logins' storm. Per long stall the odds are about
`(stall − 1 s) / 10 s`, which is why it is rare and load-bound.

Candidates 2 and 4 leave later operations healthy (not the pattern); 3 (walfsd died) prints
`init: restarted` (absent); 6 has no path. No busy answer, no poisoning and no restart in 26
runs leaves 5 the only candidate the evidence does not exclude — but it is shown by reading, not
by a run that printed `enotconn`; 16 probe runs did not reach it.

### The reshaped commit (on the orchestrator's word, 2026-10-15)

The `Busy` retry was built (03423ecf0), measured dead (0 `Busy` answers in 18 fix-tree runs,
counted by the probe; and the group argument above: at most 5 queued against a cap of 32), and
removed: `Blkd::call` makes one call again; `Why` loses `Busy`, `RETRY_US` and `BUSY_WAIT_US`
go, the fileserver test is `a_fault_says_why` (blkd's own refusal is the reason; a success clears
it), the walfsd page's status names it and its busy sentence is gone; libs/fileserver's size
ceiling is 573 (was raised to 588). Kept: `Why`/`Range::fault`, the once-only poison line, the
fake kernel's refusal shape, files.rs's `emfile`/`enomem`, `within`. Gates at the reshaped head:
`cargo test -p redoubt-fileserver -p redoubt-walfsd` ok (10+2, 31+4); size-budget PASS; docs
PASS; `cargo +nightly fmt --check` clean; `./build --arch rv64|rv32 --programs` rc=0. Head:
wp-B51 975e4dd0c (one commit on 9d680b781, 17 files, 295+/79−), worktree clean. The real fix
waits on QA thread B51-completion-hold (the Architect).

## Phase 3: the ruling built (2026-10-15)

The Architect's ruling (`.wash/local/B51-ruling.md`, thread B51-completion-hold): the clock
comes off the client. Commit **342ba9e1f** on wp-B51 (the branch's second logical commit, on
975e4dd0c): libs/client aio.rs puts no kernel timeout on the completion call (`FOREVER` in
`Hub::wait` and the waiter; `COLLECT_MARGIN_US` gone; module doc and `wait`'s doc say the hold
is how long the server keeps the call with nothing to say); the pages' sentences as ruled
(serving.md "The completion call" + the residual risk; native.md "One connection needs no other
thread"; status lists: client's old test replaced, rt mux's new test added, 14 → 15).

Tests, as the ruling named them:
- `host:redoubt-client::a_server_late_past_its_hold_keeps_the_session` replaces
  `a_server_that_breaks_its_hold_loses_the_session_at_the_margin` (B20's old flake): 50 ms hold,
  the server paused 1.1 s (past the old margin) with the gate closed; the wait returns only when
  the server is back (≥ 1.1 s); the gated read's answer comes with its buffer, a new read is
  served, no `Disconnected`. Deviation from the ruling's sketch: the server, back from its pause,
  runs `expire` before `wake`, so the parked call comes back empty and the read's answer arrives
  in the next call (`next()` takes it); the observable the ruling asks for — the session kept, the
  answer delivered, the next request served — holds.
- `host:redoubt-rt::a_dispatch_longer_than_the_hold_answers_the_parked_call_after_it`: the test
  file server gains a `slow` node whose write sleeps 600 ms in `write_or_wait` (a file server
  away on its device); b parks a call first (a request is served only into a parked call: my
  first draft forgot that and a's call came back at its hold), a parks one with a 200 ms hold,
  b's write is sent; a's call comes back empty after ≥ 600 ms, a's next read is served in its
  next call, b's write is answered into b's call.
- Both repeated on the quiet core set (mux 5/5, client 3/3); `cargo test -p redoubt-rt
  -p redoubt-client` all ok (quiet).

Gates at 342ba9e1f: docs PASS; size-budget PASS (libs/client 1105 of 1106); `cargo +nightly
fmt --check` (client, rt) clean; `./build --arch rv64|rv32 --programs` rc=0 (a libs/client
change reaches every bin). Boot evidence: steward-home-quota, aio-many-reads,
aio-many-reads-two on both widths from the tree's prebuilt — see the Phase 3 runs below.

Phase 3 runs (prebuilt of 342ba9e1f, `make set`, the machine quiet but for B61 and B52):
steward-home-quota PASS rv64 186.5 s, rv32 169.0 s; aio-many-reads PASS rv64/rv32;
aio-many-reads-two PASS rv64/rv32. Summaries swept: no page, README or milestone text beyond the
two pages mentions the margin, the promise or the case; nothing else to change.

Walfsd untouched by this commit (the ruling: Tier libs/client).

### Rebased onto main 146db8897; gates and negatives

The mux test's slow dispatch is 1.5 s (was 600 ms), so the old 1 s margin genuinely fails it.
Rebased clean: **wp-B51 = f9b3ca749 + 944e5dc29 on 146db8897** (22 files, 399+/122−; main's
B61 also touched the hub, no conflict). Gates at 944e5dc29, through q:
- `cargo test -p redoubt-fileserver -p redoubt-walfsd -p redoubt-client -p redoubt-rt` (quiet
  set): 253 passed, 0 failed.
- docs PASS; size-budget PASS (libs/client 1107 of 1108, libs/rt 3927/3927, libs/fileserver
  573/573, servers/walfsd 856/856); `cargo +nightly fmt --check` on the four crates clean;
  `./build --arch rv64|rv32 --programs` rc=0.
- **Negatives** (scratch tree at 944e5dc29 with the old clock restored: the two `hold` call
  sites `hold + 1_000_000`, and the mux test's `collect_held` given `HOLD + 1_000_000`; tree
  removed after): `a_server_late_past_its_hold_keeps_the_session` FAILS at aio.rs:650 "the call
  came back at 1.050 s, before the server was"; `a_dispatch_longer_than_the_hold_answers_the_parked_call_after_it`
  FAILS at mux.rs:612 `unwrap()` on `Err(Timeout)`. With the clock off, both pass (and 8/8 on
  repeats, quiet set).
- Boot evidence at 944e5dc29 (prebuilt of the rebased tree, `make set CASES=steward-home-quota`,
  logs under `$REDOUBT_TMP/B51/runs/final-*`):
  - **alone** (no load of mine; train 26's bench just starting, 19 free cores): PASS rv64
    198.0 s, rv32 171.3 s; no `eio`, no poison line, no restart.
  - **beside load** (the recipe: train 26's bench's boot phase in `.worktrees/train-20` plus
    two load tenants of mine running host-test and long boot cases; 6 free cores when
    sampled mid round 1): round 1 PASS 251.2 / 251.4 s (rv64 / rv32; rv64's logs pruned by my
    load cases in the same tree, verdicts in `make.log`; the load moved to the probe tree after),
    round 2 PASS 196.9 / 177.4 s, round 3 PASS 190.6 / 174.6 s; all six logs of rounds 2 and 3
    kept: 0 lines of `eio`, `volume poisoned` or `init: restarted`. The `beside.log` samples
    record the running jobs, not the header's free-core count (my sampler's slice dropped it).
  - My load loops stopped at 06:46Z; the machine is train 26's.

### Rebased again onto main 86f51e6fa (train 26, B52: tests only)

**wp-B51 = 9320a3233 + e22d4f0f3 on 86f51e6fa**, clean (same 22 files, 399+/122−; no
conflict). Gates at e22d4f0f3 (q): fileserver+walfsd+client+rt tests on the quiet set 253
passed, 0 failed; docs PASS; size-budget PASS (client 1107/1108, rt 3927/3927, fileserver
573/573, walfsd 856/856); fmt --check clean (4 crates); `./build --arch rv64|rv32 --programs`
rc=0. The proof below ran at 944e5dc29, the same code on 146db8897; B52 adds tests only, so the
boot evidence stands for e22d4f0f3.

### Accepted; rebased onto train 27's head 48023b53f (P3-1: the files.rs conflict with B61)

**wp-B51 = 746305329 + d3fc45d3a on 48023b53f** (main 86f51e6fa + B61 3efd81acf + IDLE2
87553025a), clean. The one conflict, files.rs:560-566: B61 maps `TooManyThreads` to `Emfile`;
mine adds `OutOfMemory` → `Enomem`; resolved to the superset in the first commit (comment merged
from both; `Error::Sys(SysError::…)` with the aliases the file imports). The second commit
re-applied unchanged. Gates at d3fc45d3a (q): fileserver+walfsd+client+rt tests on the quiet
set 253 passed / 0 failed; beamlet-vm + beamlet-redoubt (fake kernel, B61's hub waits beside
the `FOREVER` call) 187 passed / 0 failed; docs PASS; size-budget PASS (client 1108/1109, rt
3927/3927, fileserver 573/573, walfsd 856/856); fmt --check clean (the four crates and
beamlet-redoubt); `./build --arch rv64|rv32 --programs` rc=0. Scratch trees base/probe removed
(07:50Z); `$REDOUBT_TMP/B51/runs/` and `rerun.sh` kept for the logs this report cites.

### Final: the branch stands at its heads on main

On the orchestrator's word (stand down from the train-head rebases; the files.rs hunk against
B61 is resolved in the train's merge commit to the superset mapping), **wp-B51 = 9320a3233 +
e22d4f0f3 on main 86f51e6fa** is the branch to merge; the two train-head rebases below were
done and gated but are superseded (their commits remain in the reflog only). Scratch trees
removed; `$REDOUBT_TMP/B51/runs/` and `rerun.sh` kept.

### Train 27's head moved to d9f6510fe (B63: tests and docs only; superseded)

**wp-B51 = 1e85a63cc + bf9896edd on d9f6510fe**, clean rebase, same 22 files (397+/124−).
Gates at bf9896edd (q): fileserver+walfsd+client+rt on the quiet set 253 passed / 0 failed;
beamlet-vm + beamlet-redoubt (fake) 187 / 0; docs PASS; size-budget PASS (client 1108/1109, rt
3927/3927, fileserver 573/573, walfsd 856/856); fmt --check clean (four crates + beamlet-redoubt);
`./build --arch rv64|rv32 --programs` rc=0.

### Proof: steward-home-quota at 944e5dc29 beside load, 12 runs a width

The orchestrator's bar: at least 12 runs a width beside load, or until the base's failing
condition (0 free cores at a boot phase) is seen in the runs' notes; no `eio`, no `Ended`, the
rm within its step bound. Train 26's bench had finished by 06:50Z, so the load is two tenants of
mine (`base` and `probe` trees: kernel-containment, beamlet-reduction-rate, sched-cluster/latency,
steward-restart, walfs/littlefs/stride/model/memory/host-tests, in rounds), plus B60's and B63's
work. The sampler (`q ls` every 30 s, header kept from round proof-1) records the free general
cores; "a×b" is a samples at b free cores. Logs: `$REDOUBT_TMP/B51/runs/proof-<n>/`; "bad lines"
greps `eio`, `volume poisoned`, `init: restarted` and `Ended` across the round's six logs.

| run | rv64 | rv32 | free cores sampled | bad lines |
| --- | --- | --- | --- | --- |
| final-alone-1 | PASS 198.0 s | PASS 171.3 s | 19 free (no load of mine) | 0 |
| final-load-1 | PASS 251.2 s | PASS 251.4 s | 6 free when sampled by hand (header not kept) | 0 (rv32 logs; rv64's pruned) |
| final-load-2 | PASS 196.9 s | PASS 177.4 s | header not kept | 0 |
| final-load-3 | PASS 190.6 s | PASS 174.6 s | header not kept | 0 |
| proof-1 | PASS 235.2 s | PASS 173.8 s | 9×0 1×1 1×3 2×5 2×9 | 0 |
| proof-2 | PASS 194.0 s | PASS 179.5 s | 6×0 1×1 1×4 1×5 1×6 3×10 | 0 |
| proof-3 | PASS 199.7 s | PASS 186.5 s | 2×0 6×1 2×2 1×3 1×6 1×11 | 0 |
| proof-4 | PASS 191.9 s | PASS 175.9 s | 6×0 1×2 3×3 1×7 1×11 1×12 | 0 |
| proof-5 | PASS 199.6 s | PASS 175.0 s | 3×0 4×1 3×4 1×5 2×13 | 0 |
| proof-6 | PASS 194.3 s | PASS 176.9 s | 3×0 1×1 2×2 2×4 1×6 3×7 1×12 | 0 |
| proof-7 | PASS 195.7 s | PASS 175.6 s | 8×0 3×1 1×7 1×13 | 0 |
| proof-8 | PASS 193.0 s | PASS 176.2 s | 3×0 4×5 2×7 2×8 1×10 1×14 | 0 |

Tally at 944e5dc29: 12 runs a width, 24 of 24 PASS. The base's condition — 0 free cores during
the case's boot and login phase — was met in every proof round (40 of 110 samples at 0 free). Every rm answered `:ok` inside its 120 s `within`
bound (the case would have failed the step otherwise); no `eio` anywhere.

### What follows for the fix (not done: a design decision)

The commit on wp-B51 (03423ecf0) needed reshaping, not a fix-up (done above):
- **Drop** the `Busy` retry (`RETRY_US`/`BUSY_WAIT_US`, `Why::Busy`) and the walfsd page's busy
  rule and its host test's busy half: a retry for a refusal the kernel cannot give, and a page
  sentence that is false. `Why` (`Call`/`Answered`/`Malformed`/`Shape`/`Encode`), `Range::fault`,
  the once-only poison line, the fake kernel's refusal shape, files.rs's `emfile`/`enomem`, and
  the `within` step (which turns a 610 s wait into a seconds-long verdict naming the step) all
  stand on their own and stay.
- **The real fix is a design question** the pages do not settle, so it is for the orchestrator
  and the Architect, not improvised here. The rule: a server answers a parked completion call
  within its hold (serving.md "The completion call"); the gap: a single-threaded server whose
  request waits on a device cannot bound that wait. Options: (a) the serving library keeps
  the promise regardless of a request's length — before dispatching a request, answer any parked
  call whose remaining hold is under a bound (the client calls again at once; one extra round
  trip per 10 s at most); (b) the client's margin covers a device operation (a bigger
  `COLLECT_MARGIN_US`, or a per-server bound the hold negotiates) — weaker: the stall is
  unbounded on a shared host; (c) walfsd answers its parked calls from a second thread — against
  the library's one-thread shape. (a) is a libs/rt change (Tier A, a hotspot: every 9P server and
  the hub) with a host test of its own (a request that outlives the hold loses no session) and
  the page's rule reworded. It would make the steward-home-quota failure impossible by
  construction instead of rare.
- Candidate 5 can be shown deterministically on the host before any fix: a 9P server test whose
  device stalls past a parked call's hold + margin, asserting the client's `Ended`. That test
  belongs with the fix.

Scratch trees kept for the reviewer: `$REDOUBT_TMP/B51/base` (9d680b781 + case probes),
`$REDOUBT_TMP/B51/probe` (03423ecf0 + case probes + the two witnesses, `git diff` shows them),
`$REDOUBT_TMP/B51/rerun.sh`; removed on the orchestrator's word. Gates at the head: unchanged
since phase 2a (nothing committed in 2b).
