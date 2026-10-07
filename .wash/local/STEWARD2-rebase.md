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

# Third rebase: onto main 850bddcc3 (WFS2: walfsd serves the data volume as walfsd:data), from s2-pre-rebase3
- steward core commit, docs/servers/init.md: boot steps 5-6 = main's step 5 (each volume's walfsd, littlefsd or erofsd) + our step 6. The table had two `console` rows since the first rebase (ours and main's); merged into one: "...opens on the UART console ([steward]); it needs a `steward`, and a name that is not a `principals` entry refuses the boot" (init's check refuses both).
- steward server commit, image/boot.toml: main's walfsd entry, then our steward entry.
- steward login commit, init.md: our long steward row + the merged console row (main's duplicate row removed).
- image commit (all: walfsd:data in place of littlefsd:data, our additions kept):
  - image/manifest.json and tests/data/boot-profile/manifest-unverified.json: our entries, data volume = walfsd:data with WFS2's stack 14 / heap 64 and our buckets=5; the steward handed walfsd:data.
  - tests/data/beamlet-footprint/manifest.json: replaced by main's image/manifest.json at 850bddcc3 (it is the copy of main's single-VM manifest).
  - image/disk.toml (merged clean): header comment rewritten for two formats (data walfs by libs/walfs, alice-secrets littlefs by littlefsd's code).
  - image/README.md: ours with walfsd:data; disk bullet: data walfs via libs/walfs (walfsd's packer), alice-secrets littlefs.
  - docs/servers/README.md: holdings row "a badge at bootfsd, each volume's walfsd or littlefsd, erofsd and ipd"; graph: our solid edges + ST -- connections --> WF.
  - docs/servers/init.md steps 5-6: ours, with "each volume's walfsd, littlefsd or erofsd" and "badges at bootfsd, each volume's server and ipd".
  - docs/testbench.md: our paragraph + main's walfsd:data sentence; table ours with main's walfsd:data row (28,616 / 14 / 15 / 64) in place of littlefsd:data.
  - docs/userland/sessions.md figure: ours (solid) with walfsd for the home volume. docs/SUMMARY.md: main's "four times over" + our empty-a-budget. docs/plan/m1-separation.md Not built: native launching, a session's files, the steward's leases, approvals and the agent (walfsd now built).
  - tests/image-disk.toml: merged description; 14 servers; started walfsd:data; client handed walfsd:data badge 8; programs gain littlefsd back (alice-secrets).
  - tests/init-boot.toml: ours, comment names walfsd:data's and littlefsd:alice-secrets's ranges.
  - servers/init/tests/manifest.rs: ours renamed to walfsd:data; bound's largest stack now walfsd:data's 14 + 3; main's a_walfsd_entry... test expects buckets=5 (our image).
  - tests/size-budget.toml servers/init 2142+1 (main) +229 (ours) = 2372; tools/testbench/src/case.rs: walfsd, steward, sshd before beamlet, programs[13].
  - Cases booting the image's manifest gain the program they lacked: littlefsd in boot-profile, boot-profile-unverified, userland-read-only; walfsd in steward-restart.
## After the third rebase (folded)
- init fuzz ENTRIES: 14 at the steward core commit (main's walfsd + steward), 15 at the image commit (+ sshd).
- steward login commit's the_steward_s_own_lines_bind_homes_vaults_and_scopes: walfsd:data home, littlefsd:alice-secrets vault (program littlefsd); was fsd:* at that commit before the image commit renamed it.
- Pages: steward.md's two "STEWARD3" mentions reworded; sessions.md's two "(BEAM3)" mentions -> "its namespace to reach the VM"; shell.md "The shell in a session" status: started by the steward on the UART and per SSH login, launching not built, files wait for the namespace, + bench:steward-ssh-two-principals.
- The beamlet limit commit moved before the image commit (the flood case passes at the commit that adds it).

# Fourth rebase: onto main f820b6ba3 (B18; K23's kernel budget_reap), from s2-pre-rebase4
- image commit, docs/kernel/budgets.md residuals: main's "A lost budget is carved until its parent is reaped" (budget_reap) + ours reworded: budget_reap can empty users of a dead steward's carves, init does not call it before the restart, so it still reboots. steward.md's residual and docs/todo/empty-a-budget.md (retitled "A dead steward's carves are emptied before its restart"; the kernel call exists, init's use remains, K23's) and its SUMMARY entry follow.

# Fifth rebase: onto main b60c7cc5c (K25, B14, B21, MODEL1, BEAM4, K19, B8), from s2-pre-rebase5 (fixups folded first on f820b6ba3)
range-diff: commits 1-14 "=" (patch-identical); only the image commit "!" (conflicts below). The beamlet limit commit applied clean over BEAM4's userland/otp changes.
- docs/plan/m1-separation.md VM bullet: main's (system natives built, host-tested, no boot runs them), its File clause "waits for the session's namespace to reach its VM"; steward bullet ours; Not built: a session's files, the steward's leases, approvals and the agent (native launching dropped: main's natives).
- docs/userland/shell.md "The shell in a session" status: ours + main's "launching a program is tested on the host only" and its host test.
- tests/size-budget.toml: main's + our deltas: libs/client 1030+48=1078, libs/rt 3522+6=3528, libs/wire 3170+452=3622 (re-measured by size-budget).
Folded before it (from the f820b6ba3 gate): libs/client/tests/console.rs (BEAM3's) answers consol's ended as refused (consol commit); beamlet lib.rs formatting (beamlet commit); size ceilings sshd 1102, wire 3605, bootfsd 228 (image commit); the bootfsd commit reworded "bootfsd, erofsd: client budgets with room for every session's domain".

# Sixth rebase: onto main 226245507 (B9 beamlet 11,008, B20 tests, BEAM9 rt copy-out + ConsoleIo retry), panel folds first
Folded on b60c7cc5c first: red P1 (servers/steward/src/watchers.rs pool; steward.md), red P2 (sshd listener::again on timeout/too_many; sshd.md), editor's two notes, size ceilings sshd 1134 / steward 1170 + the image message's size lines. Autosquash conflict: the watcher test line on steward.md's "Authentication and sessions" status belongs to the image commit (where the section becomes built), not the login commit; placed there.
Rebase conflicts, all in the image commit:
- image/manifest.json, boot-profile's unverified copy: ours (no beamlet entry), session 10,880 -> 11,008 (B9's beamlet budget), alice 43,528 -> 44,040 (4 x (11,008 + 2)); the steward's heap cap follows (11,008 - 19 = 10,989).
- tests/data/beamlet-footprint/manifest.json: main's image manifest at 226245507 again.
- docs/kernel/budgets.md: ours with 44,040, 11,008, peaks 5,475 / 5,274, and B9's margin sentence (5,495; "as it did from 10,880 when the peak reached 5,475").
- docs/testbench.md: ours; session paragraph 10,989 of 11,008, 39 pages over twice 5,475, limits 688 pages. (A first pass left conflict markers in; caught by grep and fixed in the same fold.)
- servers/init/tests/manifest.rs: ours. tests/size-budget.toml libs/rt 3530 (main) + 6 = 3536.
- image/README.md and the image message: 44,040 / 11,009 / 10,989.
range-diff 10bda633a -> head: 1-11 and 14 "=", 12 (steward login: watchers), 13 (sshd: again), 15 (image) "!".
