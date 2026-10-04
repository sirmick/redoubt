Merge verdict: OK with notes

INIT4 simplifier, d3538d345 (nine commits on ccf648bad). Read only; built nothing.

## Deleted
- rig.rs, the seven net-rig*.rs binaries and build.rs are gone. The stub and ipd deps left tests/net/Cargo.toml.
- lib.rs keeps only SELF_ALWAYS.
- No `net-rig` or `rig.rs` name survives in code or docs.
- Two stale words remain: `RIG` in servers/ipd/tests/args.rs:14, and `rig` in servers/ipd/tests/sizing.rs:15. The doc comments were reworded, the identifiers were not. Rename them to `ONE_CLIENT` or similar.
- Not checked: whether anything in the testbench served only the rig (qemu.rs's slot probing and its old disk-first ordering).

## Duplicates
1. Seven manifests of 63 to 87 lines each (tests/data/net/*.json) are about 90% identical.
   - peer, twice, tcp and restart differ only in the case argument, the scope lines, and the last server's name, badge and arguments.
   - unrefused differs from attacks by one removed `self=` line.
   - One base manifest per shape, with a variant, would cut roughly 300 lines.
   - The manifest format has no include or variant mechanism today. Adding one to the testbench costs more than it saves, so I would leave this, but decide it explicitly.
   - `unrefused` is the easiest to fold: a must-fail twin that differs by one argument should be generated from `attacks`, not copied.
2. The judge (389 lines) and the client (424 lines, up from 353) do reporting and verdicts that the tester already does for other cases.
   - The tester is a boot-case program; the judge is a `reporter` under init, which is a different mechanism with its own badge rules. That justifies a separate program.
   - The judge's `case=` argument holds six cases in one binary, so every case's checks ship in every case's image.
   - One judge binary per case is not better, so the one-binary form is fine. Note that `case=tcp` and `case=twice` share `Case::Tcp{rounds}`, so `twice` is a rounds count, not a case.
3. The UDP poke has one sender, peer.rs:313, and one parse site, case.rs:702. There are not two ways to send it.

## One obvious way
- `[net.poke]` is checked against `net.forward` by guest port, so it cannot share a port with a TCP forward. That is a limit, but it is explained and tested (qemu.rs:695, cases.rs:129).
- The probe's PORT and PAYLOAD are shared by netd, the bench case and tests/cases.rs through the `redoubt-netd` dependency. That is the one place they are defined.

## Sizes
- netd 1552 to 1592: +40 for restart_probe.rs (82 lines in the new module) plus 24 lines in netd.rs and 20 in kernel.rs. Justified: it is the test-only fault the restart case needs, and it is behind a feature.
- testbench: about +131 lines (qemu.rs +91 for fixed slots and the poke forward, peer.rs +18, case.rs +22). Justified by the fixed slots and the poke.
- tests/net: rig.rs 753 and the seven stubs (13 lines each) went. net-judge 389 and the client's +187 came. Net is roughly -480 in Rust plus about 480 lines of JSON manifests, so the manifest duplication above is where the remaining size is.
