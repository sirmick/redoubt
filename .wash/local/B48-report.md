# B48 report (b48-implementer)

Worktree /home/mcloonan/redoubt/.worktrees/B48, branch wp-B48 from main 92b84a5b8.

## Design

**The leak.** `Redoubt.Pipes` starts `piped` with `launch(serve: "serve")`; the platform makes the
endpoint itself (`jobs.rs` `served()`), hands the child the receive right and keeps the send right
as a `Cap` over `Owned::new` (closed when its last copy goes, no Life: the endpoint is charged to
the VM's budget, so piped's death leaves the slot to the VM, objects.md "No call destroys an
endpoint"). `Namespace.bind("/dev/pipe", conn)` then attaches it as a 9P connection and pushes a
`Known` into `Sys.known` (`system.rs` `bind`, the `None` arm) holding a clone of that `Owned` and
the `Connection`, and binds the prefix. `Sys.known` is append-only: its entries were the VM's
startup handles, kept for the VM's life, and nothing ever removed one. So after piped's budget is
destroyed the entry pins the kernel handle (one slot of MAX_HANDLES per pipeline) and the
connection, and `/dev/pipe` stays bound to a dead server. The next pipeline's bind replaces the
prefix but not the entry.

**The owner.** A `Known` made by a bind belongs to the budget of the server it reaches: every such
entry carries that budget's `Life` as `returns` (a served send right's from `launch`'s budget; a
handle a reply brought, from the connection it came on). Startup entries carry none and stay.

**The return path.** `budget_destroy`, after the kernel's destroy succeeds and the Life is ended,
drops every `Known` whose `returns` is that Life and unbinds every prefix naming its connection
(`Namespace::unbind(&Connection)`, new in libs/client/ns.rs: by `same`). The `Owned` then has only
the Elixir term's copies; when the last is collected its `Drop` closes the send right, which is
the VM's to close. The hub's record and waiter for the connection end as today: the waiter returns
when its completion call dies with the server and gives its place up at the next `connect`
(`Io::connect`, B47). One rule, stated on the pages: a destroyed budget takes every binding of a
connection to a server it ran, so nothing of piped exists at the prompt (Pipes' moduledoc, now
true for the binding too). Not reuse-one-piped: the design is one piped per pipeline run, destroyed
when the last holder lets go (piped.md "Started by a session"); unchanged.

**Failure modes.**
- A pipeline killed mid-way: `Pipeline.own`'s `after` destroys the stages and `Pipes.release`s;
  the last holder's release destroys piped's budget, the same path. Another pipeline still holding
  piped keeps it; the entry goes when the last lets go.
- piped exits or faults on its own: `Pipes.handle_info({:exit, ...})` destroys the budget; same path.
- The session ends with pipelines open: the VM's process ends and the kernel frees its whole
  handle table (objects.md "A handle held inside dies with its holder's table"); no VM state matters.
- `Budget.destroy` fails: the Life is not ended and the entry stays, one leak; destroy failing is
  the bug then, not this path. Nothing is dropped before the kernel has agreed.
- Open files or a pending clunk on the connection hold their own `Connection` clones: they fail
  `disconnected` as today; unbinding affects only lookups to come.
- A bound handle that reached the VM through no budget it carved (`returns` None, a request's
  handle) stays known while the VM runs, as startup handles do: a residual, stated.

**Tests.** Host, `beamlet-redoubt::tests/system.rs`: a thousand times, carve, launch with serve,
play the server with a littlefsd on the served receive right (new fixture `volume_served`), bind
`/dev/pipe`, use it, destroy, stop; the namespace table and the fake kernel's handle count
(`f.held`) are where they started. Machine: `pipe-sixty-four`, pipe-eight's sibling, 64 pipelines
in one session on both widths; the session prints the table's size before and after and whether
`/dev/pipe` resolves at the prompt.

## Result (2026-10-14)

Base 92b84a5b8, head **cf9074eae** on wp-B48, one commit, not pushed. Worktree clean.

**What changed.**
- `libs/client/src/ns.rs`: `Namespace::unbind(&Connection)`, one code line, removes every prefix
  naming the connection (by `same`). The crate sat at its size ceiling (1106 code lines); raised
  to 1107 in `tests/size-budget.toml` with the commit's `Size budget: libs/client:` line.
