# AGT1: the `agent` principal in the image

Branch wp-AGT1, worktree /home/mcloonan/redoubt/.worktrees/AGT1. Base main 3a80e5af0, head 0771a430f
(one commit). Not pushed.

## Delivered

- `image/manifest.json`: principal `agent`, account 1003, budget 24,576 pages / 20 processes /
  weight 500, test key, `home: data:/home/agent`, `home_quota` 8,388,608 (bob's); no `labels`,
  no `label_sets`, no `net`. Bucket counts: keyd 5→6, consoled 4→5, bootfsd 5→6, ipd 6→7,
  walfsd:alice-secrets 4→5, walfsd:data 5→6, erofsd:system 5→6 (see "Buckets").
- Copies of the image manifest kept faithful: `tests/data/steward/vault-launch.manifest.json`
  (shell-commands, steward-vault-launch) and `tests/data/boot-profile/manifest-unverified.json`
  (checked equal to the image less verification by init's host test). Not touched:
  `tests/data/steward/idle.manifest.json` and `tests/data/pipe/manifest.json`, older two-principal
  variants that still boot as they are (no test holds them to the image).
- `tests/keys/agent`, `agent.pub` (NOT-FOR-PRODUCTION, ssh-keygen ed25519).
- `launch`: `--agent-key PUB`; `tools/testbench/src/launch.rs`: `AGENT`, a second development
  key `.tmp/launch/agent_ed25519` made once, `give_key(manifest, principal, key)`, `ssh_line(port,
  principal, identity)` and `ssh_lines` (alice's then agent's) through the one print in `system()`
  and `run()`; `case.rs` `Boot.agent_key`; `main.rs` `--agent-key`, `prepare` takes a list of
  (principal, key). The launch-system case still gives alice alone the case key; agent keeps its
  test key there (its sessions log in as alice only).
- `mkimage` and `launch::command` stage `home/agent`; `tests/data/steward/stage/home/agent/hello`.
  mkimage's header now says 1 GiB (it said 512 MiB; the README and the cases say 1 GiB).
- `servers/walfsd/src/server.rs:39`: client `BUDGET` 2 MiB → 3 MiB, with the arithmetic in its
  comment (orchestrator's answer to question 47f0595, option (a)). `servers/erofsd/src/server.rs`
  comment only: its count sentence restated for four domains (6 fit, unchanged constant).
- `tests/agent-login.toml` (rv64, rv32). Extended, never loosened: `tests/init-boot.toml` (keyd
  holds none of the 4 keys), `tests/userland-boot.toml` (carved users/agent account 1003),
  `servers/init/tests/manifest.rs` (keys list, bucket vector, bootfsd/walfsd arg strings, the
  three-home quota arithmetic), `servers/steward/tests/steward.rs` (names alice, bob, agent).

## The budget arithmetic

- A session is 11,264 pages, its budget's own cost 1: two sessions 22,530. Share = pages / label
  sets − 1. Alice 47,624 → 23,811 per set (2 sets), 1,281 over two sessions; bob 32,768 → 32,767,
  10,237 over; agent 24,576 → 24,575, 2,045 over: one login and a second context (init's default
  `contexts.max` = 24,575 / 11,264 = 2; 20 processes / 10 = 2).
- Top budgets 47,624 + 32,768 + 24,576 = 104,968, +3 own pages = 104,971, of `users` =
  192,723 pages on rv64 at 1 GiB (192,600 on rv32; the kernel's `Budgets:` line). No re-balance
  of alice or bob was needed; image/README.md says so and shows the sums. Processes 80 of 382.
- Homes: 3 × 8,388,608 = 25,165,824 ≤ data volume 33,537,024 bytes.
- init's sizes check: every size ≤ 24,575 pages, 20 processes, 500 weight. Passes (init-boot,
  the_steward_s_sizes_fit_every_principal_s_smallest_share).

## Buckets

init counts every declared domain at every shared server plus its system callers. Three
principals declare four domains (alice {} and {alice-secrets}, bob {}, agent {}). init's need:
keyd 6, consoled 5, bootfsd 6, ipd 7, walfsd:data 5, walfsd:alice-secrets 5, erofsd:system 5
(host test the_image_manifest_passes_and_its_plan_is_what_the_boot_follows). walfsd:data and
erofsd:system carry 6, as they carried one spare before: image-disk hands a client a walfsd:data
badge and userland-read-only one at erofsd:system, each a sixth caller. walfsd at 6: 6 × 364,544 =
2,187,264 > 2 MiB, hence BUDGET 3 MiB (8 fit, 2,916,352 = 712 pages; 768 pages of the entry's 1,024
beside heap 64 and stack 14: 846 ≤ 1,024). erofsd at 6: 6 × 225,280 = 1,351,680 ≤ 1,572,864, its
constant unchanged. The first attempt bumped all seven by one (walfsd:data 6 refused by walfsd);
the second put walfsd:data/erofsd back to 5 and image-disk/userland-read-only refused the boot
at servers[6] (`buckets=5 is below the 6 domains`); the third is what is committed.

## Gates (short gate, Tier A), all under q; last full run after the final edits except
docs/kernel/budgets.md (one sentence; `docs` rerun after it: PASS)

- `make -f scripts/jobs.mk prebuilt`: rv64 300, rv32 286 pieces, 0 failed.
- `make -f scripts/jobs.mk build-rv64 build-rv32 docs`: exit 0; docs PASS.
- `make -k -j4 -f scripts/jobs.mk set WIDTHS=rv64,rv32 CASES=...`: exit 0, every case PASS on
  both widths: agent-login (9.2 s / 8.8 s; agent's prompt 2.38 s / 2.20 s after ssh started,
  alice's 1.66 / 1.45), image-disk, userland-read-only, launch-system (smp 4), steward-ssh-two-
  principals, shell-commands, steward-vault-launch, userland-boot, init-boot, boot-profile-
  unverified, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page (smp 1 and 4),
  beamlet-footprint, steward-home-quota, walfsd-quota, walfsd-boot; host: walfsd-host-tests,
  fileserver-host-tests, host-tests (testbench among them: 8 launch tests), init-host-tests
  (64), steward-host-tests; formatting, no-cruft, size-budget, unsafe-budget (unchanged counts:
  paging 12, loader 18, kernel 11+10+16, redoubt-sys 3, 0 undocumented).
- `git diff --check` clean; `cargo +nightly` rustfmt on each touched Rust file.
- Not run: the whole bench (the train's), rv32 beyond the cases above (builds-rv32 built).

## Rule F

agent-login's verdicts: the steward's `carved users/agent account 1003: {}` and audit line
(principal 2), sshd's `login agent: ... labels []`, `users/agent/{} holds N pages, 10 processes`,
and the shell's tagged values from the namespace the steward gave the session (`/home/agent`
present, `/net` absent, alice's home enoent); alice's session in the same boot is the positive
control (`/net` present). No pattern can match the echo: every answer is tagged.

## Summaries checked

- image/README.md: rewritten budget passage (was stale at 11,009/11,905), principal, homes.
- GETTING-STARTED.md "Try the system": `--agent-key`, the two ssh lines, "The keys".
- docs/userland/sessions.md: the context-count sentence names agent and its purpose.
- docs/servers/steward.md:607: "and bob and agent two each".
- docs/servers/init.md Sizing: one sentence with the counts under the rule.
- docs/kernel/budgets.md:216: the top budgets name agent's 24,576.
- docs/plan/m3-agents.md Progress: the principal exists ahead of leases, `agent-login`.
- Unchanged, checked: docs/testbench.md:1222 (alice alone holds the case key: still true),
  docs/userland/agents.md (brief: nothing), README.md and docs/README.md (name no principal),
  docs/servers/sshd.md (no principal count), image/disk.toml (no change needed: the stage is a
  directory tree), docs/servers/walfsd.md (states no client budget figure).

## Open

- The owner's brief named the steward's `login agent: labels {}` line; the real lines are sshd's
  `login agent: ... labels []` and the steward's audit line; the case uses those.
- `tests/data/steward/idle.manifest.json`, `tests/data/pipe/manifest.json`: two-principal copies,
  left as they are.
