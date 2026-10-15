# B50 report

Head 6d92ddb3e on wp-B50 (one commit; review notes P3-1, P3-2 folded: the leak's bound on beamlet.md, the pruning line on admit), base main c8422f745. Worktree /home/mcloonan/redoubt/.worktrees/B50, clean.

Gates: host `cargo test -p beamlet-redoubt --features fake --test system` 27 passed (new test among them), exit 0; `cargo check -p redoubt-beamlet-programs -p redoubt-fake-kernel` exit 0; docs case PASS (6.3 s); `make prebuilt` rv64 and rv32 0 failed; `pipe-stage-handle` PASS rv64 (4.8 s) and rv32 (4.1 s); `pipe-eight` PASS rv64 and rv32 (5.9 s each, pipe-stage changed); rustfmt check clean on beamlet-redoubt, redoubt-beamlet-programs, redoubt-fake-kernel; `git diff --check` clean. Not run: the whole bench, the unsafe ratchet case, the size budget case (no `unsafe` added anywhere in the diff: `git show HEAD | grep -c '^+.*unsafe'` is 0; no touched crate is in tests/size-budget.toml: userland/otp/redoubt, tests/beamlet-programs and libs/rt/fake are outside its paths, libs/rt's entry covers libs/rt/src only).

## Paths

- userland/otp/redoubt/src/serve.rs: `LAUNCH_BADGE`; each served `Thread` keeps `launches: Vec<(badge, Arc<Life>)>` and a `next` badge; `Served::admit(endpoint, budget, life)` mints the launch's send right from the receive right with `mint(badge, Some(budget))` and records it, pruning gone lives; `deliver` maps the wake-up's badge to a Life (`Thread::life`) and builds the request's handles with `Cap::returned(h, life)`.
- userland/otp/redoubt/src/system.rs: `Life::ended()`, `Life::gone` pub(crate); `Cap::received` removed (its doc folded into `returned`); `System::launch` runs every handle of `launch.handles` and `launch.namespace` through `admit` when the budget has a Life, holds the VM's minted copies as `Owned::new` and drops them (closing them) once `jobs.launch` has returned.
- userland/otp/redoubt/src/lib.rs: `pub use serve::LAUNCH_BADGE` for the test.
- userland/otp/redoubt/tests/system.rs: `what_a_launched_program_sends_to_a_served_endpoint_goes_with_its_budget_and_is_not_closed_again`.
- libs/rt/fake/src/lib.rs: `Mint` with `budget: Some(h)` stamps the handle with that budget (was unmatched: a panic).
- tests/beamlet-programs/src/bin/pipe-stage.rs: verbs `hand NAME` and `give NAME`, taken before any stream is opened.
- tests/pipe-stage-handle.toml: new boot case, both widths, pipe-eight's recipe and budget.
- docs/userland/beamlet.md: "Handles are resource terms": the residual becomes the rule, with the external-revocation residual left and the handle generation named as its guard "in work of its own"; the `launch/1` bullet gains one clause.

## Design choice

Under the Life, not eager close. The handles such a program sends are stamped with its budget at best (its `endpoint_create` and its mints take the caller's budget, objects.md R9), so by the time the VM could close them eagerly the kernel has already freed their indices: an eager close would be the stale close itself. Under the Life they are marked gone at `budget_destroy` and never closed, the rule B47 set for a served server's returns. A handle the program forwarded rather than minted (a startup handle) is kept, never closed: the same leak-not-close trade the page already states for servers.

The badge is minted narrowed to the launch's budget (`mint(badge, Some(budget))`, kernel: "a budget handle only narrows"), so after the destroy no copy of that badge exists anywhere and the table entry can be dropped. One race remains: a request the serve thread forwarded before the destroy may still be in the VM's wake-up queue when the entry is pruned, and its handles are already freed indices. So `Thread::life` treats any badge in [LAUNCH_BADGE, SELF) that is not in the table as a gone launch's (an ended Life): kept, never closed. A stranger who held the receive right and minted a badge in that range gets its handles kept too, a leak inside the session's table and never a close. Stated on the page.

Namespace entries are treated like named handles: a served receive right passed as a namespace entry would be the same hole, and it is the same two lines.

The badge counter is per served endpoint, from 0x8000_0000 up; past SELF a launch is refused `too_many` (2^31 launches on one endpoint).

## Case design

A session on the machine has no receive right of its own: no server hands one out and there is no `endpoint_create` native. So `pipe-stage hand`, launched with `serve`, answers one call with a copy of its receive right (an endpoint the VM made, charged to the VM's budget, so it outlives the stage's budget); the session serves it. Then `pipe-stage give`, launched in a carved budget with that right among its handles, makes an endpoint in its budget and sends its receive right back. The session reads the request's badge (`>= 0x80000000`), destroys the budget, carves another (the kernel gives it the freed index), collects the term, and the second budget's `usage` and `destroy` are `:ok`; then a pipeline runs and the session's budget is as before. Without the fix the stage gets the receive right itself; with the old bookkeeping the collection closed the second budget's handle (the pipe-eight repro's shape).

First run hung at step 3 on both widths: the shell evaluates each line in a fresh process (Redoubt.Shell.Evaluator), and a served endpoint's requests go to the process that served it, so `serve` and the `receive` must be one line. Fixed in the case; the comment says why.

## Summaries checked

- docs/userland/beamlet.md "Handles are resource terms", `launch/1` bullet: updated (above). Natives status list unchanged: the paragraph names its tests inline, as B47's do.
- docs/userland/native.md "Launching from a session": "every authority the child gets is on that one call" still holds (the child gets a narrower handle than it was named); no change.
- docs/servers/piped.md, docs/kernel/objects.md (mint, R9): the kernel rules used, unchanged.
- README.md, GETTING-STARTED.md, userland/otp/README.md, docs/plan/m2-usable-shell.md: grepped for stale close, reused index, closes nothing, handle generation: no claims; no change.
- tests/size-budget.toml: no entry for any touched crate; no change.
- libs/rt/fake module doc: lists `mint` among what it models; the budget arm is a code comment; no change.

## Open

- The owner's handle-generation node remains the construction fix; the page says so without naming it.
- Reviewer (steward-red-2) might want: the stranger-badge leak bound (none: each such request leaks its handles until the VM ends, as a forwarded handle does).