- `userland/otp/redoubt/src/system.rs`: `budget_destroy`, after the kernel's destroy and
  `life.end()`, drops every `Known` whose `returns` is that Life and unbinds its connection's
  prefixes. Module doc says the rule.
- `userland/otp/redoubt/src/fixture.rs`: `volume_served(&Launched, ...)`: a littlefsd on the
  endpoint a launch with `serve` gave the child (`volume` is now `volume_on(.., None)`).
- `userland/otp/redoubt/tests/system.rs`: `a_served_servers_binding_and_handle_go_with_its_budget`,
  1000 pipelines (14 s): after each, `/dev/pipe/p` is `not_found` and the table is as before,
  before the terms are dropped; the fake kernel's handle count equals the count after the first
  pipeline (the first launch starts a job thread: its endpoint and two rights, kept for the VM's
  life, `jobs.rs`). **Mutation:** with the system.rs change reverted (fix.patch), it fails at
  pipeline 0: `/dev/pipe/p` still resolves after the destroy (`.tmp/B48/host-mutation.log`).
- `tests/pipe-sixty-four.toml`: 64 pipelines in one session, both widths; one line judges all
  (`{:ran, 64, [{"hi\n", [exited: 0], true, true}]}`), then `{:table, true, {:error, :not_found}}`.
  About 20 s a width.
- Pages: native.md (pipes: status +pipe-sixty-four, the Pages bullet), piped.md ("Started by a
  session": status, the no-budget-outlives sentence), files.md (binds: status +host test, a bind's
  lifetime), beamlet.md ("Handles are resource terms": the VM's own send right, the known table's
  rule, the tests). `Redoubt.Pipes` moduledoc.

**Gates (env from launch-env-2026-10-12.md, all through q), all rc 0:**
- `cargo test -p beamlet-redoubt --features fake --test system` (userland/otp): 21 passed, 27 s,
  on 7f4a4fbab (same code as cf9074eae but for size-budget.toml).
