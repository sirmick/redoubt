# WEIGHT1 report: the image runs the steward at 1,000 under `system`

Branch wp-WEIGHT1, worktree /home/mcloonan/redoubt/.worktrees/WEIGHT1. Base 453a48bfc, head
f75f01d94. Two commits:

- 48532c57c `image: the steward runs at weight 1,000 under system`: manifests, the latency
  cases' comments, the scheduling.md sentence.
- f75f01d94 `steward.md: a launching lease runs at its agent weight`: the Q2 rule sentence.

Never pushed. Logs are under /home/mcloonan/redoubt/.tmp/WEIGHT1/: short.log, timing.log,
fig-branch.log and fig-branch/, fig-base.log and fig-base/, console/, logs/.

## Manifests changed (the steward entry's budget weight 100 → 1000, nothing else)

I found these by parsing every `*.json` in the tree for a server entry named `steward`:

- image/manifest.json (line 84)
- tests/data/boot-profile/manifest-unverified.json
- tests/data/pipe/manifest.json
- tests/data/steward/boot.json
- tests/data/steward/confined.manifest.json
- tests/data/steward/idle.manifest.json
- tests/data/steward/many-sessions.manifest.json
- tests/data/steward/six-principals.manifest.json
- tests/data/steward/three-contexts.manifest.json
- tests/data/steward/vault-launch.manifest.json

The other two entries:

- tests/data/steward/latency.manifest.json was already at 1000 and is unchanged.
- No manifest runs the steward at 100 on purpose: no case is about that weight.

