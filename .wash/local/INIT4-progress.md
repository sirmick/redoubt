# INIT4 progress
- 2026-10-03: brief read. Step 1 (fixed slots: bus=virtio-mmio-bus.6/.7 in qemu.rs, host test devices_sit_on_fixed_slots) written, host tests pass. Designing net-tcp under init: net-judge (tests/net) as reporter; clients handed ipd+judge badges; START held by judge to sequence.
- 2026-10-03: CHECKPOINT. branch wp-init4 a3503ba6d. Steps 1+2 committed as one commit (slots + net-tcp under init). net-tcp PASS rv64 and rv32 (init, keyd, consoled, netd, ipd, net-judge, 2x net-client; tests/data/net/tcp.json). Rig cases still pass on rv64. Reported; waiting.
- 2026-10-03: step 3 committed (bench-net-peer + 3 twins under init, both widths PASS); separate commit: slirp ipv6=off (RA flood under icount after verdict). Next: net-pinned.
- 2026-10-03: step 4 net-pinned committed, both widths PASS. Next: legacy-off (step 6), host tests (7), netd-restart (8); attacks (5) last per orchestrator.
- 2026-10-03: step 6 committed (legacy-off -> virtio-probe, both widths). Next: step 7 net-host-tests.
- 2026-10-03: step 7 committed (host test reads manifests via init decoder + ipd parser; mutation-checked). Next: step 8 netd-restart.
- 2026-10-03: steps 1-4,6,7 done (head c145e2c80). Blocked: step 5 (bucket wording, Architect), step 8 (trigger/unsafe question sent). Waiting.
- 2026-10-03: step 5 built uncommitted (client to= targets, judge attacks, attacks/unrefused.json, host test exemption). Blocked: init's bucket rule (buckets >= handed badges + principal domains) makes 'next refused' unreachable; ipd sizing refuses buckets=10. Experiment without holders, buckets=7: all attacks/controls/victim lines right. Asked (a) drop bucket count. Step 8 question pending.
- 2026-10-03: step 5 committed per option (a) (no bucket count), both widths PASS. Next: step 8 (UDP poke, netd restart-probe), then 9.
