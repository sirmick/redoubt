# K27 design checkpoint: what ends bob's idle VM under a neighbour's flood

## Evidence

- The kept rv32 flood log (/tmp/s2runs/rv32-steward-sub-budget-flood) is a PASSING run: its three
  ssh logs end "ssh exited (status 0)", bob printed 55, and the three `terminate_process` lines
  are the VMs exiting at the case's `exit`. No failing run's ssh log survives in any worktree's
  target/testbench.
- `terminate_process` is the kernel's self-end path, so it marks `run` returning, a normal end. An
  SSH session's VM says its last words on its channel, never the UART: "silent" on the UART is
  inherent to an SSH session, not a symptom.
- My instrumented probe (a scratch tree on wp-STEWARD2's tip: bob's VM says on its console why its
  read ended, with the hub's reason for ending the connection) ran the rv32 case once before the
  restart: a 2400 s timeout with the console VM never reaching its banner (the host was loaded
  with other tenants' benches): a starved boot, no verdict, 53 minutes. q's restart lost the loop.

## The mechanism, by construction

A VM idle at its prompt ends normally only when its console read ends. For an SSH session that
console is sshd's channel console, reached through the VM's hub:

1. The hub's completion call to the console carries the hold (10 s) and its own kernel timeout is
   the hold plus `COLLECT_MARGIN_US` (1 s): a timeout means "the server broke its promise", and
   the hub ends the connection for good (`aio.rs` `collected` -> `end`), every request outstanding
   coming back `Outcome::Ended`.
2. `ConsoleIo::take` takes the read's `Ended` (anything but `Read(n)` or `Busy`) as the input's end.
3. The shell sees EOF, exits as at Ctrl-D, `run` returns 0, the process self-ends
   (`terminate_process`), the steward reaps the budget, sshd ends the channel.

On one rv32 hart, a neighbour's flood (alice's allocation storm in the vault session, the fourth VM
booting) can keep sshd off the CPU for more than 1 s past the end of a hold. That fits "bob's idle
VM ended itself right after alice's vault login" and the 1-in-3 rate. A refused carve or page
cannot end an idle VM: it allocates nothing at the prompt, and `exec`'s refused carve already
returns an error.

## Proposal (Tier A: libs/client)

1. libs/client aio.rs: a completion call that times out does not end the connection for good. The
   hub reopens the session on the same endpoint (a fresh first completion call); the requests that
   were outstanding come back `Ended`, as now, since their fate is unknown. A server that ended the
   session itself (`ENDED`, a reply that is no answer, a protocol error) still ends it. ~20 lines.
2. beamlet ConsoleIo: only `Read(0)` is the input's end. A read that comes back `Ended` is reported
   once on the console ("beamlet: the console's session was lost; reading again") and asked again
   on the reopened session; a write that comes back `Ended` is sent again from its queued bytes
   (they stay queued until `Wrote`). The I/O report counts the losses.
3. Tests: libs/client host test (a server that stalls past the bound once keeps its client: the
   next request is served); beamlet console host test (the fixture console stalls once past the
   bound; typing after it still reaches the VM, and the VM is alive). The rv32 flood case stays
   STEWARD2's R37 verdict; I rerun it on a quiet host.

## Alternatives

- Evidence first: a failing bob ssh log from STEWARD2's red, if one survives, or the probe looped
  when the host is quiet (about 15 minutes a run when not starved).
- Raise `COLLECT_MARGIN_US` instead: narrows the window, does not close it, and a margin is the
  hub's promise detector (B20 tuned it).

## Evidence runs (after the orchestrator's "evidence first")

- Six parallel rv32 runs of the probe on wp-STEWARD2's old tip 0608b42bf (q run --cores 1 each,
  2400 s bound): all six FAILED the same way, early: the console session printed its banner and
  never its prompt, sshd logged no connection, and the case timed out waiting for alice's first
  login audit; no SSH session log was written. The same stall showed in the single run before q's
  restart. 0608b42bf carries BEAM9's fix (checked), so this is not BEAM9's stall; the suspect is
  the probe itself (it writes synchronously on the console from console_read) or the base.
