# B62 ruling: a file operation's request waits a stated time, off the VM's thread

Thread `B62-design`. Evidence: `.wash/local/B61-report.md` "Gates" (pipe-sixty-four rv32 FAIL at
44.0 s, pipeline 14 `{:refused, {:mkdir, :emfile}}`); `.wash/local/B51-ruling.md`.

## What the two gaps are

B61's wait is on the VM's thread: `Io::connect` at `MAX_WAITERS` blocks every Erlang process up
to `HAND_US` for one ended waiter's hand-over, then refuses by the bound's name though no live
server holds a place. Beamlet's rule (beamlet.md "What waits on the VM's thread is bounded and
named") is kept to the letter and broken in spirit: `HAND_US` is the bound for a thread of the
platform's to take its work, not for a server's death to be noticed under load.

B51's residual does not reach beamlet the way the question supposes: beamlet never calls
`Hub::wait`; each connection has a waiter thread (io.rs:89), and the VM idles on its own wake
endpoint with its own timeout (`Io::wait`). A completion call outliving its hold stalls a waiter,
not the VM. What B51 leaves beamlet is this: a request in a stalled or wedged server has no bound
at all on the client, so the Erlang process that asked waits for ever, and a request answered
`busy` is retried every `RETRY_US` for ever (files.rs:291-296).

So both gaps are one: **the files layer has no clock on what an operation waits for.**

## The rule

Every file operation's request is bounded by one platform constant, `FILE_WAIT_US` = 10 s
(`COLLECT_WAIT`'s value: the file server's own longest wait for a request, past which it would
have answered `timeout` had it been running; beamlet.md's table of bounds gains the row).
The clock runs per request, from its first send, busy retries counted; and from the operation's
start for a connection's place at `MAX_WAITERS`. At the bound the operation fails to its asker
with the name of what it waited for, and nothing else changes:

| waited for | fails as | and |
| --- | --- | --- |
| an answer | `Name::Timeout` → `etimedout` (files.rs:897, the server's own `timeout`) | a later answer is dropped (`take` finds no owner); the request's fid is retired as `Reply::Lost`'s is (files.rs:298-303); the connection is kept |
| a share (`busy` again and again) | the platform's `busy` → `FileError::Ebusy` | the retry stops |
| a place at `MAX_WAITERS` | `emfile` | the operation never held one |

Nothing waits on the VM's thread for any of these. `Io::connect` at the bound does not block:
the operation parks wanting a place (as a `busy` one parks wanting its retry) and is connected
when a hand-over frees one (the VM's loop already takes hand-overs on its wake endpoint:
`Io::take_waiting`, `Io::wait`), or fails at its deadline. `HAND_US` stays what it is for the
pool's and the jobs' hand-offs; B61's `hand_over` on the connect path goes.

Answers to the four questions:
1. The deadline is the platform's, one constant, like `MAX_CALL_MS` and `REQUEST_WAIT_US`. Not
   the Elixir caller's: `File.*` is OTP's API and takes none (Redoubt.Shell stays a strict
   subset); not the process's receive timeout: the process is suspended in the call. Not the
   session's: one slow volume must not fail the console's reads.
2. The operation fails, named, the connection kept. Never refused early (a hand-over usually
   comes within microseconds), never handed back (a place is given up only by an ended server's
   waiter: B47's invariant stands).
3. Untouched: R28, the server's hold and its request deadline (`requests_wait`), the hub's
   completion call (B51's ruling). The client's clock is the client's; where both run the two
   agree at 10 s.
4. Below.

## Page sentences

files.md "Everything moves in pieces" bullet, after "so only the Erlang process that asked waits
for it.":

> It waits at most `FILE_WAIT_US` (10 s, the file server's own longest wait for a request) for
> each request, `busy` retries counted, and for a connection's place at `MAX_WAITERS`; past
> that the operation fails with the name of what it waited for (`etimedout` an answer, `busy` a
> share, `emfile` a place), the connection kept and a later answer dropped. None of it waits on
> the VM's thread.

beamlet.md's bounds table, a row:

> | `FILE_WAIT_US` | 10 s a file operation waits for each request's answer, for a share, or for a waiter's place | [`files.rs`](../../userland/otp/redoubt/src/files.rs) |

beamlet.md "Threads" (where `MAX_WAITERS` is stated) and io.rs:66-70: the sentence that a connect
at the bound waits for a hand-over becomes "an operation that finds every place held parks until
an ended server's waiter hands one over, or fails `emfile` at `FILE_WAIT_US`". B61's io.rs doc
line about `HAND_US` on this path goes with the code.

## The cases

Host (`userland/otp/redoubt/tests/system.rs`, fake kernel):
- `a_fresh_connection_at_the_bound_waits_for_an_ended_waiters_hand_over` is rewritten: the
  connect returns `Later` at once (the VM's thread is not held: assert the call's elapsed time
  is below `HAND_US`), the operation completes when the hand-over arrives.
- the Emfile test (six live, a seventh): fails `emfile` at `FILE_WAIT_US` on the fake clock, the
  six still served; no `>= 900 ms` assertion.
- new `a_request_unanswered_past_file_wait_fails_etimedout_and_the_connection_still_serves`: a
  fake server takes a request and never answers it; the op fails `etimedout` at the bound; the
  late answer, delivered after, is dropped; the next op on the connection succeeds.
- new `a_request_busy_past_file_wait_fails_busy`: a fake server answers `busy` for ever; the op
  fails at the bound, and retries stop (`busy_answers` stops rising).
- `a_write_answered_busy_goes_again_after_the_retry_interval` stands.

Machine: `pipe-sixty-four` leaves the quiet class when the implementer has run it five times
beside a whole bench on rv32 with no failure (B61's 1 in 3 was the 1 s bound under load; the
hand-over itself is still load-bound, so measure, do not assume). Its toml comment's
"MAX_WAITERS" paragraph is rewritten to say what now bounds a fresh piped's place.

## Not ruled here

A late answer that *succeeded* on the server (a write past the bound) leaves the file as the
server made it: inherent to any client bound, as the server's own `timeout` already is; files.md
need not say more than "a later answer dropped".
