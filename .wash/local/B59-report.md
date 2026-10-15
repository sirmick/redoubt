# B59 report: a session starts in its home

Worktree `.worktrees/B59`, branch `wp-B59` from main db6575a3f.

## Design

**Where the start directory is set.** In the shell, not the VM: `Redoubt.Shell.run/1` enters
the session's home once, after the banner and before the first prompt. The VM's cwd default stays
`/` (userland/otp/vm/src/vm.rs:575): it is what a VM that is no session, a tester's or a child
under `init`, has, and the shell is what knows it is a session.

**How the shell knows the home.** The steward tells it, as it tells the principal: one more
launch argument, `home=/PATH`, the path it bound the principal's home at (the same `h.path` slot
1 binds, servers/steward/src/own.rs `binding`), for console, SSH and vault sessions alike, since
`binds` gives slot 1 to labelled and unlabelled sessions both. A principal with no `home` line
gets no `home=` argument. beamlet reads it with the other identity arguments (`identity` in
userland/otp/redoubt/src/lib.rs: at most one, a clean absolute path, else the identity is
malformed and nothing is told) and `redoubt:identity/0` carries it as `home => Path | nil`.
Information, not authority, like `principal=`: the shell can only `cd` where its namespace reaches.
Rejected: the shell guessing `/home/<principal>`; the path is the manifest's (`home =
"data:/home/alice"`), and a second copy of it in the shell would drift.

**What the start does.** `Redoubt.Shell.enter_home/1`, given the identity:
- a VM that is no session (`nil`, the host): nothing, silently; the shell starts where the VM
  did, which the host tests rely on;
- a session with a home: `File.cd(home)`; the prompt shows it, as it shows every cwd;
- a session with no home, or whose home cannot be entered (`File.cd` fails: beamlet's
  `set_cwd` stats the path through the namespace, so an absent or unreadable home is an error):
  one line, `** no home to start in: <reason>; this session starts in /`, and the cwd stays the
  VM's, `/`.

**Case.** `shell-commands` (both widths, the plain and the vault session over SSH): the plain
session's first lines are `pwd()` = `"/home/alice"`, `File.write("b59.txt", "x")` and
`File.read("/home/alice/b59.txt")` = `{:ok, "x"}`; the vault session's `pwd()` is `"/home/alice"`
too (its home is still its home; writing there is the label's refusal, already in the case).
`launch-system` expects the prompt as `/home/alice (N)> ` on the owner's path. The negative, a
session with no home, is the host test of `enter_home/1` in `test/redoubt/shell_test.exs`, where
the identity is given; the steward never launches a session with an unreadable home on the bench.

**Pages.** docs/userland/shell.md "The shell in a session" states the start; sessions.md "What a
session is told" and beamlet.md `identity()` gain `home=`; steward.md's argument list too;
GETTING-STARTED.md's "A session starts at `/`" line goes.

## Branch

wp-B59 on main db6575a3f, two commits:
1. 42acd57e4 steward, beamlet: a session is told where its home is bound, as home=/PATH among its
   arguments (servers/steward/src/own.rs, tests/own.rs; userland/otp/vm/src/platform.rs,
   bif/system.rs, tests/system.rs; userland/otp/redoubt/src/lib.rs, tests/system.rs;
   docs/servers/steward.md, docs/userland/sessions.md, docs/userland/beamlet.md)
2. 8fa0fb5c6 shell: a session starts in its home, and says so once when it has none
   (userland/shell/lib/redoubt/shell.ex, shell/session.ex, shell/helpers.ex,
   test/redoubt/shell_test.exs; tests/shell-commands.toml, tests/launch-system.toml;
   docs/userland/shell.md, GETTING-STARTED.md)

## Gates (commands as run, exit codes)

Shell exports and `scripts/q` as .wash/local/launch-env-2026-10-12.md says; logs under
$REDOUBT_TMP/B59/.

- `q run --cores 4 -- cargo test -q -p redoubt-steward-server --test own`: 4 passed, exit 0
- `q run --cores 4 -- sh -c 'cargo test -q -p beamlet-vm --test system && cargo test -q -p
  beamlet-redoubt --features fake --test system'` (in userland/otp): 17 and 20 passed, exit 0
- `mix format --check-formatted` (userland/shell): exit 0; `rustfmt +nightly --check` on each
  changed Rust file: clean after one reflow of userland/otp/redoubt/tests/system.rs
- `make -f scripts/jobs.mk docs`: first run exit 2, C2: sessions.md:266 named the steward test
  by its old name (renamed to ..._context_and_home); fixed and folded into commit 1; rerun below
- `make -f scripts/jobs.mk docs` (rerun on the folded commits): PASS docs, rc=0, exit 0
- `q run --cores 4 -- tools/difftest` (userland/otp): 527/527 passed, 0 failed, 21 skipped by
  design, exit 0
