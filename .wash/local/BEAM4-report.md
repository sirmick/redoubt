# BEAM4 report: the natives, generated Elixir clients, the shell's launch

## Summary (for the result)

The seven natives (twelve functions, module `redoubt`) are built behind `Platform::system()`,
with handles as resource terms, generated Elixir clients from the wire tables, the thin
`Redoubt.Namespace/Budget/Process/Keys` layer and the shell's `ns`, `ns_lookup`, `bind` and
`exec`. Host-tested on a test platform (beamlet-vm) and on the fake kernel (beamlet-redoubt), and
on the machine on both widths in four new cases under a tester in the steward's place.

Two branches, as ruled:
- **wp-BEAM4** (on main be8f7ec0b), head b25abdee5: six commits, everything but the machine cases; the
  editor's eight, the simplifier's seven and the red team's 1-5 folded.
- **wp-BEAM4-cases** (on wp-STEWARD2's tip 62d60cb5d), head 072295075: the same six (the wire
  commit's generated clients regenerated for STEWARD2's `consol` change and new `steward` table),
  then one commit with the four cases, the tester, the pages' bench citations and the M1 plan
  edits. To be rebased when STEWARD2 merges; then wp-BEAM4 takes the cases commit.

## Commits (wp-BEAM4, oldest first)

1. `vm: the system's natives, module redoubt, over the platform's System`: platform.rs `System`
   trait and types; bif/system.rs (argument checks, caps, term building, `poll_system`); the
   registration rows; vm.rs `system_waits` and the poll; vm/tests/system.rs and its fixture.
2. `rt: a startup block lists its named handles; the fake kernel ends a thread`:
   `Startup::handles()`; the fake kernel's `thread_exit` parks the host thread. Size budget:
   libs/rt +3 (3518 → 3521 on main; 3524 → 3527 on STEWARD2's base).
3. `beamlet-redoubt: the system calls on Redoubt`: system.rs (caps, namespace, budgets, labels),
   pool.rs (typed calls), serve.rs (served endpoints), jobs.rs (launch and exit notices), io.rs
   (non-hub wake-ups), build.rs (the loader stub), tests/system.rs; beamlet.md "Natives" and the
   `Platform` tables.
4. `wire: generated Elixir clients, one function per message over the call native`: generator
   output `libs/wire/elixir/client/NAME.ex`, the hand-written `lib/wire.ex` (moved) and
   `lib/client.ex`, test/client_test.ex run by run-vectors (elixir-oracles); wire.md, ipc.md R13
   and SECURITY.md's R13 row.
5. `shell: the namespace, budgets, programs and keyd over the natives; ns, bind and exec`: the
   four thin modules, the session commands, the `handle` parameter type, mix.exs compiling the
   wire's Elixir, the registry skipping the wire's modules; native.md, shell.md, sessions.md.
6. `tests: beamlet's natives' programs`: tests/beamlet-programs with beamlet-hello and
   beamlet-caller.

wp-BEAM4-cases adds:
7. `tests: beamlet's natives on the machine, a session's VM under a tester in the steward's
   place`: beamlet-session, the four case files, manifests, args files, userland recipe, Erlang
   modules; status lines naming the cases; docs/plan/m1-separation.md.

## Design (the rulings, and what was built)

- **Q1 labels/0.** Ruled: from the kernel's stamp, no ABI change. Built: a send to one's own
  endpoint cannot be made from one thread (a send waits for its receiver), so the platform starts
  the first typed-call thread at its start and takes the labels off that thread's first wake-up,
  which the kernel stamps with the process's labels. Kernel truth, fixed, no ABI change.
- **Q2 the budget.** Ruled: the parent budget is the named handle `budget`, otherwise `no_budget`;
  the machine cases run beamlet under a small tester in the steward's slot (STEWARD2's `steward`
  section; init hands it `users`), not as sshd/login sessions. Built: `beamlet-session`. init
  refuses a steward's manifest arguments other than `buckets=N`, so the tester reads what to run
  from a bundle file, `/boot/beamlet-session.args`, that each case injects.
- **Q3 typed calls.** Ruled: not on the VM thread; a bounded timed pool, constant beside
  MAX_WAITERS, caller parks as a file operation does, result as `{reply, Ref, Result}`. Built:
  `CALL_THREADS` (2), `MAX_QUEUED` (64, `busy` past it), bounded by the call's timeout (≤ 5 s).
  Departure: the caller does not park through `asker/finished`; `call/3` returns `{ok, Ref}` at once
  and the caller waits in `receive` for the reply message, so no scheduler and no other process
  waits either way. A caller that dies does not cancel its call: the call runs to its deadline and
  its reply, with any handles, is dropped and closed. The pool's first thread is started at the
  VM's start (it reads the labels); its stack is 4 pages, counted in beamlet-footprint.
