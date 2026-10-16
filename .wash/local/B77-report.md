# B77 report: [net] cases lease host ports from q

- Base: main 6713d6ffa. Head: wp-B77 e7b765b91 (one commit; review round folded: testbench.md:358 lock sentence, the under-q stranger residual, the `--quiet ""` clause, adopt() reads environ best-effort with cores adopted regardless, a comment on the `given` arm; q-test pins its daemon to the affinity's last four cores so another member's low-core job is not adopted into it, and checks the adopted ports rather than a job count). Worktree /home/mcloonan/redoubt/.worktrees/B77, clean.
- Files: scripts/q, scripts/q-test (new), scripts/jobs.mk, tools/testbench/src/qemu.rs, tools/testbench/src/main.rs, docs/testbench.md.

## The lease design

- `q run --ports N`: the daemon finds the lowest run of N consecutive ports in its pool (`q daemon --ports lo-hi`, default 23000-23999, below Linux's ephemeral 32768-60999) that no running or adopted lease holds and that a TCP+UDP bind probe on 127.0.0.1 finds free (a stranger's port, or one in TIME_WAIT, is skipped). The lease reply carries `ports: [lo, hi]`; the client exports `REDOUBT_PORTS=lo-hi` to the command (and strips an inherited one when the job leased none, so a nested job never reuses a parent's range). The ports free in `finish()`, i.e. with the connection: a killed client frees them at once. A job whose ports cannot be found waits (`held()`), and is excluded from the starvation path as a locked job is; `start()` re-checks and keeps the job waiting if a stranger bound a port between `fits()` and the lease.
- Adoption: `adopt()` already walked the children of `q run` clients for their CPU pins; it now also reads `REDOUBT_PORTS` from `/proc/<pid>/environ` (the client sets it for the command, so it is the lease's), filtered to the pool, and `reap_adopted()` frees it with the cores. `q ls` shows the pool and leased count, per-job `ports=lo-hi`, and `ports=N` on waiting jobs; `q log` lines carry `ports=lo-hi` on start and end.
- Bench: `qemu::host_ports(tcp, udp)` returns the TCP forwards' ports and the poke's: from the range's start when `REDOUBT_PORTS` is set (`leased_ports`, pure, unit-tested; a range too small or malformed is an error), else `free_ports` + `peer::free_udp_port` as before. `virtio_devices` and the launch path in main.rs use it. `free_ports` stays for runs outside q.
- jobs.mk: the net recipe asks `--ports <forwards + pokes>` computed from the case's toml (`ports_of`), nothing for a case with only peers/dials (bench-net-peer). Every net case today needs 1.
- Compatibility: against a daemon that predates the flag the client gets a lease without `ports`, returns it, re-requests under `--lock net` and says so on stderr, so the merged jobs.mk keeps today's behaviour until the restart. Observed live: `q: the daemon at /tmp/redoubt-q.sock leases no ports; running b77-fallback under --lock net instead (restart the daemon: ...)`; that 0 s probe then sat 30 min behind STEWARD3A/3Q's `lock=net` jobs.
- `q daemon --quiet ""` now means no quiet set (default unchanged: the last four cores); q-test needs it.

## Before / after

- Train 40 (from the body): 30 lock=net cases, 12,360 s summed wait, max 1,353 s.
- Gate A, here, 30 net cases × both widths = 60 runs through the running (pre-ports) daemon, so under the fallback lock, from prebuilt: 60/60 PASS, wall 4,892 s (23:22→00:44), 55 lock=net starts in the window of /tmp/redoubt-q.sock.log (the rest fell off the `q log 400` copy), summed wait 234,058 s, max 4,862 s; only 2 lock=net starts from other tenants in that window, so the waiting is the set serialising on itself. Log copy: /home/mcloonan/redoubt/.tmp/b77-gates/A-q.log, job logs A-jobs/.
- Gate B, the same 60 runs under the new daemon (nested inside a real 12-core `q run` lease, socket .tmp/b77-nested.sock, `--quiet ""`), as two concurrent makes `make -k rv64/<30>` and `make -k rv32/<30>` (two members' net gates side by side): 60/60 PASS, wall 269 s (01:16:40→01:21:09), summed wait 748 s (core waits on 12 cores), max 66 s, 12 jobs at once, 113 `ports=` lines, no "Could not set up host forwarding"/"Address already in use" in any job log. Logs: .tmp/b77-gates/B-q.log, B-rv64.out, B-rv32.out, target/jobs/. The outer lease itself waited 1,939 s for 12 cores behind other members' work (not part of B's wall).

## What the daemon restart needs

At a train boundary, from /home/mcloonan/redoubt with the merged scripts/q: `scripts/q stop` (running jobs keep their cores), then `scripts/q daemon &` as today (pid 2187386 was started as `python3 scripts/q daemon` with defaults; the default pool 23000-23999 needs no flag). The new daemon adopts the running jobs' pins; until the restart every `--ports` job runs under `lock=net` with the stderr note. Nothing else changes.

## Gates

- scripts/q-test: PASS (14 checks: disjoint ranges, held port skipped, wait on a full pool, kill frees ports to the waiter, restart adopts 3 jobs/6 ports, new job waits then leases, all freed). ~8 s, private socket, sleeps only.
- `q run --cores 8 -- cargo test -p testbench`: 213 passed (new: qemu::tests::leased_ports_come_from_the_range_in_order). Pre-existing warning: idle.rs `harts` never read.
- make prebuilt: rc=0. Gate A and B above (both widths, every [net] class case: agent-login bench-net-peer bench-net-peer-count bench-net-peer-pcap-empty bench-net-peer-twice bench-net-self-unrefused job-interrupt-ssh launch-keep-disk launch-system net-attacks net-pinned net-tcp netd-restart shell-commands shell-compile shell-compile-module shell-contexts shell-long-output shell-machine sshd-restart-detaches steward-context-cap steward-context-idle steward-context-labels steward-context-login steward-home-quota steward-login-refused steward-login-timing steward-restart-context steward-restart-ssh steward-ssh-resize).
- make docs: PASS. rv64/no-cruft: PASS. `cargo +nightly fmt -p testbench --check`: rc=0 (one wrap applied before the commit).
- Not run: the whole bench (Tier B); the net cases under the real daemon with ports (needs the restart).

## Summaries checked

- docs/testbench.md: "On a shared host" rule (the race paragraph) and the `scripts/q` paragraph rewritten: ports leased under q, race gone there, stays outside q; class (ii) (bench-ssh-guest, bench-ssh-loopback-deadlock, the host-clock cases) unchanged, they run alone for their own reasons. The `--lock net` mention replaced.
- scripts/jobs.mk header: net class description updated.
- GETTING-STARTED.md (q lines 63-64, 157, 230, 254), README.md, docs/README.md: name q and jobs.mk but not the lock or the race; no change needed.
- .wash/qa/D3-plan-review.md mentions the free_ports TOCTOU historically; a QA record, not a summary.

## Notes

- The test lives at scripts/q-test, beside scripts/q (scripts/ is where the repo's scripts are; tools/ holds crates), not tools/ as the launch suggested.
- The gate script's trailing `cp` of the nested log was cut by the harness's 2 h background cap after both makes had finished; I copied the log by hand.
- Open risk: the daemon probes ports by binding; a job's QEMU binding inside its range can still lose to a non-q process that takes the port between the probe and the boot — the same exposure as before, now only to strangers, and it fails loudly.
