# INIT4 handoff (worktree .worktrees/init4, branch wp-init4, base ccf648bad)

Commits: a3503ba6d (slots + net-tcp), 993a032c5 (slirp ipv6=off), aa7f54b7e (peer + 3 twins),
10b0afebb (net-pinned), d1e7ce339 (virtio-probe), c145e2c80 (host test reads manifests),
34ab8ade3 (attacks, ruling (a)), 0da8cd3bd **WIP netd-restart: fold it**.
Every case is now init + keyd + consoled + netd + ipd + net-judge + net-client, from
tests/data/net/*.json. All PASS on both widths: net-tcp, the 4 bench-net-peer cases, net-pinned,
net-attacks, -self-unrefused (must-fail), bench-virtio-legacy-off (virtio-probe). netd-restart
PASSes on both widths only with netd's feature forced on locally.

**Uncommitted (step 9, rig deletion), done:** rig.rs, build.rs and net-rig* are gone; the
Cargo.tomls, lib.rs (SELF_ALWAYS only), the client's rig path and the host test are reworked;
the brief's page lines and netd.rs's module doc are written. Host tests and docs PASS. The
moved cases have not been rerun since the deletion. Also on disk, uncommitted (my last
command was rejected by the user, yet these two edits are there; confirm before keeping):
- virtio-probe.rs drops "1 for legacy" (no-cruft fails on it); fold into d1e7ce339.
- the rustfmt of restart_probe.rs; fold into the netd-restart commit.

**Step 8, as answered:** a UDP poke, `[net.poke]` {port, payload, after}. case.rs, peer.rs (granted;
say so in the body) and qemu.rs (hostfwd=udp, Console sends it once). netd feature
`restart-probe`: src/restart_probe.rs holds PORT 47000, PAYLOAD and INSTANCE 0x70726f62
(32-bit for rv32). netd faults via kernel::fault(), a cfg(feature) volatile load at 0x8 (page
0), on the poke frame in forward(). It answers the INSTANCE call with 32 random bits, so the
judge sees the new instance deterministically. Unsafe 7->8, line: `Unsafe budget: netd
(virtio-net driver; DMA, so TCB): test-only fault; a panic's hook resets the card itself, and
the case needs the kernel's fault path`. Size ceiling 1552->1592, needs a `Size budget:
servers/netd:` line. **Pending:** after rebasing over B7, add `features` to Program::Package
(case.rs), and in build.rs pass it to cargo(), with the feature build in its own out
directory. That goes in the netd-restart commit, and the case gets
`features = ["restart-probe"]` on netd.

**Bucket ruling (a):** init sizes buckets >= handed badges + principal domains, which closes
the exhaustion attack, so the holders and bucket lines went. buckets=7.

**Pending:** rebase over B7 and RT1 when told; rebuild into clean commits; the whole bench
needs the orchestrator's word. Stale lines outside my paths: docs/SECURITY.md:233,
servers/ipd/src/bin/ipd.rs (module doc), servers/ipd/tests/{args,sizing}.rs comments,
tests/{ipd,netd}-host-tests.toml comments, docs/plan/m1 (Architect's).

**What consumed my context:** reading rig.rs and qemu.rs; dumping the pcap of a slirp
router-advertisement flood (3 MB); many python edit scripts; several FAIL loops.
