# STEWARD2: rv32 console-session output stops after the first prompt

Symptom (wp-STEWARD2 on f820b6ba3, rv32 only): userland-boot, userland-read-only, boot-profile and
boot-profile-unverified time out. The console principal's session (the steward starts it on the
UART) prints its banner and first prompt; then nothing the VM writes reaches the UART, although
typed lines are read and evaluated (userland-boot's expected `verityd: block N does not match the
tree` for the flipped Version block appears, so NoSuch, Enum.sum and Version ran).

Main f820b6ba3: rv32 userland-boot passes (also at memory_mib = 1024). rv64 on the branch passes.
SSH sessions on rv32 (sshd's /dev/cons) print fine.

Probes (scratch worktree /home/mcloonan/redoubt/.worktrees/s2-main-probe, one jobs.mk case each):
- beamlet limits() reverted to size_of::<usize>(): still fails.
- erofsd BUDGET 1 MiB and erofsd:system buckets=4 (main's): still fails.
- the VM handed the steward's own erofsd:system badge instead of a minted connection: still fails.
- memory_mib 768: still fails.
- consoled heap_pages 32: still fails.
- the steward's say() silent after its carve lines: still fails.
- beamlet ConsoleIo instrumented: on any write outcome other than Wrote(n>0) or Busy, and on a
  submit error, record it and write it synchronously through `Console::write`, retried every
  10 ms up to 200 times until consoled takes it. Nothing printed: no write is refused, short or
  ended. A write is outstanding on the hub and its completion never comes back; read completions
  on the same connection do.

What differs from main: the session's /dev/cons is a consoled connection the steward mints
(`new_connection(lend, "", 0)` on its own /dev/cons connection, servers/steward/src/bin/steward.rs,
slot 4), and that one init minted for the steward. Main's VM gets init's minted console directly.

The instrumentation patch (userland/otp/redoubt/src/lib.rs):

    struct ConsoleIo { why: Option<String>, ... }
    // in take(), the write arm:
    ref o => { self.why = Some(format!("PROBE write outcome {o:?}\n")); self.stop_writing() }
    // in write():
    Err(e) => { self.why = Some(format!("PROBE write submit {e:?}\n")); self.stop_writing() }
    // at the top of console_write():
    if let Some(why) = self.cons.why.take() { for _ in 0..200 { if matches!(self.console.write(&mut self.lend, why.as_bytes()), Ok(n) if n > 0) { break; } let _ = redoubt_rt::handle::sleep(10_000); } }

## The mint chain (console principal's session on the UART)

1. `init` mints the steward's console: a `new_connection` at `consoled` through `init`'s own root
   badge, handed to the steward at `/dev/cons` in its startup namespace (as for every server).
2. The steward attaches that endpoint (`machine.consoled`, servers/steward/src/bin/steward.rs,
   after the carve) and, for the console session's slot 4, calls
   `cons.new_connection(&mut lend, "", 0)` on it: a connection minted through a minted one.
3. The session's VM gets that handle at `/dev/cons` in its namespace (`launch.namespace("/dev/cons",
   cons)`), opens it once (`Console::open`), and connects the hub to the same connection
   (`io.connect(console.file().connection())`): one read and one write out at a time.
An SSH session's /dev/cons is instead sshd's own 9P skeleton (a channel's console), which works on
rv32.

## The four cases, rv32, first failing line (wp-STEWARD2 on f820b6ba3)

- userland-boot: `timed out waiting for /UndefinedFunctionError\) function NoSuch.call/0 is undefined/`
  (the console shows the banner, `/ (1)>`, then only `verityd: block 849 does not match the tree`).
- userland-read-only: `timed out waiting for /^\[con [0-9a-f]{16}\] / \(1\)> "1\.2\.3"$/`.
- boot-profile and boot-profile-unverified: `timed out waiting for /beamlet: first console read \[t=...\]$/`
  (the banner appears; the boot-stats line, a console write after the prompt, never does).

## Reproduce (one command, from a worktree of the branch with the env of RESUME-q.md)

    make -f /home/mcloonan/redoubt/scripts/jobs.mk -C <worktree> prebuilt && \
    make -k -f /home/mcloonan/redoubt/scripts/jobs.mk -C <worktree> rv32/userland-boot

(450 s to its timeout; set the case's timeout_secs to 120 in a scratch tree to fail sooner. The
same tree's rv64/userland-boot passes in about 30 s.)

## Module reads vs console writes (for BEAM9)

The `verityd: block N does not match the tree` line is userland-boot's own expected attack line:
the case flips a bit of Elixir.Version's data after the pack (tests/userland-boot.toml), so it
says only that Version's lookup ran. It splits the theories: on the failing rv32 boot the
blocking module reads after the prompt do run and return (NoSuch's lookup, then Version's, which
reaches the flipped block, in the typed order), while nothing the VM writes after its prompt
appears: not the NoSuch error, not `55`, not beamlet's own `Elixir.Version not loaded: ... corrupt`
line. So erofsd/verityd are not stalled there; the console's writes are. On the boot-profile
cases the line that never comes is `beamlet: first console read [t=..]`, also a console write
after the prompt; whether a module read stalls there was not separated.
