# B22 report

Branch wp-B22, head c42121313 (one commit on f820b6ba3):
`testbench: Redoubt's loopback server writes its standard error to its log, not ssh's`.

## Cause

bench-ssh-loopback-host-key runs against Redoubt's server, `redoubt-sshd-host`, not the
OpenSSH guest. ssh starts it as its ProxyCommand and gives it ssh's own standard error. ssh
refuses the pinned key, prints "Host key verification failed." and hangs up. The server's next
write then fails with a broken pipe, and `main` returns the error, which prints
"Error: Broken pipe (os error 32)" on the shared stream after ssh's line. It belongs to the same
family as B12's SessionEOF. The OpenSSH guest's proxy already ends in `2>/dev/null`.

## Fix: the streams are told apart at the source

`ssh::redoubt` (tools/testbench/src/ssh.rs) now ends the proxy line with `2>>{case}/sshd.log`.
The server's standard error goes to its own log (O_APPEND, the same file it writes with
`--log`), and ssh's standard error carries ssh's lines alone. The runner's failure text was
already "ssh exited (status N), expected M; last output: <line>", and its last line is now
ssh's. must_fail and the case file are unchanged.

Test: `host:testbench::the_redoubt_proxy_keeps_its_errors_off_ssh` builds the proxy with
`ssh::redoubt` around a stand-in server that prints the broken-pipe line on stderr. It runs the
line with `sh -c`, as ssh does, and asserts an empty stderr and the line in the server's log.
Without the redirect the line reaches stderr and the first assert fails. That is argued, not run
against the old code.

## Gates

- make prebuilt: rc=0
- rv64/bench-ssh-loopback-host-key: rc=0 (run first)
- The other 13 ssh-loopback cases on rv64, all rc=0: sshd-loopback-env-refused, -interrupt,
  -window-change, -window-change-zero, -r67, -logins, -independent; bench-ssh-loopback,
  -aborted-text, -exit, -forbid, -openssh, -deadlock (quiet)
- The rv32 targets do not exist for these cases: jobs.mk says "no target; rv64/<case> runs it".
  The cases are host-only, with no guest of either width, so one run per case covers both.
- docs: rc=0 (also after the final reflow)
- formatting: the first run failed (rc=1, rustfmt rewrapped one line of the new test). After
  `cargo +nightly fmt -p testbench`, `q run cargo testbench --arch rv64 --exact formatting` gave
  rc=0.
- `q run --cores 4 -- cargo test -p testbench`: rc=0, 147 passed, 0 failed (on the final code)
- The loopback cases ran before the rustfmt rewrap. That change touched only the test's
  whitespace.

## Docs checked

- docs/testbench.md, "Sessions and the loopback server": the claim "its log goes to a file ...
  never to ssh's output" now covers the server's standard error too, and names the broken pipe.
- docs/testbench.md, "Against Redoubt's sshd": the new host test is in the status list, and
  tested goes from 11 to 12.
- docs/servers/sshd.md, Transport: the old claim, "nothing of the server's follows on the
  standard error it shares with ssh", was false (a broken pipe still could). It now says the
  bench sends the server's stderr to its log.
- tests/bench-ssh-loopback-host-key.toml, tests/bench-ssh-loopback-exit.toml: must_fail
  unchanged and passing.

## Risks

A server that dies before it opens its log (bad arguments) now prints only to that log. ssh
then reports just a closed connection, and the reason is in sshd.log beside the transcripts.

Red review note: its rerun saw no broken pipe in sshd.log, so the pipe is a race (the trains saw it on both widths), not every run; the fix covers it either way. docs/testbench.md now says the stderr goes to the log for Redoubt's sshd and is discarded for QEMU's (amended; docs rc=0).
