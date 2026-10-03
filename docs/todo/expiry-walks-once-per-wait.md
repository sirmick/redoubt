# Expiry walks once per wait

## What

A timer interrupt ends the waits whose timeout has come ([timer](../kernel/timer.md)).
`time::expire_due` (`kernel/src/time.rs`) finds them one at a time: each turn of its loop calls
`message::next_timeout`, which walks every live thread of every process whose earliest timeout
has come and returns the one earliest due wait, and the loop ends that wait and walks again. A
deadline that ends R waits in processes of T threads each costs R walks of up to R x T threads,
before any pump the ended waits make
([delivery walks every thread](delivery-walks-every-thread.md)).

Measured by `bench:worst-walk`, every PID in use, each process in a budget of its own holding
`MAX_THREADS` threads, when one deadline ends 250 waits at once (rv64, checked build, net of its
audits, 129,796 live threads across 510 processes): the interrupt takes 28.9 s. The trace does not
part the walks from the pumps inside them. An estimate from the code, not a measurement: each walk
sets a process's earliest timeout from the waits it still has, so a process leaves the walks once
its wait has ended, and the 250 walks visit 250, then 249, and down to one due process, each of
255 threads: about 250 x 251 / 2 x 255, 8 million thread visits. rv32 is not measured: the case
runs on rv64 alone.

## Why it matters

The interrupt is kernel time, not preemptible, and every wake on the machine waits for it. Its
cost follows how many waits share a deadline, squared, which any processes that agree on one
instant choose, not the work any one of them asked for
([R12 (scheduling)](../kernel/scheduling.md#r12-scheduling)).

## Where

- `kernel/src/time.rs`: `expire_due`'s loop.
- `kernel/src/message.rs`: `next_timeout`.

## Done when

- One walk collects every wait that is due, in the order the timer ends them (earliest first,
  a timeout before a deadline at an equal instant), or the due waits come off a list kept by
  deadline, so that ending R waits costs one walk, or R steps, not R walks.
- The order and the billing of each ended wait are unchanged: the timer's cases and the
  scheduler oracle pass as before.
- The interrupt that ends 250 waits at once is far under 28.9 s at full occupancy, apart from
  its pumps, on both widths.
- [Timer](../kernel/timer.md#residual-risks)'s residual says what the code then does.
- This page is deleted.
