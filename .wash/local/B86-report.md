# B86 report: a sweep's concurrent boots forward on disjoint slices of the job's port lease

- Base: main c63bbacbf. Head: wp-B86 ea31f8c3c (one commit). Worktree /home/mcloonan/redoubt/.worktrees/B86, clean (`git status --short | wc -l` = 0).
- Files: tools/testbench/src/qemu.rs, main.rs, launch.rs; docs/testbench.md. Nothing in the kernel, servers, scripts/q, jobs.mk or tests/.

## Reproduction (logs under /home/mcloonan/redoubt/.tmp/B86/)

No case on main pins `qemu_seed` and forwards a port (the seeded [net] cases have empty [net] tables), so a seeded copy of net-tcp (`forward = [8000]`, one dial) was made as tests/b86-net-sweep.toml, used for the runs below and removed before the docs gate and the commit.

- repro-jobs2.log: `q run --cores 2 --ports 1 -- cargo testbench b86-net-sweep --arch rv64 --sweep 1..4 --jobs 2`, lease REDOUBT_PORTS=23001-23001: seed 1 PASS 0.4 s, seeds 2-4 FAIL 0.1 s `hostfwd=tcp:127.0.0.1:23001-:8000: Could not set up host forwarding rule`.
- repro-jobs2-ports2.log: the same under `--ports 2` (23001-23002): identical collapse; every failing boot bound 23001 (3x in the log). The lease's size was never the question: every boot took the range's first port (`leased_ports` took from the start of REDOUBT_PORTS, process-wide).
- STEWARD4's collapse (.wash/local/steward4-logs/sweep-rv64-jobs2-collapsed.log) is the same signature on 23000: seed 1 ran 255 s, seeds 2-20 died at 0.1 s on `hostfwd ... 23000`.

## The fix and why slices, not one lease per boot

`sweep()` takes the job's lease (`qemu::Ports::leased()`, REDOUBT_PORTS) and cuts it into `--jobs` disjoint slices of (TCP forwards + poke) ports from the start; `in_parallel` runs its work in slots 0..jobs (one worker thread each), and the boot in slot k forwards on slice k, so a slice is one boot's at a time. A lease shorter than jobs x per-boot is refused before anything builds: `--jobs 2 boots 1 forwarded port each, 2 in all, and REDOUBT_PORTS=23001-23001 leases 1: ask `q run --ports 2`` (fixed-jobs2-ports1.log). One boot at a time takes the range's start, exactly as a single run does, so serial sweeps and non-sweep runs are unchanged; nothing to forward touches no lease; outside q each boot asks the OS (B6's residual stays). Slices rather than `q run --ports` per boot: the lease is the job's and the bench runs under q, not over it; a nested lease would compete with the outer one for cores and ports; a pre-`--ports` daemon would silently fall back to `--lock net` per boot.

qemu.rs: `Ports {lo, hi}` (parse, leased, count, slices, take) replaces host_ports's hidden env read; `HostPorts::{Given, Leased}` replaces the empty-slice convention of virtio_devices. main.rs: Sweep.ports, in_parallel's slot, boot_case/run_case take ports explicitly. launch.rs: `HostPorts::Given(&[port])`.

## Gates (all through scripts/q run / scripts/jobs.mk)

- `cargo test -p testbench`: 222 passed (219 + a_sweeps_boots_hold_disjoint_slices_of_the_lease, two_boots_at_once_run_in_different_slots, a_lease_slices_into_disjoint_ranges_or_none). Pre-existing warning idle.rs `harts` never read.
- fixed-jobs2-ports2.log: `q run --cores 2 --ports 2 -- cargo testbench b86-net-sweep --sweep 1..6 --jobs 2` (both widths): 12/12 PASS, `sweep ... rv64: 6 seeds, 6 passed` and rv32 the same.
- fixed-jobs2-ports1.log: `--ports 1` at `--jobs 2`: refused before building, rc 1, text above.
- net-cases.log: `make -f scripts/jobs.mk set CASES="bench-ssh-loopback steward-ssh-two-principals"`: bench-ssh-loopback PASS 8.1 s, steward-ssh-two-principals rv32 PASS 14.6 s, rv64 PASS 15.3 s.
- docs.log: `make -f scripts/jobs.mk set CASES=docs` PASS 4.3 s. budgets.log: rv64/size-budget PASS, rv64/unsafe-budget PASS. fmt.log: `cargo +nightly fmt --check` clean.
- Not run: the whole bench (Tier A tooling; the orchestrator's train).

## Summaries checked

- docs/testbench.md: sweep paragraph (the rule, the refusal, serial unchanged, outside q); the shared-host ports paragraph (a sweep's J boots are J such boots in one lease, each on a slice); Disks and network cards ("chosen by the operating system" was stale since B77: now the lease under q, a sweep's slices, the OS otherwise); The case file's status line gains the two main.rs tests; Disks and network cards' status gains the qemu.rs test (37 -> 38). The "How to use it" example (sched-latency, no [net]) needs no change.
- scripts/q header (--ports: "the bench binds its forwards inside the range, so two [net] boots never race for a port") and scripts/jobs.mk header (net class): still true; jobs.mk runs no sweeps. No change.
- GETTING-STARTED.md, README.md, docs/README.md: name q and jobs.mk, not ports or sweeps. No change.
- .wash/local/B6-report.md's residual (free_ports race in a sweep) still holds outside q only.

## Files read

Every committed hunk was read in full in the diff; qemu.rs's port, device and test regions, main.rs's sweep, in_parallel, run_case, boot_case and test regions, launch.rs's two call sites, and testbench.md's four edited passages were read in their surrounding context. The remainder of the three Rust files (disk recipes, the oracle, the SSH launch) is untouched.

## Risks

- A sweep of a `launch` case is refused already (no seed), so the `launch` arm's `host_ports(ports, 1, false)` only ever sees the job's whole lease.
- `two_boots_at_once_run_in_different_slots` holds two threads at a Barrier: a regression that ran both items in one slot would hang the test rather than fail it (the harness's timeout reports it).
