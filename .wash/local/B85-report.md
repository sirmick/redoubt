# B85 report (b85-implementer)

Branch `wp-B85` on main c63bbacbf. Reviewer: kernel-red-3.
`git status --short | wc -l` = 0 after commit.

## The wait that ran on the host's clock

`input_nobody_reads_holds_no_idle_and_waits_for_the_next_reader` idled on a 200 ms **host-clock**
deadline (`p.idle(Some(now+200_000))`) and asserted the idle slept the whole 200 ms. Under a whole
bench's load the console read's submit (`SUBMIT_TIMEOUT_US`, 1 ms on the host) ran out before the
fixture server took it, the read was queued, and the hub's `RETRY_US` wake returned the idle early
(`monotonic_us() < deadline`) → `:154` "the idle returned on input nobody reads"; the next
`console_read` then returned the queued `y` → `:56` unwrap on `Err`. Reproduced 1/80 at 24 copies,
1/60 at 48 copies on a 4-core lease (`$REDOUBT_TMP/B85/load2`, `cbload`).

Named waits on the host's clock here: the idle's `deadline`, the read's `SUBMIT_TIMEOUT_US`/`RETRY_US`,
the write's retry (`a_write_answered_busy…`), and `a_vm_busy…`'s 12 s host sleep across the session bound.

## The fix (held clock, like B81 did for system.rs)

`console.rs` holds the fake's clock (`Fake::hold_clock`, serialized by a `turn()` mutex as system.rs).
Each deadline is a step a test takes: `idle_to` (a helper thread ticks once the VM waits — guards
"no early wake", used by `input_nobody_reads`), `idle_past` (tick then idle, for a wait only the
clock ends), tick+look loops for the busy-retry and the session-bound crossing. `a_vm_busy…` jumps
to just under the bound then crosses it a `RETRY_US` at a time, so the server answers its parked
completion call a small overdue (its reply still reaches the waiter) — a single large-overdue jump
drops the session under load.

`fixture.rs`: the input `feed` thread is a counted fake thread only while it wakes the server
(`as_process` around each send), not while blocked in the host pipe read — else a held-clock tick
would wait forever on a keyboard nobody types on.

Shipped beamlet code is unchanged. Suite 16/16 in 0.03 s (was 12.0 s on the host clock).

## Gates (all via jobs.mk / q)

- beamlet-lookup-host: PASS ×5 (53–57 s; `case-1..3`, `finalgate`). 5×5 alone green.
- Under load: `input_nobody_reads` 0 failures across 24/48-copy sweeps and the 4-copy full-case
  gate (`g4`, `g45`); `a_vm_busy` 0 failures at 4 copies.
- beamlet-lookup-cli-host PASS, docs PASS, formatting PASS, size-budget PASS, unsafe-budget PASS.

## Pages

- `docs/testbench.md` "On a shared host": the held-clock paragraph now names `console.rs` beside the
  system tests. `docs/userland/beamlet.md` status line: tests still "run on the host, on the fake
  kernel"; no clock claim, unchanged (docs gate green). README/GETTING-STARTED: no mention.

## Residual (pre-existing mechanism limit, not B85)

Under ≥4 concurrent copies of the case on one 4-core lease (B81's stress profile, far beyond the
bench's one copy), a held-clock test can hit the 60 s stuck guard when the platform's resize thread
(a `FOREVER` `resize` call parked at the server) is CPU-starved — same class as B81's `system.rs`
failing under the same stress (confirmed with my `feed` change reverted: `servers_bound_and_ended…`
and `_past_file_wait` still flake). The bench runs one copy, where this does not occur; the gate
case passed ×5. A general fix (patient person-waits without an infinite hang, or joining a test's
platform threads) belongs to the fake kernel / fixture, not this package.
