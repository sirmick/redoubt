# STEWARD2 rebase onto main c8cd27ba9: resolved hunks

## 1. steward: the core reads its manifest lines, and init writes them
- docs/servers/init.md, "The boot manifest" status: main's 23 (verified-volume test) + our three steward tests = 26, both lists kept.
- docs/servers/init.md, boot step 5/6: main's step 5 (littlefsd and erofsd, one per volume) with our step 6 (steward.server, users by name, the lines).
- servers/init/src/check.rs imports: main's Verity + our Steward.
- servers/init/src/check.rs `args` doc and signature: main's bundle_key parameter and key=/floor= text, plus our sentence on the steward's lines.
- servers/init/src/fuzz.rs (merged clean, wrong count): ENTRIES 12 -> 13 (main's erofsd + our steward).
- servers/init/tests/manifest.rs (merged clean, would not compile): our three args() calls take &BUNDLE_KEY.

## 9. rt: a mint at the caller's own root reads nothing
- docs/SECURITY.md R25 row: main's row (bench:littlefsd-label-check) with our host test added before bench:net-attacks.

## 12. steward: login, the session batch and the console session
- servers/init/tests/manifest.rs, the lines round trip: our filter of the steward's own `label` lines, with main's args(.., &BUNDLE_KEY).
- servers/init/tests/manifest.rs (merged clean, would not compile): one more args() call takes &BUNDLE_KEY; the test's alice-secrets Volume gains main's `verity: None`.

## 14. image: the steward and sshd on the box (every conflict here; main's names and values, our additions)
- image/manifest.json: main's manifest (littlefsd:data, erofsd:system, verity:system 5/100, beamlet entry) taken whole, then ours: the alice-secrets volume; keyd buckets=5; consoled heap 20; bootfsd 4096/3080/buckets=5 (it holds beamlet, public); blkd heap 38; ipd 7/74, scope=6:l:22, buckets=6; littlefsd:data stack 6 heap 20; littlefsd:alice-secrets added; erofsd:system buckets=5; beamlet entry replaced by steward (handed bootfsd, erofsd:system, littlefsd:data, littlefsd:alice-secrets, ipd) and sshd; public, labels, principals, steward, console. Session size follows BEAM8: sizes.session 10,880 pages (was 24,576), so alice's top is 43,528 (two 10,881-page sessions per label set).
- image/README.md: our three paragraphs, renamed (littlefsd, erofsd) and re-sized (43,528; 10,881).
- docs/SUMMARY.md: main's two new todo entries and our empty-a-budget.
- docs/servers/README.md, steward's holdings row: ours, with each `littlefsd`, `erofsd`.
- docs/servers/init.md, boot steps 5-6: ours (sshd before the public push, the steward after it) with main's littlefsd/erofsd.
- docs/userland/sessions.md, namespace figure: our solid edges and system-volume node, as littlefsd/erofsd.
- docs/testbench.md, "The memory budget": status = main's (beamlet-footprint) + our two steward cases; paragraph, table and beamlet paragraph provisional (main's newer verity/erofsd rows, beamlet's 10,880 budget) until the rescans after the rebase.
- tests/init-boot.toml: ours (1 GiB, steward lines, 14 badges/11 consoles/13 servers) with erofsd:system; stage ours.
- tests/userland-boot.toml: main's description with our steward sentence; our steward/carve/audit expects with main's `read from (erofsd:system|the boot pack)`; 1 GiB; stage ours.
- tests/userland-bad-start.toml, userland-read-only.toml: 1 GiB, stage ours; read-only's programs: ours (steward, sshd) + main's littlefsd-client.
- tests/verity-flipped-tree.toml, verity-wrong-root.toml: main's descriptions (erofsd) with our steward sentence; main's erofsd corrupt line + our two steward lines; 1 GiB; stage ours.
- tests/size-budget.toml: libs/rt 3525 and servers/init 2371 (main's growth + ours), to be re-measured.
- tools/testbench/src/case.rs, the recipe test: main's list + steward, sshd; beamlet is programs[12], erofsd programs[9].
- servers/init/tests/manifest.rs: ours (principals' keys, buckets, badges, arena and page sums without beamlet) with main's names and &BUNDLE_KEY; bootfsd buckets=5.

## After the rebase: adaptations to main (folded into their commits)
- steward (commit 12): the system volume is `erofsd:system` (slot 5, the session's handle, and beamlet's `endpoint=erofsd:system` argument); binding-table docs and test strings renamed littlefsd/erofsd; own.rs formatted.
- bootfsd commit (10): erofsd's client budget 1 MiB -> 1.5 MiB (six buckets of 225,280 bytes), test and erofsd.md sentence: every session's domain is a bucket at the system volume.
- init (commit 1): fuzz ENTRIES 13; tests pass &BUNDLE_KEY; alice-secrets Volume gains `verity: None`. Image commit: ENTRIES 14 (sshd); unverified-copy test reads full_image(); the steward's round-trip test passes a zero bundle key.
- image commit: sizes.session 10,880 (BEAM8) and alice 43,528; littlefsd:data and erofsd:system buckets=5, erofsd heap 26; steward-restart's programs littlefsd/erofsd; main's image cases re-aimed (boot-profile and boot-profile-unverified: steward and sshd programs, `started steward`, the steward stage; image-disk: steward, sshd, 1 GiB, 14 servers, client badge 8, steward stage; beamlet-footprint: its own copy of main's single-beamlet manifest, tests/data/beamlet-footprint/manifest.json); boot-profile's unverified manifest copy regenerated; init.md status count 28; sessions.md littlefsd-label-check; memory table and sshd/latency sentences from the post-rebase scans; the flood floods heap lists (beamlet-heap-flood's way) since BEAM8's budget lets 1 MiB binaries reach the backstop.

# Second rebase: onto main 2151b2aa4 (BEAM3, K24, scripts/q), from d9627acc8 (pre-rebase ref s2-pre-rebase2)
Commits 1-13 applied clean. Commit 14 (image) conflicts:
- README.md "Today": main's "the VM's file operations run over 9P in a boot" kept, then our steward/SSH sentence; remaining: native launching, leases, agents.
- docs/plan/m1-separation.md: client bullet = main's (Rerror by one table; dropped fids, calls by path...); VM bullet = main's (hub, files over 9P), its last clause now "Elixir's File in a session waits for the session's namespace to reach its VM"; steward bullet ours. Built list: main's shell sentence (files wait for a session's namespace) + our steward bullet. Not built: native launching, a session's files, walfsd, the steward's leases, approvals and the agent.
- tests/size-budget.toml: main's value plus our delta: libs/client 1026+48=1074, libs/rt 3518+6=3524, libs/wire 3153+453=3606 (re-measured by size-budget below).
- image/manifest.json, tests/data/boot-profile/manifest-unverified.json: consoled = BEAM3's stack 6 / heap 22 (ours was 5/20; rescanned below); the rest ours (no beamlet entry: BEAM3's beamlet 18/10,861 moves into the steward's session launch).
- tests/data/beamlet-footprint/manifest.json (our copy of main's single-VM manifest): git carried BEAM3's changes into it by rename detection; identical to main's image/manifest.json.
- servers/init/tests/manifest.rs, the image bound: ours (no beamlet); comment's launch term corrected to "the largest stack, 7 + 3" (the bound takes the largest stack_pages).
- docs/testbench.md memory paragraph and table: ours, with BEAM3's consoled row (10,384 / 6 / 11 / 22, the hub) pending rescan; the session-beamlet paragraph takes BEAM3's figures (18-page stack from 35,288 bytes; cap 10,861 = 3 over twice 5,429) and says beamlet-footprint scans the VM alone under its own copy of the single-VM manifest.
## After the second rebase (fixups folded into their owners)
- steward commit: SESSION_STACK_PAGES 17 -> 18 (BEAM3's measured 35,288 bytes); the session heap cap follows, 10,880 - 19 = 10,861.
