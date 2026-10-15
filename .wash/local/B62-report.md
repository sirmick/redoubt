# B62 report (b51-implementer, 2026-10-15)

Node B62: one clock in the files layer, `FILE_WAIT_US`, off the VM's thread (the Architect's
ruling, `.wash/local/B62-ruling.md`, thread B62-design). Worktree
`/home/mcloonan/redoubt/.worktrees/B62`, branch `wp-B62` from the train head a22f121cd (B61 and
B51 in it); to be rebased onto main when train 27 publishes, on the orchestrator's word.

## What was built (one commit)

- `userland/otp/redoubt/src/files.rs`: `pub const FILE_WAIT_US: u64 = 10_000_000` (its doc says
  it is `COLLECT_WAIT`'s value and why). `Op` gains `hub: Option<Conn>` (an operation parks
  without a hub connection while every waiter's place is held), `deadline: Option<u64>` (set at
  a request's first send, cleared by a non-`busy` answer; set at the operation's start while it
  wants a place, cleared when it gets one) and `busied: bool` (the request out was answered
  `busy` at least once). `begin` no longer connects: it takes the fid and starts the op with
  `hub: None` and the place deadline. `go_on` connects first when the op has no hub: on
  `TooManyThreads` before the deadline it parks with `retry_at = now + RETRY_US` (the `busy`
  machinery; `pump` sends it again; `retry_in` wakes the VM for it); past it, or on any other
  refusal, the result is `emfile`/`enomem`/`eio` as before, never on the VM's thread. On a send
  the deadline is set if unset. `pump` runs an expiry pass first: `give_up` names what was
  waited for — no hub: `Emfile`; `busied`: `Ebusy`; else `Etimedout`. A request in flight at the
  bound (first send, or a retry not yet answered) has its `requests` entry dropped, so a late
  answer finds no owner (`take` returns), and its fid is retired (`walked = false`, no clunk) as
  `Reply::Lost`'s is; a request parked for its retry was never taken: a walk's fid is freed, a
  walked fid is clunked by `go_on`. `retry_in` includes deadlines. `opened` reads the hub from the
  `Option` (an answer implies one).
- `io.rs`: `Io::connect` at `MAX_WAITERS` does `take_waiting` + `reap` and then refuses
  `TooManyThreads` at once; B61's `hand_over` on that path is gone (`hand_over` stays for
  `forget`, `HAND_US` unchanged). Docs of `MAX_WAITERS`, `connect` and `forget` say so.
- `lib.rs`: `pub use files::{FILE_WAIT_US, posix}`.
- `vm/src/platform.rs`: `FileError::Ebusy` → `"ebusy"` (no other exhaustive match on `FileError`
  in the tree: checked by grep; `cli/src/files.rs` maps the other way).
