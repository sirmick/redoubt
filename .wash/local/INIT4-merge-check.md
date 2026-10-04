# INIT4 merge check at d3538d345 (architect-10)

The brief's page lines are written as given: testbench.md (the slots, the `tests/net/` row),
netd.md "Started by `init`" and its residual bullet, ipd.md, init.md (both). SECURITY.md:135 and
serving.md:456 follow the ipd test's rename, and SECURITY.md:159 is fixed. The poke and IPv6
paragraphs in testbench.md are true to the code. "rig" is left only in
docs/plan/m1-separation.md:158, which is the architect's (init-step-close.md).

## A. In d3538d345 (it drops SECURITY.md:233 whole, so its sources go too)

Delete these bullets whole. Every one of these servers now boots under `init` in the bench
(init-servers, init-driver-restart, net-*):
- blkd.md:298-299, "**`blkd` does not boot in the bench.** ..."
- bootfsd.md:140-141, "**`bootfsd` does not boot in the bench.** ..."
- keyd.md:311-312, "**`keyd` does not boot in the bench.** ..."

servers/README.md:320-321: replace the bullet "**Until `init` places them, servers run only
under test launchers.** ..." with:
> - **Until `init` places it, `fsd` runs only under test launchers.** Its rules hold where a test
>   launches it; one instance per volume is what the running system adds ([fsd](fsd.md)).

## B. In the netd-restart commit, when the features piece lands after the rebase

- netd.md:169 becomes:
  > Status: built · tested: bench:init-boot, bench:netd-restart

  The interim line (feature not built yet) is right until then.
- init.md:430: drop "; a killed `netd`'s restart has no case yet", add `- bench:netd-restart`
  after `- bench:init-driver-restart`, and change (9) to (10).
- testbench.md's case-file example (around :140, the `programs` list) gains the per-program
  feature field, in whatever form the code takes, with a comment saying it builds that program
  with those features. Send me that line at the rebase.