The ipd entry in image/manifest.json was not touched (IPD2's hotspot).

## `system`'s sum

The image's servers' weights sum to 4,900 at the base and 5,800 now (+900), against `system`'s
250,000. The 13 servers are:

- keyd 100, consoled 1000, bootfsd 100, blkd 1000, netd 1000, ipd 100
- walfsd:data 100, walfsd:alice-secrets 100
- blkd:system 1000, verity:system 100, erofsd:system 100
- steward 1000, sshd 100

servers/init/tests/manifest.rs reads the image, and `init-host-tests` passes. Its system-fit test
asserts a weight of 4600. That figure is for the test's `image()`, which leaves out the steward,
sshd and walfsd:alice-secrets, so the test does not depend on the steward's weight.

## Deleted and edited sentences

- "(the image's is 100)" is gone from the six cases that carried it: steward-latency,
  steward-latency-n4, steward-latency-n16, steward-deadline-burst, steward-deadline-burst-16 and
  steward-launch-overlap. Each now reads "1000 under `system`, the image's own: the workload the
  page's targets were set at".
- docs/kernel/scheduling.md, "With the real steward": "The steward runs at the workload's weight,
  1,000 under `system`, the image's own, so the targets are measured at the weight the image runs
  the steward at." The brief's path docs/servers/scheduling.md does not exist; the page is
  docs/kernel/scheduling.md.
- docs/servers/steward.md, Residual risks, the launch bullet ("Steward work is paid by the
  steward"): one sentence states Q2's rule and its reason. It says the launch's measured cost
  and the derived bound stand.

## Summaries checked

Searches run over docs/, README.md and GETTING-STARTED.md:

- `weight 100`, `at 100`, `image's`, `steward…weight`
- the system sums: 4,900, 4,600, 250,000

| Page | Passage | Result |
| --- | --- | --- |
| docs/servers/init.md | Weights, lines 172-180: "init, the steward and the drivers … 1000 against a principal's 100" | Now true; no figure moved, no edit |
| docs/kernel/budgets.md | Lines 259-262, 294 and 917: the steward gets 1,000 / large weight | Now true, no edit |
| docs/kernel/scheduling.md | Lines 6 and 1706-1713: "the steward, whose weight is large" | True, no edit |
| docs/kernel/scheduling.md | Line 869 | Edited, above |
| docs/servers/steward.md | Line 1259: "a large manifest weight" | True, no edit |
| docs/SECURITY.md | R12 row | Names no weight, no edit |

No other page names the image's steward weight. The 250,000 lines (scheduling.md:56,
budgets.md:129/150/259, model.md:88) are `system`'s size, which is unchanged.

## Gates (all exit 0)

The short gate:

- `make -f scripts/jobs.mk -j2 build-rv64 build-rv32`: rc=0, rc=0
- `make -f scripts/jobs.mk prebuilt`: rc=0
- `make -k -f scripts/jobs.mk -j16 set CASES="docs formatting size-budget unsafe-budget no-cruft init-host-tests userland-boot init-boot bench-net-peer ipc-outcomes sum-clear lend-untouched-page"`:
  rc=0, every run PASS. That covers both widths, and lend-untouched-page at smp 1 and 4 on both.

The timing cases and the manifest smoke, on both widths:

- `make -k -f scripts/jobs.mk -j16 set CASES="steward-end-lease steward-lease-ends agent-login deadline-in-launch sshd-login-bound steward-six-principals steward-restart boot-profile beamlet-reduction-rate beamlet-schedulers-one-hart session-tcp-echo shell-compile init-boot launch-system steward-ssh-two-principals"`:
  rc=0, 32 of 32 runs PASS (deadline-in-launch at smp 1 and 2).

Re-run for the page figures with logs kept, `set` one case at a time, -j2, every run PASS:

- boot-profile, boot-profile-unverified, launch-system, steward-ssh-two-principals and
  steward-end-lease, on both widths.

Not run, by the orchestrator's ruling: the other manifest users. The train's whole bench judges
them (92 cases in /home/mcloonan/redoubt/.tmp/WEIGHT1/gate.txt, less those run above). The
latency and burst cases run on latency.manifest.json, which is unchanged, so they were not re-run.

## Page figures compared

### Logins (docs/servers/sshd.md, the 5 s login bound)

The page records 2.01 / 1.70, 2.00 / 1.78 and 2.41 / 2.06 s, with the bound at 2 × 2.41 → 5 s.
Measured from `ssh`'s start to the prompt, host clock, rv64 / rv32:

| Login | rv64 | rv32 |
| --- | ---: | ---: |
| launch-system | 1.97 s | 1.89 s |
| steward-ssh-two-principals, alice | 1.82 s | 1.78 s |
| steward-ssh-two-principals, bob | 2.23 s | 2.09 s |

Earlier, under the 16-job load: rv32 launch-system 1.89 s, alice 1.81 s, bob 2.05 s. Every login
is under the 2.41 s worst the bound derives from. No edit; the bound does not move.

### End-lease

steward.md's 589 ms is `steward-latency`'s `served end_lease` for a lease carrying a beamlet VM.
That case runs on the unchanged latency manifest and was not re-run. The `served end_lease`
lines in `steward-end-lease` are 0.2 to 12 ms (208 to 11,779 µs), well below 589 ms, but they
are not the quantity the page states. No edit.

### Boot-profile prompt: a finding on beamlet.md, not caused by this package

docs/userland/beamlet.md's last table row ("the shell's commands loaded when first called")
states the first prompt at 13.1 / 10.9 s on rv64 and 14.7 / 12.4 s on rv32, verified /
unverified. The prompt now comes about 2.5 s earlier at the base already:

| | rv64 verified | rv64 unverified | rv32 verified | rv32 unverified |
| --- | ---: | ---: | ---: | ---: |
| page | 13.1 s | 10.9 s | 14.7 s | 12.4 s |
| base 453a48bfc | 10.35 s | 8.49 s | 11.76 s | 9.90 s |
| branch f75f01d94 | 10.40 s | 8.54 s | 11.78 s | 9.89 s |

The figures are `first prompt drawn [t=…]`, seed 1, `icount`. Base and branch differ by at most
0.06 s, so the steward's weight does not move the boot. The drift came before this package; I
did not bisect it.

The page's table and its breakdown paragraph (the 1.9 / 5.8 / 11.8 / 14.7 s points) need
re-measuring by whoever owns beamlet.md. beamlet.md is outside WEIGHT1's owned paths, and the
change is not this package's, so I left it unedited and report it here.

The 20 s target ("the slowest measured prompt plus a tenth, rounded up to 5 s") is unchanged by
the new numbers: 11.8 × 1.1 = 13.0, which rounds up to 15 s. The target would only drop if
someone re-derived it, and the brief says no bound moves.

## Notes

- Reading: I read every changed hunk and its section in full. I did not read steward.md (about
  1,900 lines) or scheduling.md whole, per the launch-env context rules. All committed text is my
  own; no helpers.
- Design problems: none.
- Open risks: none from the change. The other manifest users wait for the train's bench.
- Next step: review by weight1-red.