- wp-STEWARD2 has since moved to 47a830958 (on main 26f79ed08). Rerun: the probe moved onto it
  (one conflict, the eof flag BEAM10 added, merged by keeping both), plus one unmodified baseline
  of 47a830958 in its own tree (.worktrees/K27-base), all seven in parallel. The baseline decides
  whether the stall is the probe's.

## Results on 47a830958 (STEWARD2 rebased onto main 26f79ed08)

- baseline (unmodified): 1/1 PASS, 11.9 s.
- probe, 6 in parallel (q run --cores 1): 6/6 PASS, ~11.5 s each; 0 failed completion calls; all 18 console reads ended Read(0) at the case's exit.
- probe sweep, 30 runs six at a time: 30/30 PASS; in the 8 run dirs the bench kept, 0 failed completion calls and 24/24 reads ended Read(0).
- The proposed mechanism (a completion call timing out at hold + margin, the read coming back Ended) never fired. K27 does not reproduce on this tip.

## The probe (scratch only, never committed), as a patch on 47a830958

```diff
diff --git a/libs/client/src/aio.rs b/libs/client/src/aio.rs
index bb5fc556e..29734d6cd 100644
--- a/libs/client/src/aio.rs
+++ b/libs/client/src/aio.rs
@@ -43,6 +43,14 @@
 use alloc::collections::VecDeque;
 use alloc::vec::Vec;
 use core::num::NonZeroU64;
+use core::sync::atomic::{AtomicU32, Ordering};
+
+/// PROBE: why the last connection ended: kind (1 call failed/no buffer, 2 reply words, 3 answers did not frame, 4 send failed), reply word 0, reply word 2.
+pub static PROBE_END: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
+/// PROBE: the last completion call that failed in a waiter: a sequence number (how many so far),
+/// the connection's index, the kernel status, the hold (ms) and how long the call took (ms).
+pub static PROBE_CALL: [AtomicU32; 5] =
+    [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
 
 use redoubt_rt::abi::{Error as SysError, FOREVER, MAX_LEND_PAGES, PAGE_SIZE};
 use redoubt_rt::handle::Endpoint;
@@ -335,9 +343,20 @@ impl Hub {
     /// Takes a completion call's reply words 0 to 2 (`None`: the call failed) and its buffer.
     fn collected(&mut self, c: usize, reply: Option<[u64; 3]>, buffer: Option<Buffer>) {
         // Failed, ended, refused, or a session the server opened afresh: what was outstanding is gone.
-        let (Some([0, bytes, 0]), Some(lend)) = (reply, buffer) else { return self.end(c) };
+        let (Some([0, bytes, 0]), Some(lend)) = (reply, buffer) else {
+            match reply {
+                None => PROBE_END[0].store(1, Ordering::SeqCst),
+                Some(w) => {
+                    PROBE_END[0].store(2, Ordering::SeqCst);
+                    PROBE_END[1].store(w[0] as u32, Ordering::SeqCst);
+                    PROBE_END[2].store(w[2] as u32, Ordering::SeqCst);
+                }
+            }
+            return self.end(c);
+        };
         let n = usize::try_from(bytes).unwrap_or(usize::MAX);
         if n > lend.len() || self.answers(c, &lend[..n]).is_err() {
+            PROBE_END[0].store(3, Ordering::SeqCst);
             self.end(c);
         }
         self.conns[c].lend = Some(lend);
@@ -419,7 +438,10 @@ impl Hub {
                 Ok(()) => drop(conn.queue.drain(..count)),
                 // Not taken: the server is busy. They go again at the next entry, first.
                 Err((SysError::Timeout | SysError::Busy, _)) => return,
-                Err(_) => return self.end(c),
+                Err(_) => {
+                    PROBE_END[0].store(4, Ordering::SeqCst);
+                    return self.end(c);
+                }
             }
         }
     }
@@ -491,7 +513,16 @@ fn waiter(endpoint: Endpoint, wake: Endpoint, index: u64, lend: Buffer) {
             continue;
         };
         let hold = if held.is_empty() { COLLECT_WAIT } else { 0 };
+        let begun = redoubt_rt::handle::time_now().unwrap_or(0);
         let mut outcome = endpoint.call(&collect_words(hold), &[], Some(buffer), hold + COLLECT_MARGIN_US);
+        if let Err(e) = &outcome.status {
+            let took = redoubt_rt::handle::time_now().unwrap_or(0).saturating_sub(begun);
+            PROBE_CALL[1].store(index as u32, Ordering::SeqCst);
+            PROBE_CALL[2].store(*e as u32, Ordering::SeqCst);
+            PROBE_CALL[3].store((hold / 1000) as u32, Ordering::SeqCst);
+            PROBE_CALL[4].store((took / 1000) as u32, Ordering::SeqCst);
+            PROBE_CALL[0].fetch_add(1, Ordering::SeqCst);
+        }
         // A session the server opened afresh (word 2) is as good as an end: what was outstanding
         // is gone.
         let words = match (&outcome.status, &outcome.reply) {
diff --git a/userland/otp/redoubt/src/lib.rs b/userland/otp/redoubt/src/lib.rs
index 7b4eed1df..aeaf68ad6 100644
--- a/userland/otp/redoubt/src/lib.rs
+++ b/userland/otp/redoubt/src/lib.rs
@@ -136,10 +136,20 @@ struct ConsoleIo {
     busy: u64,
     /// The console took nothing, or has gone: nothing more is written.
     gone: bool,
+    /// PROBE: why the read or write ended, said synchronously at the next console read.
+    why: Option<alloc::string::String>,
+    /// PROBE: how many failed completion calls have been said.
+    calls_said: u32,
 }
 
 fn now() -> u64 { redoubt_rt::handle::time_now().unwrap_or(0) }
 
+fn probe_why(what: &str, outcome: &Outcome) -> alloc::string::String {
+    let e = &redoubt_client::aio::PROBE_END;
+    let o = core::sync::atomic::Ordering::SeqCst;
+    format!("PROBE console {what} ended: {outcome:?}; hub end kind {} w0 {} w2 {} at t={}\n", e[0].load(o), e[1].load(o), e[2].load(o), now())
+}
+
 impl ConsoleIo {
     /// Takes `done` if it is the console's read or write; anything else, a file operation on
     /// `/dev/cons` on the same connection among them, is handed back.
@@ -151,6 +161,7 @@ impl ConsoleIo {
             self.reading = None;
             match done.outcome {
                 Outcome::Read(0) => {
+                    self.why = Some(probe_why("read", &done.outcome));
                     self.ended = true;
                     self.eof_pending = true;
                 }
@@ -163,6 +174,7 @@ impl ConsoleIo {
                 Outcome::Busy => {}
                 // Refused, flushed, or the console has gone: there is no more input.
                 _ => {
+                    self.why = Some(probe_why("read", &done.outcome));
                     self.ended = true;
                     self.eof_pending = true;
                 }
@@ -178,7 +190,10 @@ impl ConsoleIo {
                     self.retry_at = Some(now().saturating_add(RETRY_US));
                 }
                 // A console that takes nothing, or has gone, gets no more.
-                _ => self.stop_writing(),
+                _ => {
+                    self.why = Some(probe_why("write", &done.outcome));
+                    self.stop_writing();
+                }
             }
             self.write_buffer = done.buffer;
             self.write(io);
@@ -295,6 +310,8 @@ impl Redoubt {
             retry_at: None,
             busy: 0,
             gone: false,
+            why: None,
+            calls_said: 0,
         };
         Ok(Redoubt {
             lend,
@@ -502,6 +519,21 @@ impl Platform for Redoubt {
             self.start_reading();
         }
         self.take_completed();
+        if let Some(why) = self.cons.why.take() {
+            write_all(&self.console, &mut self.lend, why.as_bytes());
+        }
+        let c = &redoubt_client::aio::PROBE_CALL;
+        let o = core::sync::atomic::Ordering::SeqCst;
+        let seq = c[0].load(o);
+        if seq != self.cons.calls_said {
+            self.cons.calls_said = seq;
+            let line = format!(
+                "PROBE completion call failed (#{seq}): connection {} status {} hold {} ms took {} ms (late by {} ms past hold + margin) at t={}\n",
+                c[1].load(o), c[2].load(o), c[3].load(o), c[4].load(o),
+                c[4].load(o).saturating_sub(c[3].load(o) + (redoubt_client::aio::COLLECT_MARGIN_US / 1000) as u32), now()
+            );
+            write_all(&self.console, &mut self.lend, line.as_bytes());
+        }
         if !self.cons.input.is_empty() {
             return ConsoleInput::Data(self.cons.input.drain(..).collect());
         }
```