- **Q4 serve/1.** Ruled as recommended. Built: one thread per served endpoint (at most 2), the
  library's `Admission` (8 buckets, 4 in flight each) and `Parked` with `REQUEST_WAIT_US` (5 s);
  requests reach the VM as one wake-up each, header and lent bytes in a transfer; `reply/2` goes
  back as a send through a badge minted from the receive right (u32::MAX), carrying the reply's
  handles so the thread holds its own copies. R26 and R28 claimed, R77 not.
- **Unasked points that stood:** a thread per running job for its exit notice (MAX_JOBS 4,
  reused); the stub embedded in beamlet by build.rs, as init and the steward do (not read from
  /boot); term glue in vm/src/bif/system.rs.
- **ns_lookup of a name.** `ns_lookup/1` of a name with no `/` gives the named handle: Elixir has
  no other way to reach `keyd` or `budget`, and the table names no eighth native.

## Gates (exact commands and exits)

Cases through `make -k -f scripts/jobs.mk -C <tree> <arch>/<case>` after `make ... prebuilt`
(rc 0 both trees, both widths); host tests through `scripts/q run`.

wp-BEAM4 (main's base):
- Both builds: prebuilt rv64 206 cases built, rv32 192, rc 0.
- Both widths, PASS: beamlet-files, beamlet-console, beamlet-boot, beamlet-footprint, aio-many-reads,
  userland-boot, init-boot, ipc-outcomes, bench-net-peer (the smoke set entire).
- rv64, PASS: docs, formatting, size-budget (libs/rt raised +3 in its commit), unsafe-budget
  (unchanged: no `unsafe` added; beamlet-redoubt is `forbid(unsafe_code)`), no-cruft,
  elixir-oracles (vectors and the generated clients' checks), wire-host-tests, client-host-tests,
  rt-host-tests.
- Host: `cargo test -p beamlet-vm` all pass (system 15/15); `cargo test -p beamlet-redoubt --features
  fake` all pass (system 8/8, console 10, files 15, limits 7, lookup 2, pack 2, userland 2);
  `cargo test -p redoubt-client` pass; `cargo test -p redoubt-wire` pass; `cargo test -p
  redoubt-wire-gen` 18/18; `mix test` in userland/shell 85/85 (the client test moved to
  libs/wire/elixir/test, run by elixir-oracles); `cargo test -p redoubt-doccheck` pass.
- After the last platform fix (startup handles kept for the process's life), beamlet-redoubt's
  suite was rerun (pass); the machine cases were rerun on the cases tree only (below).

wp-BEAM4-cases (STEWARD2's base):
- Both widths, PASS, twice (before and after the last fix): beamlet-natives, beamlet-serve,
  beamlet-launch, beamlet-natives-attack; and beamlet-files, beamlet-footprint.
- PASS: userland-boot rv64, init-boot, ipc-outcomes, bench-net-peer, aio-many-reads,
  beamlet-console, beamlet-boot (both widths); docs, formatting, size-budget, unsafe-budget,
  no-cruft, elixir-oracles, rt-host-tests, wire-host-tests (rv64).
- FAIL, STEWARD2's base, not this package: rv32 userland-boot (`init: sshd (PID 305) exited, code
  3`; wp-BEAM4 passes it on main), and client-host-tests (`libs/client` test `console` does not
  compile: a match without consol's new `ended`; this package touches no `libs/client` file).

## Outside the owned paths

- `libs/rt/src/startup.rs`: `Startup::handles()` (the signature the natives need).
- `libs/rt/fake/src/lib.rs`: `thread_exit` parks the host thread (test-only fake kernel).
- `tests/size-budget.toml`: libs/rt +3, with the `Size budget:` line.
- `docs/kernel/ipc.md` R13 and `docs/SECURITY.md` R13 row: bench:elixir-oracles added.
- `Cargo.toml`/`Cargo.lock`: the tests/beamlet-programs member.
- `docs/plan/m1-separation.md` (cases commit): progress claims.

## Documentation check (summaries)

- docs/userland/beamlet.md: "Natives" to built (status, the twelve functions, refusals, bounds,
  the handle rule's tests); `Platform` table gains `system`; "beamlet on Redoubt" status and table.
- docs/userland/native.md: "Launching from a session" built; "Standard input and output" keeps
  both Open choices and says what M1 builds (a fresh console connection, readable too) and that
  "no stage holds the console" waits for pipes; "Killing a job" says budget_destroy/1 is the kill
  today; "The client library" names the VM as a caller of `ns` and `launch`.
- docs/servers/wire.md: "Generated clients" built; the generator's output and figure name the
  client file.
- docs/userland/shell.md "The shell in a session": exec, ns, bind; status names bench:beamlet-launch.
- docs/userland/sessions.md: the `ns()` example is what `ns()` prints for a steward's session.
- docs/kernel/ipc.md R13, docs/SECURITY.md R13 row: the generated-client test.
- docs/plan/m1-separation.md: generated clients built; the system natives run on the machine
  under a tester; the shell's exec; "Not built" no longer lists native launching.
- Checked, no change: README.md, GETTING-STARTED.md (no claim about the natives or launching);
  docs/userland/README.md (planned status of session wiring stands: the steward's sessions are
  STEWARD2's); docs/plan/m2-usable-shell.md (screen natives only).

## Footprint

beamlet-footprint, rv64 on main's tree: heap peak 5,287 pages of the 10,861 cap (main's table:
5,429); stack 35,992 bytes of 18 pages (35,288). rv32 on the cases tree: heap 5,108 pages, stack
29,224 bytes. The manifest's 10,880-page budget and 10,861 cap stand. The registry change (the shell no longer loads every
module of its application to find commandlets) is why the peak fell while the wire's 22 modules
joined the application.

## Red team (BLOCK on 1-3), folded

1. (P1) serve.rs held the served endpoint by handle only: dropping the Erlang side's resource
   closed the receive right under the serve thread. Now the served endpoint's object is held by
   its thread's record for the VM's life; test `an_endpoint_served_stays_open_when_its_term_is_dropped`
   (a reply-brought receive right, its term dropped, still served; fails without the fix).
2. (P2) bind's attach waited without bound on the VM's thread: now `Connection::attach_within`,
   each call at most `ATTACH_US` (1 s); test
   `a_bind_to_a_server_that_never_answers_is_refused_within_its_bound`.
3. (P2) serve/1 twice on one endpoint made two threads with one answering badge: refused
   `already_served` (tested in the requests test); a served endpoint's slot is for the VM's life,
   said on the page.
4. (P3) the hand-off to a pool or job thread waited FOREVER: now `HAND_US` (1 s; 100 ms proved
   too tight on a loaded host, where a merely slow thread was retired and its work lost, a
   1-in-10 flake of the host suite found and fixed: 30/30 since), a thread that does not take it
   set aside and the call ended `protocol`; client.ex's wait has `after timeout + 5 s`.
5. (P3) the stale comments were already fixed with the simplifier's folds.
6. (note) expired -> malformed stays a residual; follow-up: a wire `timeout` status.
7. (Q6) as in the risks below.

## Open risks

- (Red's Q6, settled) A child spec asking for all of the VM budget's free weight is refused by the
  kernel (`invalid_argument`, budget.rs: a budget holding a process keeps weight to run on); the
  VM's silent exit, code 0, in that first launch run was the platform dropping the process's own
  startup handles, the console's among them, at the VM's end, so the result line was lost. Fixed
  (startup handles are kept for the process's life); the launch case prints exec's whole result.
- An expired or over-admission request is answered `malformed` (code 1): no status every protocol
  shares means timeout.
- A program `exec` launches can read the session's console (its own connection), so it can take
  typing the shell would read, until pipes (M2).
- The cases run the VM under a tester, not the steward: the steward's own sessions with these
  natives are STEWARD2's follow-up.
- Typed calls by a dead caller are not cancelled; they run to their deadline (≤ 5 s), holding one
  pool thread.

## Cases branch (2026-10-07, re-aimed)

wp-BEAM4-cases is one commit, f3c06d547, on wp-STEWARD2's announced tip 10bda633a (on main
b60c7cc5c, which carries BEAM4's merge bdb38430e): the branch was reset to the tip and the cases
commit 072295075 cherry-picked; the six natives commits the merge already carries were dropped,
nothing else of the old 22 was ours. The generated Elixir clients on the tip are current for
STEWARD2's `consol` change and `steward` table (redoubt-wire-gen 18/18, `generated_files_are_current`),
so no regeneration was folded.

Conflicts resolved (four pages, status lines): docs/userland/beamlet.md "Natives" status now
"partly tested: the machine's cases run the VM under a tester in the steward's place, not under
the steward itself · tested (29)", the four bench cases first; docs/userland/native.md "Launching
from a session" adds bench:beamlet-launch; docs/userland/shell.md "The shell in a session" keeps
STEWARD2's wording (the steward starts it on the UART and per SSH login) and says launching is
tested under a tester in the steward's place, naming bench:beamlet-launch beside the host test;
docs/plan/m1-separation.md takes the cases' progress text, and keeps main's "Not built" line
(walfsd is built on main). Folded into the commit, as the handoff asked: GETTING-STARTED.md "The
shell" no longer says "no boot runs them"; beamlet_launch.erl prints exec's whole result before
matching it. Session sizing checked against the tip: image/manifest.json sizes.session.pages is
10880 = the tester's SESSION_PAGES (the 11,008 on main is beamlet's own boot budget, not a
session's).

Gates on f3c06d547 (prebuilt rv64 227 cases, rv32 213, rc 0):
- `make -k -f scripts/jobs.mk -C .worktrees/BEAM4-cases <arch>/<case>`: PASS both widths
  beamlet-natives, beamlet-serve, beamlet-launch, beamlet-natives-attack (rv64 3.9/10.0/3.6/5.0 s).
- Smoke set: PASS both widths beamlet-files, beamlet-console, beamlet-boot, beamlet-footprint,
  aio-many-reads, init-boot, ipc-outcomes, bench-net-peer; PASS rv64 userland-boot. FAIL rv32
  userland-boot (450 s, timed out waiting for the console session's `UndefinedFunctionError`
  line): the known STEWARD2-base failure the launch named, fixed by BEAM9 (a one-page consoled
  share pinned by a paged read), not chased here.
- PASS rv64 docs, formatting, size-budget, unsafe-budget, no-cruft (no Rust changed: tests and
  pages only, plus the tester already reviewed on the old base).
- Host: `q run --quiet -- cargo test -p beamlet-redoubt --features fake` from userland/otp, 49
  passed, 0 failed (console 10, files 16, limits 7, lookup 2, pack 2, system 10, userland 2); the
  BEAM10 flake did not show in this run.

Summaries checked: README.md (no claim about the natives: unchanged); GETTING-STARTED.md
(updated above); docs/plan/m1-separation.md (updated); docs/userland/README.md (session wiring is
STEWARD2's: unchanged); docs/servers/wire.md "Generated clients" (names bench:beamlet-natives,
carried by the auto-merge); docs/userland/beamlet.md "beamlet on Redoubt" status (names
bench:beamlet-natives, auto-merged, no longer says the natives are host-only).

Next: STEWARD2 rebases once more over BEAM9's merge; then `git rebase --onto <new tip> 10bda633a
wp-BEAM4-cases` (one commit, the same four status lines may conflict again), `prebuilt`, the four
cases both widths, rv32 userland-boot expected to pass then.

### Red's note folded (2026-10-07): the tester carves as the steward does

Head cb6ed7944 (still one commit on 10bda633a; f3c06d547 amended). Changes:
- beamlet-session carves `users` as the steward does (servers/steward.md, "Fixed sub-budgets per
  label set"): alice's top budget (account 1001, 32,768 pages, 8 processes, weight 1000, as the
  four manifests now say; was 16,384), under it one sub-budget per label set the case names (an
  equal share less the cost, that set's labels: adding one is the steward slot's system-class
  power), and from that each session (10,880 pages, 2 processes, weight 100, the set's exact
  labels). `labels=N[,N]` in the args file sets the following VMs' label set. The manifests'
  `sizes.session` is 10,880 too, so init's share check and the tester agree.
- Grants: a session gets `/dev/cons`, `/boot` + `bootfsd`, `erofsd:system` (all fresh
  connections) and its home at `/home/alice` (a fresh `littlefsd:data` connection; the cases'
  volume's root is the home), as the steward gives them. Dropped by name: `littlefsd:data`
  everywhere, `keyd` except in beamlet-natives, where the `keyd` word hands it: the generated
  client's call needs a typed server, and consoled serves no `consol` call (size/resize are
  sshd's), the steward's own `steward` handle has no server with a tester in its place. `service`
  stays in beamlet-serve: an endpoint the VM serves must come from its launcher.
- beamlet-natives: `ns` is now `/dev/cons /boot /home/alice budget bootfsd erofsd:system keyd`;
  it binds its home at `/mnt`, writes through the bind and reads back through the home.
- beamlet-natives-attack: the unlabelled read VM asks for a child with `labels => [7]`:
  `{error,class_denied}`, the kernel's (a user-class caller may not add a label). A third VM in
  alice's labelled session {7} runs nothing: `Console::open` is ORDWR, the 9P skeleton's label
  check refuses a labelled caller's write-open of the UART console (consoled R69), beamlet
  returns 1 before any line; the tester's `started ..., labels [7]` and `ended: Exited, code 1`
  are the expects, and `attack: labels:` is forbidden. Found on the way: a labelled session
  cannot run beamlet on the UART at all (vault sessions are SSH's), and `shed_label`
  (`label_denied`) cannot be shown from a session that can print.
- Not done, and why: a steward-decided refusal. The tester cannot stand in for the steward's
  decisions without the verdict becoming the tester's (rule F); a session's `steward` handle
  needs the real server. It stays the open risk already listed: the steward's own sessions with
  these natives are STEWARD2's follow-up.
- Also noted: `budget_create` with `labels` left out passes an empty set, so in a labelled
  session every child carve is `label_denied`; the page says labels may be left out without a
  default. A follow-up for the natives (default to the VM's own labels), not folded here.

Gates on cb6ed7944 (prebuilt rv64 227 / rv32 213, rc 0): PASS both widths beamlet-natives,
beamlet-serve, beamlet-launch, beamlet-natives-attack (rv32 did not meet BEAM9's stall: these
sessions' consoles are minted once); PASS rv64 docs, formatting, size-budget, unsafe-budget,
no-cruft. Smoke set and host suite unchanged since f3c06d547 (no library code changed: the
tester, the cases' files and one page).

### Rebased onto STEWARD2's final tip (2026-10-08)

Head b1103a365: one commit (26 files) on wp-STEWARD2's final tip c3567f923 (on main cc51f76ad),
`git rebase --onto c3567f923 10bda633a wp-BEAM4-cases`, cb6ed7944 before. Two conflicts, both
pages: docs/plan/m1-separation.md keeps the base's new steward and files bullets (the milestone
split) with the cases' natives sentence; docs/userland/beamlet.md "Natives" status, the base's
list grew by one host test, so the count is 30 (26 + the four bench cases). Nothing else moved.

Gates on b1103a365 (prebuilt rv64 229 / rv32 215, rc 0; jobs.mk):
- PASS both widths: beamlet-natives, beamlet-serve, beamlet-launch, beamlet-natives-attack.
- Smoke set PASS both widths: beamlet-console, beamlet-boot, beamlet-footprint, aio-many-reads,
  userland-boot (rv32 now passes: BEAM9 is in the base), init-boot, ipc-outcomes, bench-net-peer;
  beamlet-files PASS rv64; rv32 FAILED once in the chain (`{'EXCEPTION',error,{badmatch,{error,
  eexist}}}` at `prim_file:make_dir("/home/alice/d")`, after every earlier line passed) and
  PASSED twice rerun alone. Not this branch's files (walfsd, the base's beamlet_files.erl); the
  base's new "a file request answered busy goes again after the retry interval" (45f1880bc) is
  the suspect: a retried mkdir whose first try had landed reads eexist. Named here, not chased.
- PASS rv64 docs, formatting, size-budget, unsafe-budget, no-cruft.
- Host: beamlet-redoubt --features fake 55/55 (console 13, files 18, limits 7, lookup 2, pack 2,
  system 11, userland 2), q --quiet, rc 0.

Run logs: /home/mcloonan/redoubt/.tmp/BEAM4c/ (beam4c-*.log: prebuilt, cases, smoke, gates,
host suites), moved there from /tmp.

### Rebased onto main df5705f10 (2026-10-08)

Head 572d72fce: one commit on main df5705f10 (STEWARD2 merged; its final fold rewrote the
commits under the branch). `git rebase --onto df5705f10 c3567f923`, no conflict; range-diff
against b1103a365 differs in context only. Gates (prebuilt rv64 229 / rv32 215): PASS both
widths beamlet-natives, beamlet-serve, beamlet-launch, beamlet-natives-attack; PASS docs. Log:
/home/mcloonan/redoubt/.tmp/BEAM4c/main-rebase.log.
