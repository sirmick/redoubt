# PIPE1 report (pipe-implementer-2), 2026-10-08

Branch wp-PIPE1, worktree /home/mcloonan/redoubt/.worktrees/PIPE1, head 7db73104a, base origin/main
3206c43b4. Clean tree, not pushed. Six commits:

1. 6fcca2112 client: a hub's tag stays its request's until its completion is taken
2. 2faeb60a9 beamlet: a launch can start a server, and sixteen jobs run at once
3. 1484b6794 piped: a session's pipes, each end a file one stage is given (WIP commit folded in;
   Size budget lines for servers/piped 387 and servers/init 2501)
4. 96e96018d image: a session holds 10 processes, for a pipeline's stages
5. d46684421 shell: pipelines of native stages, each stage's streams pipes of piped (now with
   piped's release)
6. 7db73104a shell: exec holds no console; a stage's output is drawn through the guard

## The pipe-never-reads race: root cause and fix (commit 1)

Not piped and not a race in Pipeline. It was a bug in the client hub (libs/client/src/aio.rs). The
hub freed a tag's slot when it *parsed* an answer, but the caller (beamlet's file table,
userland/otp/redoubt/src/files.rs) takes completions later from the hub's `done` queue and
matches them to its requests by (connection, tag). One completion buffer carries several answers.
When a server answers out of order (piped answers a parked call after a later one), a request the
caller sends on taking the first answer could get the tag of an answer still waiting in `done`.
The table then held two entries with one tag, and `swap_remove` reordering let the newer one take
the older one's answer.

The evidence: a request log in the VM (diagnostics since reverted) caught it. The feeder's open,
with `:append`, so create first, got Tcreate tag 0. The sink's open got tag 0 too, while the
create's answer was still in `done`. The sink then took the create's refusal: piped's PERMISSION
for a create below the root. That gave `eacces` before; it was `eio` once I renamed that error to
tell the sites apart. The stage `say` exiting 2 in some runs was a consequence: the owner let its
connections go when the sink crashed.

Fix: `free_tag` also counts as taken every tag whose completion waits in `done`. Test:
host:redoubt-client::a_tag_is_not_reused_before_its_completion_is_taken. It fails without the fix
(tag reused) and passes with it. The hub's consumers are beamlet (VM, tests) and aio-reader
(aio-many-reads cases); all were run.

Measured: before the fix, about 1 failure in 7 runs. After it, 12/12 runs on both widths, three
times: before the rebase, in the gate run, and on the final head.

## Ruling 2: piped is destroyed when its last pipeline ends (commit 5)

- `Redoubt.Pipes` monitors each process that opens it. `release/0`, or that process ending, lets
  go of it. With no holder left, piped's budget is destroyed before `release` returns.
  `Redoubt.Pipeline`'s owner releases in its `after`, so on every path, before `run/2` answers.
- Relaunch cost, from pipe-carries (QEMU TCG):
  - The session's first start: 187 ms rv32 / 226 ms rv64. This includes loading Pipes and
    reading /boot/piped.
  - A restart: 53 ms rv32 / 61 ms rv64.
  - Both are under the few-hundred-ms threshold, so the destroy-and-relaunch design is kept.
- Cases:
  - pipe-carries prints both times. It judges `{error,not_running}` after its last holder lets go.
  - pipe-never-reads and pipe-interrupted no longer pre-open piped. Their "carved again as before"
    verdicts now also show piped's budget is gone.
- Ruling 1 (768 pages): the arithmetic is stated on piped.md and native.md: lends 552 pages at
  worst for the two buckets admission needs, beside 32 pages of buffers, image and stack; 512
  would not hold that; measured use is 86 pages rv64 / 83 rv32.

## Rebase onto origin/main 3206c43b4: what main's changes needed

- d1a27c7af changed `serve_parking`'s callback type. piped.rs now uses
  `refuse_malformed(request).map(|()| None)`, as ipd does. The rv64 and rv32 builds and piped's
  host tests pass.
- HOME1 added `home_quota` and volume `bytes`. tests/data/pipe/manifest.json was regenerated from
  image/manifest.json, plus pipe-stage in public.
- init's size ceiling on main is 2500. The branch needs 2501, so the piped commit carries a Size
  budget line for it.
- Cases on the image's manifest:
  - image-disk and userland-read-only list their programs, so they gained piped (public[1]
    otherwise names nothing in the bundle).
  - init-boot now expects 2 public entries.
  - The predecessor's gate set had not run these.
- M2 progress: the conflict was resolved by keeping main's SSH-session sentence and the PIPE1
  sentence together.

## Gates on head 7db73104a (all exit 0)

- From prebuilt, `make -k -f scripts/jobs.mk set CASES=...`, both widths:
  - The first run was 101 pass and 10 fail. The fails were formatting, size-budget, image-disk,
    userland-read-only, init-boot and pipe-hostile-output; all are fixed above.
  - The rerun of the 18 failed or uncovered (case, width) runs: all pass. These were formatting,
    docs, size-budget, unsafe-budget, image-disk, userland-read-only, init-boot,
    pipe-hostile-output, userland-bad-start, verity-wrong-root and verity-flipped-tree.
- The cases in the set:
  - formatting, docs, size-budget, unsafe-budget
  - piped-build, piped-host-tests, beamlet-lookup-host, beamlet-lookup-cli-host
  - init-host-tests, steward-host-tests, rt-host-tests, client-host-tests, host-tests,
    r4-host-tests, sshd-host-tests
  - aio-many-reads, aio-many-reads-two, userland-boot, boot-profile, boot-profile-unverified
  - every pipe-* case
  - steward-boot, context-login, home-quota, login-refused, restart, restart-reboot, restart-ssh,
    session-ends, ssh-idle, ssh-resize, ssh-two-principals, sub-budget-flood, vault-launch,
    vault-session
  - consol-size, shell-commands, every sshd-loopback-* case
  - everything scripts/shell-cases picked: all beamlet-* cases, image-disk, userland-read-only,
    init-boot and others
- `./test-shell`: every stage passed, rc 0.
- `tools/difftest`: 527/527 passed, 21 skipped by design, rc 0.
- pipe-never-reads: 12/12 on both widths, on the final head.
- NOT RUN:
  - steward-model-host-tests: the model crate is untouched, and the predecessor's run stopped at
    30 min.
  - The whole bench: integration trains run it per train.

## Summaries checked

- README.md: M2 is still planned; PIPE1 is one step of it. No change.
- GETTING-STARTED.md: no launch or pipe claims. No change.
- image/README.md: the 10/40/20 processes and piped public are as built. No change.
- docs/plan/m2-usable-shell.md: Progress merged with main's sentence.
- docs/userland/native.md:
  - The hub section has the new tag bullet and test (13).
  - The streams and pipes section has piped's lifetime, the 768 arithmetic and the restart cost.
- docs/servers/piped.md:
  - The intro states the lifetime.
  - Serving pipes has the arithmetic and the restart cost.
  - Failure and restart: the session destroys piped itself.
- docs/userland/shell.md, sessions.md, sshd.md, beamlet.md and servers/README.md: no claim about
  piped's lifetime. No change.

## Open risks

- The hub change holds a tag until its completion is taken. A caller that never drains `done`
  would run out of tags, as it would run out of completions. Both consumers drain `done` on every
  dispatch.
- The binding at /dev/pipe stays, pointing at a dead connection, between pipelines. The next
  start's bind replaces it. A file operation there in between fails as a closed server would.