## The 100-run sweep (probe on 47a830958, six at a time, q run --cores 1, loaded pool)

97 PASS, 3 FAIL (runs 37, 38, 39: one batch, started within 2 s of each other). Every run's logs
were kept (/home/mcloonan/redoubt/.tmp/from-tmp/k27sweep100: 100 run dirs, 300 session logs; the failures are batch-7). The three failures are K27's shape
exactly: bob's session prints its prompt, then `ok` (the VM's `run` returning) and ssh exits
status 0 before bob can print 55; the steward reaps users/bob/{}; sshd ends bob's session.

In all three, and in no passing run:
- `PROBE completion call failed (#1): connection 0 status 13 hold 10000 ms took 11001..11002 ms`:
  connection 0 is the VM's `/dev/cons`, for an SSH session sshd's channel console (bob's slot);
  status 13 is `Timeout`; the client's timeout fired on time (1..2 ms past hold + margin), so it
  is sshd that did not answer its own parked completion call within 1 s of the hold's end;
- `PROBE console read ended: Ended` (hub end kind 1: the call failed), which ConsoleIo takes as
  the input's end: the shell exits as at Ctrl-D.
No other connection's call failed; no console write ended; the 291 other reads ended `Read(0)`
at the case's own `exit`.

When: bob's missed window (hold end to hold end + 1 s) against alice's vault login (the
steward's `audit 1001/{7}`, after which it carves users/alice/{7}, 11 009 pages, and launches
the vault session):

| run | window (s) | vault login audit (s) |
| --- | --- | --- |
| 3580473 | 15.14 - 16.14 | 14.81 |
| 3580533 | 15.18 - 16.18 | 15.72 |
| 3580608 | 15.99 - 16.99 | 16.90 |

Each window opens within 0.4 s after the vault login or straddles it; alice's flood has not
begun (it is typed after her vault prompt). By sshd's design (servers/sshd/src/bin/sshd.rs module
doc) bob's console is served by bob's slot's driver thread, and alice's login (keyd, the steward
call) runs on her own slot's driver: "a call to keyd or the steward holds up only its own
connection". So bob's driver, a separate thread, was not run for over a second while the
steward handled the vault login: a system-wide stall around the vault session's carve and
launch, not sshd's own serving and not the flood.

