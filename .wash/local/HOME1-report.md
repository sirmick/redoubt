# HOME1 report (home1-implementer)

Branch wp-HOME1, worktree .worktrees/HOME1, base origin/main 8e50b5489, head 3c3df9e47. Clean tree.

## Commits
- 98fee65b7 testbench: a manifest's volume bytes are held to their partition, at the pack and at a
  case's load (tools/testbench/src/{disk.rs,case.rs}, image/disk.toml).
- 3c3df9e47 init, steward: a principal's home holds its manifest quota, shared by all its sessions
  (servers/init/src/{manifest,check,refusal}.rs, servers/init/tests/manifest.rs,
  servers/steward/src/{own.rs,bin/steward.rs}, servers/steward/tests/own.rs,
  libs/fileserver/src/quota_tests.rs, image/manifest.json, tests/data/{boot-profile/manifest-unverified,
  steward/vault-launch.manifest,init/overcommit}.json, tests/{steward-home-quota,init-refuses-overcommit,
  steward-restart,size-budget}.toml, docs/servers/{init,steward,walfsd}.md, docs/testbench.md,
  docs/userland/sessions.md, docs/plan/m1-separation.md). Size budget lines for servers/steward
  (1212->1256) and servers/init (2439->2486).

## Changes since the predecessor's handoff
- Rebased; the predecessor's testbench session `name` dropped: main has its own (efe54342d).
- Steward stack 7 -> 8 pages (image manifest and its two copies, testbench.md memory table): the
  console session's home now binds through the carve; rv64 peak 14,456 B (was 13,224).
- manifest.rs doc table lists `bytes` and `home_quota`; stale "Home and vault quotas" cites fixed;
  init.md's Home quotas subsection moved below the boot-manifest section's own closing paragraphs.
- New host test testbench::a_case_s_manifest_is_held_to_its_disk (case-load hold).
- M1 progress: one sentence on home quotas.

## Gates (all through scripts/q / jobs.mk, prebuilt from the committed tree)
- cargo test -p redoubt-init -p redoubt-steward-server -p redoubt-fileserver -p testbench: rc 0.
- New cases: steward-home-quota PASS rv64+rv32 (twice: on 1443667ac and 3c3df9e47);
  init-refuses-overcommit PASS rv64+rv32. Verdicts: alice SSH `{:second, {2816, {:error, :enospc}}}`
  beside her console session's 5 MiB, bob `{:bob, 5120}`, alice `{:after_rm, 4096}`; init's refusal
  line + status 255.
- Gate set (.tmp/HOME1/gate-cases.txt, 83 cases: scripts/shell-cases origin/main + every init-*,
  steward-*, sshd-*, walfsd-* case + image-disk, userland-boot, boot-profile-unverified,
  unsafe-budget, size-budget, formatting, docs, no-cruft), both widths, on 1443667ac: 140 PASS, 5 FAIL.
  4 FAIL = steward stack (fixed above); steward-model-host-tests = steward_policy past its 35 s deadline
  under a 12-way gate (host-clock: no verdict).
- Rerun on 3c3df9e47: userland-boot, userland-read-only, steward-session-ends,
  steward-ssh-two-principals, init-boot, boot-profile-unverified, steward-vault-launch,
  steward-home-quota (both widths), docs: all PASS; steward-model-host-tests alone PASS (147.9 s).
  The rest of the gate is not rerun on 3c3df9e47: the amend changed only the steward's stack_pages
  in three manifests and docs.
- No #[path] includers of steward/init sources outside their crates (fileserver's quota_tests.rs is
  included by quota.rs, run).

## Attack cases, verdicts from the system
- steward-home-quota: the enospc is walfsd's refusal as the shell prints it; sizes are walfsd's.
- init-refuses-overcommit: init's bare refusal line, before any server starts, and the firmware's 255.

## Summaries checked
README.md, GETTING-STARTED.md (no quota/home claims), docs/SECURITY.md (R48 row unchanged: still
true), docs/plan/m1-separation.md (updated), m3-agents.md (shared-pool attack still "not yet": a
different attack), docs/userland/README.md (no claim affected), init/steward/walfsd/sessions/testbench
pages (updated).

## Found, not changed (outside HOME1)
- m1-separation.md: "Not built: a session's files, and a steward restart without a reboot." is stale
  (both shown built), followed by a broken "[R2 ...]" fragment; "Remaining work" still lists the
  steward restart as to come; "Elixir's File in a steward's session waits for the namespace" is stale
  (steward-home-quota writes with :file in sessions).
- steward.md Failure and restart status names littlefsd:alice-secrets (the image's is walfsd).

## Open risks
- Before HOME1, sessions' homes were minted with quota 0 (read/remove only); with HOME1 every
  session can write up to its principal's quota. A walfsd restart kills the steward's kept carve as
  it already killed init's handed connection: home binds fail until the steward restarts (unchanged).
- walfs's per-entry share makes the usable quota a little under home_quota (7,936 of 8,192 KiB here).

## Round 2 (steward red: OK with notes), head 55d85d5ff on origin/main 8fa39c5ad
- Rebased onto 8fa39c5ad (no conflicts); a3c2a9bae (bench hold) + 55d85d5ff (HOME1, notes folded).
- P2 fixed: init refuses a home equal to another principal's on its volume, inside it or holding it
  (Why::SharedHome at principals[i].home, same pass as SharedVault). Test
  host:redoubt-init::no_home_is_another_s_or_inside_it (nested both ways, equal, "/", and
  /home/al beside /home/alice passes); listed in init.md and steward.md.
- walfsd.md Quotas: a home holds a little under home_quota (7,936 of 8,192 KiB in the case).
- steward-home-quota timeout_secs 1800 -> 610 (passes 238-304 s; twice the slowest).
- Size budget servers/init 2486 -> 2500 (line in the commit).
- Gates on 55d85d5ff: cargo test init/steward-server/fileserver/testbench rc 0; prebuilt rc 0;
  init-refuses-overcommit, init-host-tests, docs, size-budget, formatting and every steward-* and
  sshd-* case on both widths: 44 PASS, 0 FAIL (steward-home-quota 243 s rv64, 238 s rv32);
  steward-model-host-tests alone PASS.

## Round 3: rebased onto origin/main 76fd2b9bd (RCMD1), head 98e8d8f9f
- 0892fb070 (bench hold) + 98e8d8f9f (HOME1). One conflict, tests/size-budget.toml: main raised
  servers/steward to 1224; HOME1's 44 lines make 1268 (size-budget PASS at 1268/1268, init 2500/2500).
  own.rs/steward.rs merged on their own: RCMD1's session_args and HOME1's How::Carved both kept; merged
  regions read.
- Gates on dd8270570 (98e8d8f9f's tree but one docs line): host tests rc 0; prebuilt rc 0;
  steward-home-quota, init-refuses-overcommit, every steward-*/sshd-* case, init-host-tests, docs,
  size-budget, formatting, userland-boot, userland-read-only, init-boot on both widths: 50 PASS,
  0 FAIL; steward-model-host-tests alone PASS.
- Steward stack peak with RCMD1's launch arguments is 14,856 B (of 8 pages = 16,384 x 2 bound
  holds); testbench.md's memory table and the commit message updated to it; docs PASS on 98e8d8f9f.
