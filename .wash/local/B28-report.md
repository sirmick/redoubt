# B28 report: difftest erlang/files2 flakes about one run in three; erlang/ports always fails

Branch wp-B28, worktree /home/mcloonan/redoubt/.worktrees/B28, head 66496d25f, one commit on main
ac178530e (userland/otp/tests/erlang/files2.erl, userland/otp/tests/erlang/ports.erl). Tier B:
two difftest modules, no VM change.

## files2: the host, not the VM

The result tuple's element 2 is the access time. In the main checkout's cached artefacts the
polluted side was the ORACLE's (files2.expected: atime {{2026,10,8},{6,42,38}}, beamlet's got:
2020), so the flake is on BEAM as on beamlet, and a polluted cached oracle result then fails every
later beamlet run until the sources change. Measured on the host, no VM involved (python, ext4
mounted relatime): a file written and given atime == mtime by utime had its atime bumped to now
within 50 ms, 40 of 40 times, with no reader of mine; relatime bumps an access time at or before
the modification or change time on a read, and a watcher reads every new file here
(`wash-fswatch`, Wash's own; recorded as WASH-R24 in ~/wash/BUG-REPORT-redoubt-workspace.md).
A future access time, which relatime never bumps, stayed 40 of 40 and survived an explicit read.
Fix: the test sets atime {{2030,6,7},{8,9,10}} and mtime {{2020,1,2},{3,4,5}}, telling
change_time/3's two arguments apart, with a comment on relatime and the watcher.

## ports: the host's /bin layout

`{'EXCEPTION',error,enoent}` at the first spawn_executable: /bin/echo and /bin/cat are uutils
symlinks into ../lib/cargo/bin/coreutils; the sandbox mounts the host's /bin (itself /usr/bin)
and /usr read-only, and resolves the link relative to the sandbox's /bin, so the target is a /lib
it has no mount for. The sandbox's view is right (the target is not mounted); the test now names
only /bin/sh by path (dash, a link within /bin) and runs echo and cat through `sh -c`, so it holds
on any host with a POSIX sh; the expected output is unchanged. The sandbox's symlink resolution
across mounts is not changed here.

## Gates on 66496d25f (q, scratch under .tmp/B28)

- `tools/difftest erlang`: 43/43 passed, 1 skipped by design (ports passes; the oracle re-ran).
- files2 20 of 20, ports 5 of 5 (.tmp/B28/one.sh: the difftest's own beamlet invocation for one
  module, against its cached expected).
- `cargo test -p beamlet-vm`: 80 passed, 0 failed. `./test-shell`: every stage passed.
- docs PASS, formatting PASS. No VM change, so beamlet-files/-boot were not rerun.

## Documentation check

The difftest's README (userland/otp/README.md) and docs/userland/beamlet.md describe the
difftest, not these modules' contents: no change. The sandbox's symlink rule across mounts is
not on a page; if the owner wants /bin/echo to work on a uutils host, that is a sandbox change
for a follow-up, not this one.