Not reachable with this probe: whether the stall is kernel work that cannot be preempted (a carve
of 11 009 pages, the launch's mapping of the session image) or scheduling. The sched-trace ring
cannot help a failing run (printed only at system_reset; 64 MiB of guest frames).

## The 60-run sweep with both probes (orchestrator's approval, probe in .wash/local/K27-probe2.patch)

60/60 PASS (six at a time, loaded pool, all logs kept in /home/mcloonan/redoubt/.tmp/from-tmp/k27sweep60). No hub failure; no
kernel stretch over 100 ms in any run: each run's longest is 2.2 to 4.5 ms (median 2.5 ms),
receive or process_exit; the longest user-mode stretch between two kernel entries is 8.7 ms
(two runs over 5 ms). The steward's vault login: carve ~0.1 ms, connects 0.01 to 23 ms, launch
~0.33 s of which the 15 image reads ~0.27 s.

## Reading

The case runs on the host's clock: its disk, userland disk and SSH forward keep it off icount
(docs/testbench.md, "Which cases run in guest time"). There a stall of QEMU itself moves the
guest's time with no guest code running. The 100-run sweep's three failures were the first three
runs of one batch and failed at host times about 88.5, 89.5 and 91.4 s (run dirs' start stamps
plus the guest time of each failure), within 3 s of each other; the batch's other three runs,
at earlier guest phases then, passed. A host episode of a few seconds fits: if a stall spans the
end of bob's hold, both sshd's hold expiry (T) and the VM's call timeout (T + 1 s) come due during
it, and the kernel answers expired deadlines first at its next entry (irq.rs: "Every entry but
kmain's switch answers the deadlines that have passed first"), so the VM's call is timed out
before sshd's thread runs to answer. Nothing in the guest is slow; in passing runs every
in-guest stretch is under 10 ms.

Not yet proven: no failing run carried the kernel probe, and a stall while the guest idles in
`wfi` shows in neither the kernel nor the user stretch. The decisive probe: at each wake from idle,
how late the wake is past the deadline the timer was armed for.

## Scratch location

From 2026-10-08 (owner's rule) sweeps and scratch live under the project's ignored scratch
directory, $REDOUBT_TMP = /home/mcloonan/redoubt/.tmp/K27/, never /tmp (a RAM filesystem). The
earlier sweeps were moved to /home/mcloonan/redoubt/.tmp/from-tmp/ (k27sweep100, k27sweep60).
Probe-3 sweeps: .tmp/K27/sweepC-cut (18 runs before a restart, 18 PASS) and .tmp/K27/sweepC (the
100-run sweep: verdicts.log, batch-N/run-*/ with the console log, the three *.ssh.log and the
*.hosttime file, summary.txt).

## Probe 3 (idle-wake lateness + user/kernel stretch timers + host-time stamps; .wash/local/K27-probe3.patch)

Scratch under .tmp/K27/: sweepC-cut (24 verdicts before a restart) and sweepC. Two loops overlapped
after a restart notice that turned out not to have killed the first, so sweepC's verdicts.log
double-counts by run number; the count that stands is the distinct run directories kept: 172,
every one PASS, eight at a time on a loaded pool.

- hub completion-call failures: 0; console reads ending `Ended`: 0.
- kernel entries over 100 ms: 0; per-run longest median 3.1 ms, max 30.7 ms (one `process_exit`,
  the VM's own end at the case's `exit`).
- user-mode stretches over 100 ms: 0; worst 13.2 ms.
- idle wakes late past their armed deadline: none over 5 ms in any run.

So with the detectors in place no failure occurred in 172 runs; the only failures remain the three
of the first 100-run sweep (one batch, within 3 s of host time, each at bob's hold's end right
after alice's vault login), which carried only the hub probe. Across every probed sweep:
3 failures in 100 + 60 + 24 + 172 = 356 runs, all three in one three-second host-time window.

## Reading and proposal

The mechanism of the three failures is established (sshd's channel console missed its hold's end
by over 1 s; the VM's completion call timed out on time; ConsoleIo took the read's `Ended` as EOF;
the shell exited as at Ctrl-D). The cause of the miss is not: in 256 runs with in-guest timers no
stretch of kernel, user or idle time came within a tenth of a second, so the guest never stalled
while it ran; the three misses clustered in one host-time window on a host-clock case under eight
parallel QEMUs, which is what a host-side stall of QEMU looks like from inside the guest (the
guest's clock advances while nothing runs: no detector in the guest can see it except a late idle
wake, which did not happen in any probed run). Positive confirmation would need a failure under
probe 3; 172 runs produced none.

Proposal (the orchestrator decides): no product change. steward-sub-budget-flood, and every other
host-clock steward/ssh case that judges a session's survival across a hold (10 s), joins jobs.mk's
alone class: it cannot move to guest time, since its SSH forward keeps it on the host's clock
(docs/testbench.md, "Which cases run in guest time"). The hub's 1 s margin stays (B20's rule).

## Closing (orchestrator's decision, 2026-10-08)

No product change: the kernel, the hub's 1 s margin and ConsoleIo stay as they are. K27's
deliverable is the bench's, one commit on wp-K27 from main 142f8a531: 935afba93 `testbench: a
host-clock case judging a session's survival across a hold is a verdict only alone`:
- scripts/jobs.mk: steward-sub-budget-flood, steward-ssh-two-principals and steward-vault-session
  join the quiet (alone) class (`quiet +=`, filtered against the tree's cases, so on main they
  take effect when STEWARD2 merges); the class comment says why.
- docs/testbench.md "On a shared host": the sentence naming the three and the mechanism (a host
  stall past the console's hold reads, inside the guest, as the server's silence).
- Excluded: steward-session-ends and steward-login-refused (an ending and a refusal, not a
  survival); main's ssh-loopback multi-session cases (no VM session, so no hub hold).
- The commit message carries the evidence: 356 runs, 3 failures in one 3 s host window, the
  detectors' maxima. Probe patches: .wash/local/K27-probe.patch, -probe2.patch, -probe3.patch.
Gates: `make list-classes` with the new jobs.mk on the STEWARD2 tree shows the three under quiet;
prebuilt, docs and rv64/formatting on the K27 tree and the three moved cases once each through
the new jobs.mk on .worktrees/K27-base: results below when they land.
Gate results (head 935afba93; log .tmp/K27/gates.log): K27 tree prebuilt rc 0, docs PASS,
rv64/formatting PASS; through the new jobs.mk on .worktrees/K27-base (wp-STEWARD2 47a830958), each
in the quiet class: rv32/steward-vault-session PASS 8.6 s, rv64/steward-sub-budget-flood PASS
11.6 s, rv32/steward-sub-budget-flood PASS 11.7 s, rv32/steward-ssh-two-principals PASS 15.9 s.

## Red's note folded (amend, head 8bf0c79a1)

The alone-class criterion is now stated as the mechanism, not a list: a host-clock case with a VM
session whose console goes through the hub's hold and which must still be alive after a hold
boundary, because a stall of a second or more at the boundary ends it (jobs.mk's class comment,
testbench.md's sentence, dateless). By that criterion steward-session-ends joins (its sessions
are VM sessions on the host clock; under load a boundary can fall while one is alive and expected
to answer), so the quiet class gains four: steward-sub-budget-flood, steward-ssh-two-principals,
steward-vault-session, steward-session-ends; steward-login-refused stays out (refused logins
start no VM). Gates for the amended head: docs on the K27 tree; list-classes and the flood case
on both widths through the new jobs.mk on a fresh wp-STEWARD2 worktree (.worktrees/K27-cases):
results in .tmp/K27/gates2.log.
Gate results for 8bf0c79a1 (.tmp/K27/gates2.log): K27 tree prebuilt rc 0, docs PASS; new jobs.mk
on .worktrees/K27-cases (wp-STEWARD2 47a830958): list-classes shows the four under quiet;
rv64/steward-sub-budget-flood PASS 11.5 s, rv32 PASS 11.3 s, each under the quiet class.
