# SMP3 design checkpoint (reconciled with main cc51f76ad; no kernel edit yet)

## 1. The pick is bigger than one predicate

`ptable.rs`'s `ProcessState::Running(mask)` plus one `current_thread` per process assume one
hart runs a process: `leave_previous` sets `Ready` while another hart may still run P;
`activate_process_thread` panics on `Running` reached from another PID; `next_thread` skips
`p.running()`. Plan:

- The ready mask excludes **every** running thread; the hart blocks' `(pid, tid)` are the one
  truth. A process stays `Running` while any hart runs it; `current_thread` becomes per hart
  (`block.tid` already is).
- Then "has a runnable thread no hart is running" is `ready(b) > 0`, and `next_thread` loses
  its `running()` skip.
- `Queue::pick` keeps RECON1's ranks untouched: candidates in rank order, each asked `next`; a
  candidate with no thread that *is* running elsewhere is skipped (not descheduled); one running
  nowhere is descheduled as today. At one hart it is identical to today.
- Checked audit at every pick: each hart block's `(pid, tid)` names a `Running` process with
  `tid` not in its mask; every `Running` process has a hart; the running count equals the blocks.
- Wake IPI in `leave()`: the sum of `ready(b)` over the reconcile's `gained` budgets (frames the
  settle just wrote: no other frame read) `> usize::from(next.is_none())` sends `wake_idle`.

## 2. Shootdown callers

Main flushes by address and ASID; a hart that ran P and left owes the stale mask, so the targets
stay "harts whose block names P now".

| Site | File | Flush? |
| --- | --- | --- |
| `unmap` | mem.rs:1127 | shootdown(caller) |
| `set_flags` removing R/W/X | mem.rs:1146 | shootdown(caller) |
| `process_map` source | process.rs:547 | shootdown(caller) |
| `take_buffer` at send (`lend_out` clears VALID) | message.rs:959 | shootdown(sender) |
| `return_lend` (borrower's VALID\|S cleared) | message.rs:1682 | shootdown(server) |
| `free_abandoned_lend` (`unmap_from` clears a VALID\|S borrower entry) | message.rs:1706 | shootdown(server) — not in the brief |
| `terminate_process` (self-exit while a sibling runs elsewhere) | ptable.rs:764 | shootdown(self) — not in the brief |
| `kill_process` | ptable.rs:784 | SMP1's, unchanged |
| `drop_lent` (transfer delivery, abandon), `lend_back` | message.rs:1583, 1757, 1603 | none: the entry was invalid |

`fence.i` sites: `map_anon`, `map_fixed`, `set_flags` with EXECUTE, and `process_map` (the child)
call `shootdown(target)`; the routine fences before it acknowledges.

The checked build's `println!` in `shootdown` moves to destruction's caller (an unmap at every
call would flood logs); `smp-evict`'s expect keeps matching.

## 3. A hart shot down for a live process

`irq.rs:140` sends a shot-down hart to `kmain` through `switch_to_thread(KERNEL_PID)`, leaving
the thread's state as it was. For a live process the thread must go back ready: route it through
`activate_process_thread(.., KERNEL_PID, .., true)`; the trap is taken again when it next runs.

## 4. Model and oracle

- Model: untouched (single hart; the brief).
- Oracle: `parse()` rejects unknown kinds, so the new shootdown record (`S`: id = target PID,
  pass = asked harts << 8 | acknowledged harts) must be admitted and ignored by every check.
  `dump()` drops the hart byte (`as u8 as char`); add a sixth field, the hart, and let `parse`
  take five or six fields. `smp-fence`'s verdict is a new post_check over `S` and `K` with the
  hart field.

## 5. Cases

As briefed: `smp-shootdown` (unmap, lend returned, lend within one process; icount and mttcg
tomls; the negative `smp-no-shootdown`), `smp-fence` (trace build), the whole bench at
`--smp 2` on both widths. `tools/testbench/src/main.rs` needs the post_check hook (B7 hotspot):
I ask before editing it.

## Question

Does item 1 (the process state machine) stay inside SMP3, or do you want it carved out?