- `fixture.rs`: the sink's server gets `requests_wait(FOREVER)` with a comment: a held read waits
  as long as the test holds it, so the server answers no `timeout` of its own (its default was
  `COLLECT_WAIT`, the same 10 s, which would have raced the client's clock).
- Tests (`userland/otp/redoubt/tests/system.rs`): the two hub tests rewritten and two new, as the
  ruling names them (below). `a_write_answered_busy_goes_again_after_the_retry_interval` stands.
- Pages: `docs/userland/files.md` "Everything moves in pieces" (the ruling's sentence) and its
  status list (+2, 26 → 28); `docs/userland/beamlet.md`: the bounds table's `FILE_WAIT_US` row,
  and "Threads" rewritten so B61's and B51's gap statements become the rule (the hand-over wait
  is the operation's, off the VM's thread; the two old failures named as history; the dangling
  "(open; in the residuals)" pointer gone — beamlet.md has no residuals section). `native.md`
  mentions neither `HAND_US` nor the files' waits: nothing to change. `tests/pipe-sixty-four.toml`:
  the MAX_WAITERS paragraph rewritten; the case stays in `scripts/jobs.mk`'s quiet list until the
  measurement below says otherwise.

Not in: `posix(Name::Busy)` still maps to `Eio` — a `busy` Rerror never reaches `posix` (the hub
turns it into `Outcome::Busy` first), so the ruling's `Ebusy` is minted only by `give_up`.

## Tests

| test | what it shows | time |
| --- | --- | --- |
| `a_fresh_connection_at_the_bound_waits_for_an_ended_waiters_hand_over` (rewritten) | six places held by servers that end 100 ms later; the fresh server's `make_dir` answers `Later` within `HAND_US` (asserted < 1 s; it is µs), the operation completes once the hand-over comes (≥ 100 ms) | 0.1 s |
| `a_fresh_connection_past_the_bound_is_refused_emfile_and_the_live_ones_still_serve` (rewritten) | six live servers hold every place; the seventh's `make_dir` fails `Emfile` at ≥ `FILE_WAIT_US`; the live ones still serve | 10 s |
| `a_request_unanswered_past_file_wait_fails_etimedout_and_the_connection_still_serves` (new) | the sink holds a read; the read fails `Etimedout` at ≥ `FILE_WAIT_US`; the hold is released (the late answer is dropped); a write on the same connection is taken | 10.2 s |
| `a_request_busy_past_file_wait_fails_busy` (new) | the sink answers every write `busy`; the write fails `Ebusy` at ≥ `FILE_WAIT_US` after more than one try; after a settling 200 ms the sink's write count stops rising | 10 s |

The fake kernel's clock is real time (`boot.elapsed()`), so the three bound tests cost 10 s each
(the suite went from ~25 s to ~33 s). A first draft of the `busy` test flaked once under the
whole suite: the deadline fell while a retry was in flight and `give_up` named it `etimedout`;
the `busied` flag fixes the name (a share is a share whether the retry is parked or in flight),
and the test counts the sink's writes only after a settling idle.

## Gates (through q; the beamlet workspace commands in a subshell)

- `cargo test -p beamlet-vm -p beamlet-redoubt --features beamlet-redoubt/fake`: 189 passed,
  0 failed, twice in a row (after the `busied` fix; before it 25/26 in system.rs with the flake).
- `cargo check` of both crates with `--tests`: clean (vendor `num-traits` deprecation warnings
  only). `cargo +nightly fmt -p beamlet-vm -p beamlet-redoubt -- --check`: clean.
- docs PASS (5.0 s); size-budget PASS (the userland crates are not budgeted);
  `./build --arch rv64|rv32 --programs` rc=0 before the `busied` fix — rerun below.

## Rebased onto main 1dd0acb27 (train 27 published)

**wp-B62 = bd984f751 on 1dd0acb27**, clean (9 files, 277+/66−). Gates there (q): beamlet-vm +
beamlet-redoubt (fake) 189 passed / 0 failed; fmt --check clean; docs PASS; size-budget PASS;
`./build --arch rv64|rv32 --programs` rc=0.

## Review (steward-red-2: OK with notes) and the docs amend

**wp-B62 = d60ec4f78 on 1dd0acb27** (docs-only amend into the one commit; 10 files, 293+/76−).
P2-1: native.md "Standard input and output, and pipes" and beamlet.md's open-items bullet no
longer state B61's VM-thread second or call the gap open: the operation parks off the VM's thread
for `FILE_WAIT_US`, the old refusal named as history. files.md gains the residual: a timed-out
request retires its fid for the connection's life, bounded by `MAX_FIDS` (64) a connection; and
P3-1's note as a sentence there: the console's own waits (lib.rs:444, :519) consult the console's
retry, not a file operation's deadline, so a timed-out operation is noticed when the VM next
idles or takes a completion, bounded by the console's progress (no code change). docs PASS.
Frozen for train 28.

## The measurement: pipe-sixty-four on rv32 beside load

The ruling: the case leaves the quiet class once it has passed five times beside a whole bench
on rv32. Train 27's bench was in its quiet tail when B62's prebuilt was ready (08:11Z), its boot
phase over, so the general-core load is mine (tenants running kernel-containment,
beamlet-reduction-rate, sched-*, steward-restart and the host-test cases in rounds); the case
itself runs on the quiet set as its class says (`make rv32/pipe-sixty-four`), and each run's
`q ls` header is noted before it. Logs: `$REDOUBT_TMP/B62/pipe-sixty-four-rv32-<n>.log`.

| run | general cores free before it | verdict |
| --- | --- | --- |
| 1 | 19 (no load yet; the bench's quiet tail on the quiet set) | PASS 16.5 s |
| 2 | 19 | PASS 17.0 s |
| 3 | 7 | PASS 16.6 s |
| 4 | 7 | PASS 16.5 s |
| 5 | 11 | PASS 16.6 s |

Runs 1-5 were not beside a saturated machine (one load tenant is capped at half the general
cores while another waits), so five more followed, started once the header read at most 2 free,
with two more load tenants (scratch worktrees at a22f121cd, their prebuilts then their case
rounds) beside the first:

| run | general cores free before it | verdict |
| --- | --- | --- |
| 6 | 0 | PASS 18.3 s |
| 7 | 0 | PASS 17.6 s |
| 8 | 4 | PASS 18.4 s |
| 9 | 4 | PASS 17.6 s |
| 10 | 4 | PASS 17.7 s |

10 of 10 PASS on rv32; the five under load (runs 6-10) at 0-4 free general cores. This is load
of mine, not a whole bench's boot phase (the ruling's words), so the case stayed in the quiet
class for B62 itself. The three load worktrees are removed; the B51 worktree was removed under
my first load loop by B51's merge (it died harmlessly).

### The measurement the rule asks for: beside train 28's bench

B62 accepted and merged as train 28 (19d158892); its whole bench's boot phase in
`.worktrees/train-20` is the load. Prebuilt of wp-B62 d60ec4f78 rebuilt (08:26Z), the runs started
when the header read ≤ 6 free (`make rv32/pipe-sixty-four`, quiet class as then, through q);
before each run the header and the bench's tenant line:

| run | general cores free | train-20 (running/cores, waiting) | verdict |
| --- | --- | --- | --- |
| 11 | 0 | 7/20, 25 waiting | PASS 17.1 s |
| 12 | 0 | 13/20, 18 waiting | PASS 17.2 s |
| 13 | 5 | 8/15 | PASS 17.4 s |
| 14 | 2 | 10/18, 1 waiting | PASS 16.7 s |
| 15 | 2 | 9/18 | PASS 17.1 s |

5 of 5 beside the bench's boot phase (08:28-08:30Z, its densest hour). Logs
`$REDOUBT_TMP/B62/pipe-sixty-four-rv32-11..15.log`.

## Follow-up: wp-B62b, the case leaves the quiet class

Worktree `/home/mcloonan/redoubt/.worktrees/B62b`, branch `wp-B62b` from d60ec4f78, one commit
**5a8ea2754**: `scripts/jobs.mk` drops `pipe-sixty-four` from the quiet list (with its B61
comment); the toml's comment says the case runs on the host's clock as pipe-eight does (neither
sets `icount`; pipe-eight has always run in the boot class), that the wait is the operation's
now, and records the five runs against the 450 s deadline. `make -f scripts/jobs.mk list-classes`
in B62b: not in `quiet`; the boot class takes it. Run in its new class from B62b's prebuilt, both
widths, beside the bench (`make -f scripts/jobs.mk set CASES=pipe-sixty-four`: the boot class
gives the guest one general core, `--cores 1`, beside the bench; the quiet class gave it a core of
the reserved set):

| round | rv64 | rv32 | general cores free before |
| --- | --- | --- | --- |
| 1 | PASS 16.5 s | **FAIL 125.6 s**: forbidden `{54, {:error, {:refused, {:mkdir, :emfile}}}}` | 7 |
| 2 | PASS 15.9 s | **FAIL 450.1 s**: timed out waiting for `{:ran, 64, ...}` | 12 |

Logs `$REDOUBT_TMP/B62/pipe-sixty-four-rv32-newclass-fail1.log` (round 1) and
`pipe-sixty-four-rv32-newclass-fail.log` (round 2). The refusal, read: the case runs on the host's
clock; pinned to one general core beside a whole bench the rv32 guest is starved, so ten seconds
of host time are too few guest instructions for an ended piped's waiter to hand its end over, and
the fifty-fourth pipeline's fresh piped found every place still held at `FILE_WAIT_US` — the new
bound reached, by its own name, where the old 1 s one was; starved further (round 2) the sixty-four
pipelines did not finish in 450 s. On the reserved quiet core the same case passes in 17 s beside
the same bench (runs 11-15). So the class is the right one for a host-clock case, and
**pipe-sixty-four stays in the quiet class**: wp-B62b is withdrawn (worktree and branch removed;
its commit claimed what this trial disproves), `scripts/jobs.mk` and the toml stand as B62 left
them (the toml's comment already says the hand-over is load-bound and what would move the case).
The B62 ruling's "five rv32 runs beside a whole bench" are the 5/5 above, run as the quiet class
runs them; moving the case out of that class is a different question (a host-clock guest on a
shared core), not B62's.
