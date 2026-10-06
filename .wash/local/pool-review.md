# The pool, measured on train 3 (Architect, for the owner)

Source: train 3 on 777bba164, worktree `.worktrees/train-3`, the 217 `target/jobs/rv64-*.log`
files (rv64 only: no rv32 log exists there), each log's modification time as the case's end and
the bench's own `PASS/FAIL … N.Ns` line as its case time, so a case's start is end − case time.
Process ages from `ps` at 13:01. Nothing below is a guess; where a number is derived, the
arithmetic is shown.

## 1. Where the 3 h 26 min went

The cases phase ran from 09:23:14 (first case start) to 12:49:08 (last end): 12,354 s.

| Component | Seconds | Share | How measured |
| --- | ---: | ---: | --- |
| Case time (QEMU running, 207 cases with a time; 10 failed or timed-out logs carry none) | 1,061 | 9 % | sum of the bench's `N.Ns` |
| Seven idle gaps over 300 s with no case running at all | 9,131 | 74 % | interval union of (start, end) |
| Small gaps between consecutive cases (about 210 transitions, ≈ 10 s each: the bench's start, cargo's lock, QEMU's start, the jobserver's locks) | 2,084 | 17 % | idle time less the seven gaps |
| Build phase before the cases (both widths) | ≈ 70 | – | `build-rv64.log` ends 09:21:45, `build-rv32.log` 09:22:15; the cases' `make` starts 09:22:15 |

The seven gaps: 09:27→09:49 (22 min), 10:10→10:42 (32 min: the member's `jobserver all` on a
shared case that closed the gate), 10:42→11:02 (20 min), 11:02→11:12 (9 min), 11:16→11:29
(13 min), 11:48→12:37 (48 min), 12:37→12:45 (8 min). Every gap ends when a shared case starts,
so each is a closed gate or an empty pool, not a slow case.

**Concurrency: never above one.** In every 10-minute sample across the 205 minutes, the number
of train cases running was 0 or 1, although `make` had 24 job slots. The pool's 24 tokens were
held by other members' jobs (`jobserver free` reads 0 all day), and a `take` waits behind any
pending `all`. The train's 217 cases therefore ran one at a time, and only when nothing
exclusive was pending: 1,061 s of work spread over 12,354 s.

**The bench itself is not the cost.** `cargo testbench --list` and `cargo build -p testbench`
take 0.1 s and 0.05 s when cargo's lock is free; the same `--list` took 15.3 s a minute earlier
under the day's load, waiting for the lock another cargo held. A 0.1 s case submitted through
`jobserver take` at 13:05 had not started after five minutes: the train's `all host-tests` had
held the gate since 12:50 (1,181 s at 13:10 and counting).

**The serial tail is one test.** `model-host-tests` runs `mutations_are_caught`
(`model/tests/mutations.rs:31`): one `#[test]`, one thread (`nlwp` 3, 99.9 % of one core),
looping over `Mutation::ALL`'s 147 mutations and running the contract suites for each. On this
host at 13:01: the train's debug run had been going 3 h 01 min (pid 970841) and K19's **release**
run 3 h 13 min (pid 902323), both still running. "45 min in release" is not what the machine
shows; the test is single-threaded, so release changes a constant, not the shape.

## 2. Is make + fifo the right design?

What the data says about the mechanism (`.wash/local/jobserver`, 140 lines, GNU make's fifo
jobserver plus two `flock`s):

| Observed | Mechanism behind it |
| --- | --- |
| 0 tokens free at load average 6.7 on 24 cores, all day | a token is held per *process* (`make` jobs and `rustc`s), not per core in use; a job blocked on a lock or waiting on a single-threaded child holds its token for hours |
| 74 % of the train's wall in gate closures | `all` closes the gate for every shared job on the machine, from any member; a 40-minute closure by one member's host test stopped the train |
| a QEMU with no token (train concurrency 1 beside 24 tokens) | `take` is best-effort (`read -t 0.05`), so the token says nothing about CPU, and `make` itself is what serialised the train, having no free job token |
| a `jobserver take sleep 1.5` alive for 20 h 49 min (pid 2672901) | a wrapper waiting on the gate or holding a token with its child gone; nothing reaps it |
| the model test on one core for 3 h while 17 cores idle | no class says what a job needs (a QEMU is one core at 100 %; a bounded host test is 4 threads; the mutations test is one) |

