HOME1 handoff (home1-implementer). HOME1 is MERGED to main as 7659ce688 (steward red OK, nothing open). Nothing of mine is uncommitted; .worktrees/HOME1 (wp-HOME1, head 98e8d8f9f) can be removed.

What HOME1 left in the tree, for whoever touches it next:
- Manifest: principals[].home_quota (bytes, required with home), volumes[].bytes (required where homes are carved). init refuses Quota, OverCommitted, SharedVault, SharedHome (equal/nested homes on one volume). Lines: steward `home ... quota=N`.
- Steward: one carved home connection per principal (How::Carved, Machine::carve/fresh_carved, Via enum), kept for its life; sessions minted through it with quota 0.
- Bench: disk recipe `manifest =` holds volume bytes to partitions at pack; case load holds a case's manifest to its disk (disk.rs hold, case.rs).
- Steward stack 8 pages (rv64 peak 14,856 B incl. RCMD1 session args). Size ceilings: steward 1268, init 2500.

Traps learned:
- target/prebuilt goes stale on any tree change (even docs): edit nothing while a prebuild/case run is going; set edits aside as a patch under $REDOUBT_TMP.
- Session heap ~3 MB: write in 64 KiB chunks from one reused binary; IO.binwrite raises on enospc, use :file.write.
- A principal's console + 2 SSH sessions overflow its sub-budget; steward-home-quota uses console + 1 SSH per principal.
- steward-model-host-tests' steward_policy job runs past 35 s under a loaded gate: rerun alone.
- Usable home quota is a little under home_quota (per-entry share): 7,936 of 8,192 KiB.

Stale lines found, not changed (outside HOME1): docs/plan/m1-separation.md "Not built: a session's files, and a steward restart without a reboot", the broken "[R2 ...]" fragment after it, "Remaining work" listing the steward restart; steward.md Failure-and-restart status names littlefsd:alice-secrets. Report: .wash/local/HOME1-report.md.
