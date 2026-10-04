# FSD3 report, round 2 (fsd3-implementer-3)

Branch wp-fsd3, tip e1ac44c75, on main bfdb471b3 (INIT5). 15 commits, no fixup/amend/WIP left;
every review change went into its owning commit by fixup/amend! + `rebase --autosquash` (no
editor). Commit hashes below are from the c2283a825 base (tip a5b505e97); subjects are unchanged.

Later on the same branch: the Architect's two page edits (fsd.md Authority names the console;
testbench.md lists bench:image-disk before bench:init-boot), folded into their commits, tip
334415a69; then `rebase --onto bfdb471b3 c2283a825`, resolving two overlaps with INIT5:
- tests/size-budget.toml, servers/init: INIT5's +3 carried into each of FSD3's ceilings
  (1914, 1915, 1916, 1917); each commit's increment and Size budget line unchanged.
- servers/init/tests/manifest.rs `the_image_manifest_s_bound` (image commit): INIT5's one 64-page
  batch launch term with FSD3's seven-server counts:
  259 + 15 + 7 + 28 + 56 + (4 + 3 + 64 + 3 + 16 + 3) + 5.
Nothing else differs from the pre-rebase branch diff. On e1ac44c75 every commit passes the host
gates and both-width builds; formatting and unsafe-budget pass at the tip; init-boot, image-disk,
fsd-boot, init-refuses-confined-server and fsd-confined-labelled PASS on rv64.

## Changes, by owning commit

1. Red's P1 (99806bf4d init range): `check.rs volumes()` refuses any `handed` item at an endpoint
   a `blkd` receives on, `Why::BlkdHanded` ("only init mints a badge at blkd, a volume's range
   (R47)"). Host test `no_server_is_handed_a_badge_at_blkd` (volume attached + netd handed badge 1;
   no volume, keyd handed badge 9). init.md Volumes bullet gains the ruled clause; the test is
   listed in init.md "Starting the servers" (16), fsd.md R47 (2, in 34ef5fe69) and SECURITY.md R47
   row (code location check.rs added). Size budget: servers/init 1903 -> 1911 (86 lines, message
   updated); later ceilings +8 (1912, 1913, 1914).
2. Sharing::Server (a5b505e97): reachable. `confined_refuses_a_server_instance_serving_two_label_sets`
   re-aimed: blkd with no devices, labelled {}, a {7} fsd attaching a {7} volume -> Server at
   blkd's servers[i]; labelling blkd {7} then passes. Back in init.md's confinement check and R34
   (8 each) and SECURITY.md R34. confine.rs's "Defensive" comment replaced by the shape that
   reaches it. Driver test stays dropped; bench init-refuses-confined-server refuses netd labelled
   vs unlabelled ipd as Endpoint (comment rewrapped). Commit message rewritten.
3. fsd.md restart paragraph and fsd-restart's description: the ruled text (608934b65); the commit's
   title now "fsd: a restarted fsd mounts with a boot's checks, then serves". No "Open: none" was
   there to keep.
4. Editor: blkd.md's label-check line says "which blkd takes as arguments" in cf881ab3c and "which
   init gives blkd" from 99806bf4d (cf881ab3c's message reworded likewise); testbench.md list
   grouped bench then host (da19e681e); over-width paragraph rewrapped (d70ce5557); editor 4
   checked: no fsd-labelled-volume left, R25 (8) right, init.md example args match.
   Red's note: fsd-reboot's capture groups dropped.
   Simplifier 1+2: `[[file]]` read from a path may carry `servers` entries merged into the
   manifest by name (members replaced, or entry appended); `BundleFile::merged` in case.rs, the
   merged file written to the run dir in main.rs; documented in testbench.md "Bundle files"; host
   test `a_file_s_servers_merge_into_its_manifest_by_name`. In d70ce5557 (fsd-reboot is the first
   user). reboot/restart/corrupt/quota.json and image.json deleted (386 JSON lines); each case sets its
   client's args over boot.json; image-disk boots image/manifest.json plus its client. Each merge
   was checked equal to the file it replaced before deletion.
   Simplifier 3: one reader, `read_file(conn, lend, path)` (e8ccef9d0).
   Orchestrator's sleeps: gone. fsd-label-check is event-ordered: writer receives on `writer`,
   outsider on `outsider`, each handed the other's (badge 1). The writer's second start sends to
   the outsider, which only then attaches (refused), says its line and sends back; only then does
   the writer read back. New writer code 24 (OUTSIDER), already forbidden by the case.
   Messages of 99806bf4d, d70ce5557, da19e681e, e8ccef9d0, a5b505e97 updated to match.

## Gates (tip a5b505e97 unless said)

Per commit (`rebase --exec`, all 15 ok on c2283a825): init-, fsd-, blkd-, littlefs-host-tests,
docs, size-budget (cargo testbench, exit 0); `cargo test -p testbench` (exit 0); release build of
init, fsd, blkd (+ fsd-programs where present) for riscv64imac and riscv32imac (exit 0).
At tip: formatting PASS, unsafe-budget PASS (no new unsafe; count unchanged).

Cases, `cargo testbench <case>` one at a time, each exit 0, PASS on rv64 and rv32:
fsd-boot, fsd-reboot, image-disk, init-boot, fsd-restart, fsd-corrupt-volume, fsd-quota,
fsd-one-volume, fsd-label-check, fsd-confined-labelled, init-refuses-confined-server.

Not run: the whole bench (waits on the orchestrator's word).

## For reviewers

- Red: the P1 fix is 99806bf4d (check.rs, refusal.rs, manifest.rs, init.md).
- The merge feature serializes with serde_json (keys sorted); init's strict JSON accepts it (the
  five merged-manifest cases pass on both widths).
