# B73: the bench's QMP socket in a private 0700 directory

Head: wp-B73 18969e24b (one commit on main 985ec0047). Worktree clean. Not pushed.

Gates (all through scripts/q, worktree /home/mcloonan/redoubt/.worktrees/B73):
- `cargo test -p testbench`: 199 passed, 0 failed (rc 0)
- `cargo run -q -p redoubt-doccheck`: rc 0
- `make -f scripts/jobs.mk prebuilt`: rv64 299 cases 0 failed, rv32 285 cases 0 failed
- `make -j4 -f scripts/jobs.mk set CASES="consrelay-footprint init-boot beamlet-footprint
  steward-ssh-two-principals steward-session-ends userland-read-only userland-boot"`
  (every `memory = true` case): 14/14 PASS on rv64 and rv32, rc 0; log
  `.worktrees/B73/.tmp/b73-gate.log`; no `redoubt-qmp-*` left under `.tmp` afterwards
- `rustfmt +nightly --check` on qemu.rs and `git diff --check`: clean

## Delivered

`tools/testbench/src/qemu.rs`
- `Removed` (unlink one file on drop) and `qmp_socket()` (`<tmp>/redoubt-qmp-<pid>-<n>.sock`)
  are replaced by `QmpDir`: a directory `<tmp>/redoubt-qmp-<16 hex from /dev/urandom>` made
  with `DirBuilder::new().mode(0o700)` (exactly what `mkdtemp` does: `mkdir(S_IRWXU)`,
  retried on `AlreadyExists`), no `unsafe`, the socket at `<dir>/qmp.sock`. `<tmp>` is
  `std::env::temp_dir()`, which q sets to `$REDOUBT_TMP` through `TMPDIR` (the bench's
  existing convention for every scratch path).
- `QmpDir::under` refuses, before QEMU starts, a socket path of 108 bytes or more
  (`sun_path` with its NUL), with a message naming the length, the limit and "set
  REDOUBT_TMP nearer the root"; the directory it made is removed by the guard on that path.
- `run`: `let qmp = if boot.memory { Some(QmpDir::new()?) } else { None }`, declared before
  the guest so it drops after QEMU is reaped; `Drop` is `remove_dir_all`. Covers a pass, a
  fail verdict and an `?` error (unwinding too). A SIGKILL of the bench cannot run a
  destructor: the directory stays, empty of any listener (QEMU dies with the bench by
  PDEATHSIG), 0700, random, so it never collides with a later run and exposes nothing.
  Stated in the page and the doc comment. Doing better (connect and unlink at spawn) would
  change `measure_stacks` and the timing cases; out of XS scope.
- Host test `a_qmp_socket_is_private_and_binds` replaces `a_qmp_socket_path_binds`: two
  directories differ, mode & 0o777 == 0o700, socket path < 108, directory gone after drop;
  then under a deliberately deep temp dir (`.../testbench-qmp-deep-<pid>/xxx…(108)`) the
  constructor errs with "must be under 108" and leaves that directory empty.
- `a_killed_bench_leaves_no_qemu` uses `QmpDir` for the stand-in's socket instead of its own
  predictable `temp_dir()` path.

Docs
- `docs/testbench.md` "The memory budget": the umask note replaced by what the code does
  (private directory, random name, removed on any end short of a kill, the kill residual,
  the 108-byte refusal); status line gains `host:testbench::a_qmp_socket_is_private_and_binds`.
- `docs/todo/qmp-socket-private-dir.md` deleted; its line removed from `docs/SUMMARY.md`.

## Summaries checked (SWARM "pages move with the code")
- `README.md`, `GETTING-STARTED.md`, `CONTRIBUTING.md`, `tools/testbench/README.md`,
  `docs/todo/README.md`, all of `docs/`: grep for `qmp`/`umask`/the todo page name; the only
  mentions were the rewritten passage and the SUMMARY line. No milestone progress page
  mentions the follow-up. No other change needed.

## Not run / risks
- No whole bench (XS, Tier A bench code; reviewer kernel-red-2 runs the bench gate).
- The pre-existing warning `field harts is never read` (idle.rs) is untouched and not mine.
- Reviewer may question "mkdtemp" vs `DirBuilder.mode(0o700)`: same syscall and mode; chosen
  to avoid an `unsafe` libc call in the bench.