- `q run --cores 8 -- ./test-shell`: every stage passed (BEAM 292 passed, 70 skipped; beamlet-redoubt
  system 21 passed), on 83ae0bf64 (ns.rs's unbind was then five lines, same behaviour).
- `make -f scripts/jobs.mk prebuilt` at cf9074eae: rc 0 (rv64 and rv32, 278 cases each).
- `make -k -f scripts/jobs.mk set CASES="pipe-sixty-four pipe-eight pipe-carries pipe-interrupted
  pipe-never-reads pipe-no-authority pipe-hostile-output job-kill userland-boot beamlet-footprint
  client-host-tests size-budget formatting"` at cf9074eae: 23 of 23 PASS (both widths where the
  case has them; size-budget, formatting, client-host-tests rv64). pipe-sixty-four 20.1 / 20.8 s.
  A first run at 83ae0bf64 had size-budget FAIL (1111 of 1106), the rest PASS.
- `make -f scripts/jobs.mk docs` at cf9074eae: PASS, rc 0.
- Formatting of each changed Rust file: `rustfmt +nightly --check`, ok; `git diff --check` clean.
- **Not run:** the whole bench; the fake-kernel sweep over the other libs crates (ns.rs gained
  one method nobody else calls; client-host-tests and prebuilt both built it).

**Summaries checked** (grep for pipe, piped, /dev/pipe, handle table, leak). README.md:35 and
docs/userland/README.md:22,62 name pipes among the shell's features only; GETTING-STARTED.md has
nothing; docs/servers/README.md:72,244 (piped's row: "the endpoint its session's launch made", the
buffers of one session's pipes) still true; its :339 leak note is a dead launcher's, not this.
No change to any. docs/userland/sessions.md "Namespaces" Binds
paragraph: still true (a bind creates no authority); the lifetime rule is files.md's, which it
links; no change. docs/userland/shell.md pipes status: lists no pipe-eight either; no change.
docs/servers/piped.md "Residual risks": unchanged, nothing there claimed otherwise.
docs/userland/beamlet.md "Threads" paragraph (waiters, pipe-eight): still true; no change.

**Residuals.** A handle bound that came through no budget the VM carved (a request's handle,
`returns` None) stays known while the VM runs, as startup handles do. The hub's `Conn` record and
its waiter end as before (lazily, at the next connect past MAX_WAITERS), B47's path.

**Review:** Tier A (the session's authority), steward-red-2.

## Round 2 (steward-red-2 OK with notes on cf9074eae), 2026-10-14

Head **12e44667e** on 92b84a5b8, still one commit, not pushed; worktree clean. It differs from
0b3e500a7, where prebuilt, the 13-case set and docs ran, only in tests/system.rs (the two settle
fixes below); the host suite and ./test-shell ran at 12e44667e.

**P2-1, the window.** Forgetting the Known and letting the term close the send right left the
index free while `Io.conns` and open files still named it, and the hub's record ended only when
its waiter saw the death: a later request on the old record could go to the next holder of the
index (the next launch's send right), a wrong recipient inside the session's own table. Of the two
remedies the reviewer named, the first is in; the second was tried and backed out (below).
- `Hub::disconnect(conn)` (libs/client/aio.rs, one line: `end` from the caller's side): what is
  outstanding comes back `Ended`, the queue is dropped, and every submit after is refused
  `Disconnected`, so a stale file's clunk at its term's collection is never sent.
- `Io::forget(conn)` (io.rs), called from `budget_destroy` for each forgotten connection: ends its
  hub record. Nothing else of `Io` changes.
- Test `a_forgotten_connections_requests_reach_no_later_holder_of_its_index`: open a file on the
  bound server, destroy, drop the term (the handle count falls by one: the index is free), launch
  again (its send right takes the index; the child's receive right is played into a probe
  process), close the stale file and idle: the probe's endpoint has 0 queued; stop the server and
  the waiter's own handle goes. **Mutation** (io.rs, system.rs, aio.rs at cf9074eae's: the
  `.tmp/B48/r2.patch` run was of the parked variant, whose first check differed; the disconnect
  path is what this test's `queued == 0` holds, and without `disconnect` the clunk goes out on the
  reused index to the probe: to be rerun on the final head if the reviewer wants the log).
- **Parking, tried and backed out.** Parking the `Owned` in `Io` until the hub released the record
  (released at the waiter's last hand-over) passed the host suite twice (22/22) but made
  `pipe-sixty-four` fail on rv32 every run (4 of 4: the 64th, 25th, 7th and 9th pipeline answered
  `{:error, :refused}`; rv64 passed each time), with nothing in the console log but the tuple. A
  bisect: lazy release with parking kept, 2 of 2 rv32 FAIL (pipelines 12, 19); disconnect without
  parking, 2 of 2 rv32 PASS (17.1 / 17.5 s). Control, cf9074eae rebuilt in a scratch worktree:
  2 of 2 rv32 FAIL too (pipelines 29, 14), where the same head passed 4 of 4 this morning; so the refusal is load-dependent and not round 2's, and the bisect's passes and failures were chance. On the final head the 13-case set then passed 23 of 23, pipe-sixty-four rv32 among them (19.4 s). So holding the dead send right open past the term's collection is what rv32 cannot
  take, by a mechanism not found: a `:refused` from a later pipeline's first request on its own
  piped, at a random pipeline. It belongs to whoever opens it next; the waiter's own window stays,
  stated on beamlet.md's residuals as the round-2 note allowed.
- `Fake::served_child(owner)` (libs/rt/fake): the fake's `last_launched` is global to the binary,
  and twice handed the window test a child another test had just killed; both B48 tests take the
  latest child whose first handle is a receive right the VM holds a send right to.
- The thousand test's baseline is taken once the handle count has held still for 50 ms: the
  waiter closes its own handle after its hand-over, on its thread (one run read it a moment early).
  B47's `servers_bound_and_ended_one_after_another_never_run_out_of_waiters` had the same early
  first sample and failed once under ./test-shell's full parallel suite beside my thousand
  launches (`[17, 16, 16, ...]`); its first count now settles the same way. A candidate for
  B52's "a test that fails about 2 runs in 30".

**P3-2:** files.md, beamlet.md, system.rs's module doc and the commit message say "the budget the
handle came through" (a `launch` with `serve`'s budget, or the connection a call brought it on).
**P3-1:** the waiter's window is on beamlet.md's residuals, with the parking finding.

**Size budget:** libs/client at 1108 (two one-line methods); the commit's line names both.

**The rv32 refusal, named (orchestrator's (a)).** Redoubt.Pipes and Redoubt.Pipeline mapped every
file error but enoent to `:refused`; they now answer `{:refused, {:read, path, reason}}`,
`{:refused, {:mkdir, reason}}` and `{:refused, {:mint, reason}}` (pipes.ex, pipeline.ex, their
specs and docs; native.md "Bounds"), which is the orchestrator's finding (1) folded in: small, and
the diagnostic itself. With it, four rv32 runs beside SCHED4's machine step (`q ls`: one 2-core
`before/rv32-containment-smp2` job running, quiet cores free) all failed the same way, at pipelines
10, 12, 7, 7: `{:refused, {:mkdir, :eio}}`, the fresh piped's first request, the pipe directory's
mkdir. In the VM that is files.rs's `io.connect(&conn).map_err(|_| Eio)`: `Io::connect` refuses
`TooManyThreads` when `MAX_WAITERS` (6) hub connections exist and none can be released, a release
needing the connection's waiter to have handed over its end. The failing pipelines sit right past
six, and the earlier random indices (19, 25, 29, 64) fit the same window: an ended piped's waiter
returns only once its completion call has failed with the server's death and the VM has taken its
last hand-over; under load on rv32 the next pipeline's first request comes first. A pass of the
same case shows the waiters returning in time (rv64 every run; rv32 when the machine was quieter).
Classification, as asked: **not B48's** (the cf9074eae control fails the same way, and B47's
lazy release is what the bound rests on); **a pre-existing limit the case exposes**: the six
waiter places are a hard bound with a timing dependence that beamlet.md's Threads paragraph
states as if free ("a session runs any number of pipelines, each taking one place while it
runs"). **B-node candidate:** `Io::connect` at MAX_WAITERS with every place held by an ended
connection whose waiter has not yet handed over its end should wait, bounded, for a hand-over
(or ask the waiter to return) rather than refuse `TooManyThreads`, which the files layer reports
as `eio`; with a case that runs pipelines back to back under load on rv32 (pipe-sixty-four, once
the fix lands, back out of the quiet class). Not folded into B48: the bound and its wait are the
hub's design (beamlet.md "Threads", B47's), the reviewer's domain, and the hour was up.
**The case** keeps its 64 and joins the quiet class (scripts/jobs.mk, with the reason; the toml's
comment says why): a host-clock case whose failure beside other work is no verdict
(testbench.md, "On a shared host"). To be run alone once when SCHED4's step pauses, as the
orchestrator said. Test for the new error shape: the machine runs above print it; no ExUnit case
provokes a server's refusal on the fake, stated as a residual.

**Gates at 0b3e500a7 / 12e44667e, all through q (beside SCHED4's machine step: `q ls` showed its one 2-core
job, `before/rv32-containment-smp2`, quiet cores free):**
- `cargo test -p beamlet-redoubt --features fake --test system` at 12e44667e: 22 passed, twice
  (29.2 s, 29.2 s), rc 0; at 0b3e500a7 22 passed once, then one of two tests read a waiter's
  handle a moment early under ./test-shell's parallel suite (fixed by settling, above).
- prebuilt: rc 0 (278 cases each width).
- the 13-case set both widths: 23 of 23 PASS; pipe-sixty-four now in the quiet class, rv64 20.3 s,
  rv32 16.3 s (a pass stands; its earlier failures beside other work are above).
- docs: PASS, rc 0.
- `./test-shell` at 12e44667e: every stage passed, rc 0 (BEAM 292 passed, 70 skipped; beamlet 361
  passed, 1 skipped; beamlet-redoubt system 22 passed). At 0b3e500a7 it failed once in that
  suite on the early sample, as said.
- `rustfmt +nightly --check` on each changed Rust file, `mix format --check-formatted` on the two
  Elixir files, `git diff --check`: clean.
- Not run: the whole bench; pipe-sixty-four alone with the machine quiet (awaits the orchestrator's
  word).

**Summaries checked (round 2).** beamlet.md "Threads" paragraph: still says an ended connection
gives its place up when the next one needs it (true; eager release backed out) and that a session
runs any number of pipelines, which the B-node above qualifies; left as is, the residual states
the window. native.md "Bounds": the refusal shapes added. shell.md:247 (pipelines' status) and
:689 (`screen`'s `{:error, {:refused, why}}`): the same shape now; no change. README, userland and
servers READMEs: as in round 1, nothing to change.

**Files in the commit (final):** docs/servers/piped.md, docs/userland/{beamlet,files,native}.md,
libs/client/src/{aio,ns}.rs, libs/rt/fake/src/lib.rs, scripts/jobs.mk, tests/pipe-sixty-four.toml,
tests/size-budget.toml, userland/otp/redoubt/src/{fixture,io,system}.rs,
userland/otp/redoubt/tests/system.rs, userland/shell/lib/redoubt/{pipeline,pipes}.ex.

## Round 3 (steward-red-2 OK with notes on 12e44667e), 2026-10-14

Head **280d2da04** on 92b84a5b8, one commit, not pushed; worktree clean. Since 12e44667e only
docs/userland/beamlet.md and userland/otp/redoubt/tests/system.rs moved; the code is 12e44667e's,
whose prebuilt, 13-case set and test-shell stand.

- **P2-1:** beamlet.md's Threads paragraph qualifies "a session runs any number of pipelines": as
  long as an ended piped's waiter has handed its end over before the place is needed; six that
  have not refuse the seventh fresh one `{:refused, {:mkdir, :eio}}` (pipe-sixty-four on rv32
  beside other work). The residuals list states it as an open item beside the waiter's window,
  with what it owns: a bounded wait at the bound, and a refusal that names a thread bound. Not
  named B61 on the page: the docs checker's C4 refuses a package ID in a page (three hits at
  beamlet.md:964, 969, 1159 on the first try), so the page says "open" and this report says B61.
- **P3-1:** the window test's mutation at the head. The first run of it passed: the test's stale
  clunk went out, under the mutation, on a slot the fake had already given to another handle or to
  none, and the probe server had stopped receiving, so the hub's send (a rendezvous, `SUBMIT_TIMEOUT_US`)
  was never taken and the endpoint's queue stayed empty with or without the fix. The test now has
  the second server answer two calls with handles it mints (the second taking the freed index, the
  pool's thread already started by the first) and go on receiving, counting whatever arrives after.
  With the fix (`Io::forget` + `Hub::disconnect`) reverted (`.tmp/B48/r3-fix.patch`, and the
  `io.forget(conn)` line stubbed), it fails at `redoubt/tests/system.rs:472`:
  `assertion failed: a request reached the new holder, left: 1, right: 0`
  (`.tmp/B48/host-r3-mutation3.log`); with the fix, 1 passed (`host-r3b.log`).
- **P3-2, for B61:** `userland/otp/redoubt/src/files.rs:560`, `io.connect(&conn).map_err(|_| FileError::Eio)`,
  maps every connect refusal to `eio`, so a thread bound (`TooManyThreads`) reads as a device
  error at the prompt; B61 should give it its own name.
- **Gates, through q:** `cargo test -p beamlet-redoubt --features fake --test system` at cb7163340
  (280d2da04 but for the three page rewordings): 22 passed, 31.6 s, rc 0. `make docs` at 280d2da04:
  PASS, rc 0 (at cb7163340 it failed C4 on the three `B61` mentions, reworded). The window test
  alone with the fix (host-r3b.log) 1 passed; with the fix reverted (host-r3-mutation3.log) FAILED
  at tests/system.rs:472 as recorded. `rustfmt +nightly --check`, `git diff --check`: clean. Not
  rerun since 12e44667e: prebuilt, the 13-case set, ./test-shell (no code moved); the whole bench;
  pipe-sixty-four alone (train 25, when SCHED4's step pauses). Beside this round: SCHED4's
  rv32-containment-smp2 job.