- `q run --cores 8 -- ./test-shell`: every stage passed (352 tests on each VM, 0 failures), exit 0
- `make prebuilt && make -k -j8 set CASES="$(scripts/shell-cases)"` (beamlet's set: 54 cases,
  both widths), first run: prebuilt rc=0; killed at the 2 h background limit with 45 of 108
  verdicts, 22 PASS. Two real findings, both mine:
  1. `job-interrupt-ssh` rv64: `beamlet: home=/home/alice is not on the userland volume`.
     beamlet's main takes the first argument it does not know as the module to start, and the
     steward appends `home=` after `context=`, so `home=/home/alice` became the module: every
     steward session failed to start, and the other rv64 failures (timeouts waiting for the
     shell's banner, boot-profile, launch-system, the steward's OutOfMemory step on restart
     loops) follow from it. Fix: beamlet.rs skips `home=` as it skips `principal=`, `label=`
     and `context=` (folded into commit 1, with its usage comment).
  2. `beamlet-footprint` rv64: heap needs 11094 pages for twice its 5547-page peak, capped at
     11092. The start read the identity through `Redoubt.Shell.Session`, which loaded a
     command's module at the prompt, one page over the peak the cap allows (5,546). Fix: the
     shell reads `redoubt:identity/0` itself (folded into commit 2); `Session.identity/0` stays
     public with its doc (unused by the start).
  Rerun after the fixes as `prebuilt`, then `rv64/beamlet-footprint rv32/beamlet-footprint`, then
  the rv64 cases (-j6) and the rv32 cases (-j6) as two jobs: below.
- `q run --cores 8 -- ./test-shell` (rerun after the repairs): every stage passed, 352 tests on
  each VM, 0 failures, exit 0
- `make prebuilt` rc=0; `make rv64/beamlet-footprint rv32/beamlet-footprint`: both PASS (the rv64
  peak back under the cap). Then the rv64 cases (-j6): 50 of 53 case targets rc=0 across the two
  logs... precisely: rv64 49 PASS + footprint, rv32 47 PASS + footprint; the same 5 cases FAIL on
  both widths: pipe-hostile-output, userland-read-only, boot-profile, boot-profile-unverified,
  userland-boot, each "timed out waiting for /^\[con ..\] / \([0-9]+\)> .../": the console
  session's prompt is now `/home/alice (N)> ` (image/manifest.json's console is alice), which is
  the package's point. Their expect lines now read `/home/alice \(`, folded into commit 2; no
  other verdict depends on those files.
- Rerun of those five on both widths after `make prebuilt`: below.
- `make prebuilt` rc=0, then the five on both widths: all 10 rc=0 (pipe-hostile-output,
  boot-profile, boot-profile-unverified, userland-read-only, userland-boot), exit 0
- `q run --cores 4 -- ./launch --system --print-only --ssh-port 2259`: builds, prints the QEMU
  command and the ssh line, exit 0 (port 2222 was another member's; --print-only boots nothing)
- Not run: a whole bench (shell track: the gate list is scripts/shell-cases + ./test-shell +
  difftest); beamlet's Miri and the unsafe ratchet (no unsafe touched; beamlet's crates forbid it)

## Final branch

wp-B59 on main db6575a3f, head 05d01abcd:
1. 8e7ce4853 steward, beamlet: a session is told where its home is bound, as home=/PATH among its
   arguments
2. 05d01abcd shell: a session starts in its home, and says so once when it has none
Every verdict above after the first set run is on these two commits' content (the case-file
fix changed only the five cases' expect lines).

## Affected summaries checked

- README.md, docs/README.md, docs/userland/README.md, docs/servers/README.md, image/README.md,
  docs/plan/m2-usable-shell.md, userland/shell/help/*.md: grep for the start directory, `/`,
  cwd, `/home/alice`; no claim about where a session starts (userland/README.md's example already
  shows the `/home/alice (1)>` prompt; m2's "the directory a session starts in is neither read nor
  run" still holds). No change.
- GETTING-STARTED.md "Try the system": said "A session starts at `/`, which nothing writes";
  now says it starts in its home, with `ed("notes.txt")` and `fm()`. Changed.
- docs/userland/shell.md "The shell in a session": the start stated, status line names
  bench:shell-commands and bench:launch-system and the host-only negative. Changed. Its
  "Files and text" and the Helpers moduledoc say relative names resolve against the working
  directory; the moduledoc now names the home as the start. "Saved history" (planned) untouched.
- docs/userland/sessions.md "What a session is told": `home=/PATH`, its malformed forms, the
  renamed steward test. Changed. Its examples already showed `/home/alice (N)>`.
- docs/userland/beamlet.md: `identity()`'s map and the natives table gain home. Changed.
- docs/servers/steward.md "Authentication and sessions": the argument list. Changed; the manifest
  lines and home-quota sections already say a session may have no home.
- docs/kernel/budgets.md (the footprint cap): unchanged, the peak stays under 5,546.

## Design problems and risks

- beamlet's main takes its first unknown argument as the module: every new identity argument must
  be added to its skip list (done for home=; found by job-interrupt-ssh). A positive match (`MODULE`
  as the last argument) would be sturdier; not this package's.
- The footprint cap has one page of headroom on rv64 (peak 5,546 of a 5,546 bound): anything the
  shell loads at the prompt now moves the image's share a 128-page step (docs/kernel/budgets.md).
- A session whose home the steward bound but whose carve the volume refused (steward.md "the
  volume's server refuses a carve its room cannot hold all the same, and a session then has no
  home") prints the one line and starts at `/`; untested on the bench (no case makes such a home).