The design counts processes and stops the world for exclusivity. Both are wrong for this
machine: the work is 166 one-core QEMUs of a few seconds, 14 host-test crates, and a handful of
long serial tests, and the only true exclusivity is the host-clock class (wall-clock asserts),
which needs quiet cores, not an empty machine. **A small scheduler of our own is the right
shape**, and the shape is measured by the table above, not invented: one process per machine;
a queue with classes (`qemu`: one core each, pinned, N = cores − reserved; `host`: bounded
threads on the remaining cores; `clock`: the host-clock cases on reserved cores, serialised only
among themselves, never draining anything; `net`: the port lock as today); a lease bound to the
job's pid so a killed job returns its slot; members submit and wait, so a train and a member's
gate interleave by slot, not by a global stop. Its predicted gain is arithmetic from measured
case times (section 3); it is **predicted, not measured**, so the first step is a one-day
prototype replayed over train 3's 217 cases with the same logs, against the 3 h 26 min.

## 3. What a train's wall time should be at 24 cores

With 16 cores for QEMU and 8 for host work, from measured times:

| Phase | Today (train 3) | Arithmetic | Expected |
| --- | ---: | --- | ---: |
| Build, both widths | ≈ 70 s | unchanged | ≈ 70 s |
| rv64 cases (1,061 s case time + ≈ 210 × 10 s starts) | 12,354 s | (1,061 + 2,100) / 16, floor at the longest case (142 s) | ≈ 200 s |
| rv32 cases | not in this train's logs | same shape | ≈ 200 s |
| Exclusive tail (rt 7.6 s, client 4.2 s, r4 0.8 s, rt-miri 84.8 s, ssh-guest 1.2 s, loopback-deadlock 1.3 s; `host-tests` alone: no finished log) | serial | serial on reserved cores, beside the QEMUs | ≈ 100 s + host-tests |
| `model-host-tests` | ≥ 3 h 01 min, one thread | 147 independent mutations over 20 threads: 147/20 × ≈ 75 s | ≈ 9–10 min debug |
| **Train, both widths** | **> 6.5 h** (3 h 26 + the model) | | **≈ 20 min** |

The three changes that get closest, each with the measurement that justifies it:

1. **Parallelise `mutations_are_caught` across mutations** (one thread per mutation from a pool;
   the contract runs share no state: `ipc_contracts(Some(m))`). Measured: one thread at 99.9 %
   for 3 h on 24 cores. Expected: 3 h → ≈ 10 min. **This is the first change, before B18:**
   K19's release run of the same test is at 3 h 13 min, so release alone does not change the
   shape. B18 stays, as the second factor on the same test.
2. **End the drain:** host-clock cases run on reserved cores (`taskset`) serialised among
   themselves, and nothing closes a gate for everyone. Measured: 9,131 s of the train's 12,354 s
   were gate closures and empty pool.
3. **Admission by CPU, not by process:** one QEMU slot per core, taken blocking, leased to a pid;
   host jobs by thread count on the other cores; the small scheduler above. Measured: train
   concurrency 1 beside 24 tokens; the pool reported 0 free at load 6.7.

## 4. The tests, classified for the owner

| Kind | Count on train 3 (rv64) | Case time | What it proves |
| --- | ---: | ---: | --- |
| **Light: host unit tests** (a crate's `cargo test`: rt, client, littlefs, stride, blkd, init, …) | 14 cases | 178 s | each rule at its API, in isolation, with the fake kernel; fast and deterministic |
| **Light: short boots** (≤ 5 s of guest) | 149 | 135 s | one rule's attack on the real kernel and servers under QEMU: the bulk of the book's `bench:` lines |
| **Medium boots** (5–20 s) | 11 | 97 s | multi-server scenarios: ssh loopback, net attacks, init restarts |
| **Heavy: profiles and timing** (`userland-boot` 142 s, `boot-profile` 125 s, `boot-profile-unverified` 86 s, `sched-latency-tcg` 97 s, `sched-latency` 60 s, `sched-cluster` 22 s) | 6 | 532 s | boot time to the prompt; wake-latency distributions under icount; the scheduler's bounds |
| **Heavy: the containment gate** (`kernel-containment`, 192 MiB trace ring) | 1 | failed on R12's clause (B15) | nine hostile leases against every R10/R12 verdict at once, judged by the oracle |
| **Heavy: the model's mutations** (`model-host-tests`) | 1 | > 3 h, one thread | every modelled rule has a mutation the model catches: the book's `mutation:` lines |
| **Heavy: host-clock tails** (`rt-miri` 85 s; rt, client, r4 host tests; `bench-ssh-guest`) | 6 | ≈ 100 s | wall-clock bounds that need a quiet host; the only class that is truly exclusive |

The light classes are 174 of 217 cases and 313 s of case time; the heavy ones are the wall. A
train's time is the serial tail, not the light cases, and today the tail is one single-threaded
test plus the gates everyone else closes.
