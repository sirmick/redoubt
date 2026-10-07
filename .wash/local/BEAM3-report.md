# BEAM3 report: asynchronous underneath, and files over 9P

Branch wp-BEAM3, base main 8f51851d9 (BEAM8's merge), head 489c740b7 after the panel's folds, tree clean.

## Summary (for the result)

BEAM3 done on wp-BEAM3 @ a8fb9d349 (base 8f51851d9). Six commits: console on the hub (+ footprint
raise), interp.rs native-body retry (one hunk), Rerror names by one table (wire/rt/client),
file ops park only their asker, files over 9P (+ namespace-root ruling A), the waiter's bounded
hand-over (fix B). Short gate all green: host beamlet-vm/beamlet-redoubt/redoubt-client/
redoubt-init exit 0; docs, size-budget, unsafe, no-cruft PASS; fmt clean; 12 build cases x2
PASS; beamlet-files, -console, -boot, -footprint, aio-many-reads, userland-read-only,
userland-boot, userland-bad-start, boot-profile, init-boot, bench-net-peer, ipc-outcomes: 24/24
PASS. Shell start: the VM thread is out of its endpoint 7.93 s (guest time) from platform start
to prompt; lookups only 106 ms of it. Rulings implemented: waiter-session B, namespace-root A,
footprint-headroom A. Found and fixed two regressions of mine (prompt `?`, reason `other` →
`corrupt`). Detail below.

## Commits (oldest first)

1. 7e9fa7f59 beamlet: the console on the hub, a waiter per connection, no reader thread.
   Carries the footprint raise as ruled (BEAM3-footprint-headroom A): image/manifest.json and
   tests/data/boot-profile/manifest-unverified.json: consoled stack 5→6, heap 18→22; beamlet
   stack 17→18 and heap cap 10,862→10,861 (cap + stack must stay under the 10,880 budget; still
   twice the 5,429-page peak + 3); docs/testbench.md rows and prose; servers/init/tests/manifest.rs
   the image's bound 524→525 (beamlet's launch stack page). Arithmetic in the message.
2. 9f623290d beamlet: a native body honours its native's retry (interp.rs NATIVE_BODY, one hunk;
   applied cleanly beside BEAM8's `u(ins, 0)` accessor, no edit).
3. cc46b988f wire, rt, client: an Rerror keeps its name by one table. Now also: beamlet's lookup
   reason names `corrupt` (userland.rs `unread` row + host test), and userland-boot /
   userland-bad-start expect `...could not be read: corrupt` (was `other`: the one table now
   names erofsd's `corrupt`, which the old client lumped into Other).
4. 387bc3562 beamlet: a file operation the platform finishes later parks only the process that asked.
5. 9e70aac98 beamlet: files over 9P on Redoubt, through the hub. Folded in: beamlet-files on
   PACK1's shape (erofsd:system, endpoint=), a rustfmt fix of bin/beamlet.rs's usage line, and
   ruling BEAM3-namespace-root A: a path above a binding is a directory the namespace answers
   (info = directory, no other fields; list_dir = next names of the bindings below; open, mkdir,
   rm, rename there = eacces; neither inside nor above = enoent). Host test
   a_path_above_a_binding_is_a_directory_the_namespace_answers; sessions.md one line; files.md
   prose + status (17).
6. a8fb9d349 client, beamlet: a caller busy past the session bound keeps its sessions (fix B).
   libs/client/src/aio.rs waiter(): a wake-up not taken within HAND_OVER_US (COLLECT_WAIT/4 =
   2.5 s) is held, compacted into the answers' own pages, while the waiter calls again with a
   hold of 0 (keeps the session, takes what is ready); at most MAX_HELD = 4 held, then it waits
   without bound (stated in aio.rs and native.md). Tests: redoubt-client
   a_caller_busy_past_the_session_bound_keeps_its_session (12 s); beamlet-redoubt
   a_vm_busy_past_the_session_bound_keeps_its_console (12 s; FAILS on the unfixed waiter with the
   console ended, checked). Size budget: libs/client 1000→1026 with its line. Hub::wait (the
   B20 margin test's path) is untouched, so B20 is not fixed here.

## Rebase resolutions

- Onto fdafcf2cb: clean.
- Onto 8f51851d9 (BEAM8): only commit 1 conflicted: the two manifests (kept BEAM8's budget
  10,880 and args; my stack 18; cap then 10,861 as above) and docs/testbench.md (BEAM8's prose and
  beamlet heap figures; my stack figures and the consoled sentence). vm.rs auto-merged.
- Earlier (predecessor, onto 992d447ce): redoubt/src/lib.rs, bin/beamlet.rs, tests/limits.rs,
  docs/userland/beamlet.md, docs/plan/m1-separation.md (listed in the progress note).

## Gates (final head, exact commands and exits)

- `q run --cores 4 -- cargo test -p beamlet-vm -p beamlet-redoubt --features beamlet-redoubt/fake`
  (userland/otp): exit 0 (files 13, console 10, vm 37, io_wait 4, ...). Run on the tree before the
  cap/bound edits, which touch no beamlet code.
- `q run --quiet -- cargo test -p redoubt-client`: exit 0 (aio 12 incl. the busy caller).
- `q run --cores 4 -- cargo test -p redoubt-init`: exit 0 (manifest 53).
- jobs.mk `docs rv64/size-budget rv64/no-cruft rv64/unsafe-budget` on a8fb9d349: all PASS.
- `cargo +nightly fmt --check`: redoubt-client, -init, -wire, -rt and the userland/otp workspace: clean.
- jobs.mk `prebuilt` then both widths: all 12 *-build cases PASS (the runtime-change sweep;
  client's API change is two pub consts), and beamlet-footprint, userland-read-only,
  userland-boot, userland-bad-start, boot-profile, init-boot, beamlet-boot, beamlet-console,
  beamlet-files, aio-many-reads, ipc-outcomes, bench-net-peer: 24/24 PASS. These ran on the tree
  one commit-edit before the head: the only later change is servers/init/tests/manifest.rs (a
  host test), which no image contains.
- Not run: aio-many-reads-two (pre-existing failure on other worktrees, not in the gate list);
  the whole bench (the train's).

## Measurements

- Longest synchronous wait at the shell's start (rv64 boot-profile, icount guest time, verified
  volume; uncommitted probe, reverted): the VM thread never enters its endpoint from the
  platform's start to the first console read: 7.93 s. Lookups: 95, all from the pack, 106 ms in
  total, the longest 23.5 ms. So the stretch is the interpreter's own start work, under the 10 s
  session bound in guest time but not by much; fix B covers it (at most a write answer and a read
  answer are held then, one write being out at a time).
- Footprint (beamlet-footprint, vs main's on the same base): beamlet heap at the prompt 5,240 →
  5,242 pages rv64, 5,067 → 5,068 rv32; beamlet main stack peak 33,464 → 35,288 B rv64, 27,512 →
  28,520 rv32; consoled stack 9,144 → 10,376 B rv64, heap 8 → 10 pages rv64, 9 → 11 rv32. The
  waiter's own 8-page stack and its 16-page completion buffer per connection are held from the
  start; held hand-overs exist only while the VM is away (compacted to the answers' pages).
- consoled's heap (for a possible follow-up B): the peak rose 2 pages, not 5 (cap 18 → 22 is
  twice-peak rounding). What a multiplexed session holds per outstanding request is its record
  (at most 256 B, REQUEST_STATE) and, for a write, the one page the write was sent in until it is
  answered, plus the session's tag bitmaps; beamlet keeps one read parked and one write out, so
  about a page or two of peak is that, unmeasured beyond the case's peaks.

## Documentation check (summaries)

- docs/userland/beamlet.md: async section (status 9 tests, the busy-scheduler sentence), files
  row — updated. docs/userland/native.md "Many requests at once": busy-caller bullet, status 12 —
  updated. docs/userland/files.md "Files over 9P": status 17, the ancestor rule — updated.
  docs/userland/sessions.md: one line (ancestor directory) — updated. docs/testbench.md memory
  table and prose — updated. docs/servers/wire.md, native.md error names — updated in commit 3.
- Checked, no change needed: README.md and GETTING-STARTED.md (no claims on beamlet files or the
  console path); docs/plan/m1-separation.md (its hub/files sentence from the earlier rebase stands);
  userland/otp/README.md (updated in commit 5, still true); docs/kernel/budgets.md (states no
  beamlet or consoled numbers).

## The surface BEAM4 and BEAM5 build on

BEAM4 (launching, the natives' calls and serving) and BEAM5 (`/net`, `gen_tcp`) get: the VM's
`Files` trait as it stands (userland/otp/vm/src/platform.rs: open/close/read/write/pread/pwrite/
seek/truncate/sync/handle_info/info/list_dir/make_dir/delete/del_dir/rename/read_file, the
link/permission/time defaults refused, and the asynchronous trio `asker`/`finished`/`abandon`
with `FileError::Later`: a native that returns `Later` is parked by `Ctx::await_io` and called
again when the platform names its asker finished; a dead asker's operation is abandoned with what
it holds); beamlet-redoubt's `Io` (userland/otp/redoubt/src/io.rs: one hub per VM, `connect` gives
a connection its waiter once, at most MAX_WAITERS = 6, `request()` submits from the scheduler's
thread, `completed()` and the platform's `dispatch` hand each `Done` to its owner by (conn, tag),
which is where a `/net` data file's or a launched job's completions plug in beside the console's
and files'); `Console::file` (libs/client/src/console.rs: the open console's fid, so a
console-like file goes through the hub, not a blocking call); and the waiter's guarantee
(libs/client/src/aio.rs): every answer reaches the VM's endpoint in order, once, with its buffer;
a VM away from its endpoint for longer than a server's session bound keeps every session, the
waiter holding up to MAX_HELD = 4 untaken wake-ups (compacted to the answers' pages) and calling
with a hold of 0 meanwhile; past 4 it waits without bound and the server may end the session,
and every request then comes back `Ended` with its buffer. What it does not give: a typed call
(no hub carries one: `rename`, the console's size) blocks the scheduler; one thread per
connection remains the cost of waiting on several endpoints.

## Departures and residuals

- Twstat is refused: write_stat applies nothing, truncate at a position is enotsup; littlefsd
  stores no mtime (0). Rename and the console's size are typed calls on the VM thread.
- Outside the original owned paths, as ruled or raised at checkpoints: libs/client aio.rs (fix B),
  console.rs Console::file, file.rs helpers; libs/wire/rt (error table); image/manifest.json,
  its boot-profile copy, docs/testbench.md and servers/init/tests/manifest.rs (footprint ruling);
  tests/userland-boot.toml and userland-bad-start.toml (the `corrupt` reason).
- An open for reading of a namespace ancestor is also eacces (the ruling named open-for-write);
  reading a directory as a file has no meaning there.
- At MAX_HELD (4) unhanded wake-ups the waiter waits without bound, and the server may end the
  session at its bound: stated on native.md.

## Open risks

- The start stretch (7.93 s guest) will grow with the shell; fix B makes it harmless for the
  sessions, but the console's echo still waits for the VM.
- aio-many-reads-two fails before and after this branch (seen on train 3's worktree too).

## Panel folds (head 489c740b7; commits now 15bb1ebec, 7ff193504, 56e623aec, ace33b08b, caa94b5f8, 489c740b7)

- Red P1: ConsoleIo::take hands back any completion on the console's connection that is not its
  read's or write's, so a file operation on /dev/cons is answered (host test
  a_file_operation_on_the_consoles_connection_is_answered; without the fix it hangs: checked).
  Red P2: an abandoned operation stops at its next answer (an_abandoned_operation_stops_at_its_next_answer);
  a waiter that cannot start is recorded and its connection refused for good (io.rs); the
  testbench row's beamlet stack is 35,288 B, from beamlet-footprint/userland-* rv64 on BEAM8's VM;
  below() cleans its path (/home/. lists alice). (5) not mine (table stands).
- Simplifier 1-7: posix is beamlet-redoubt's exhaustive match on ErrorName (out of the VM); the row
  test reads docs/servers/wire.md's table and holds every text -> name -> atom to the code; Busy is
  in the one table and aio reads it so; ErrorName::text gone; Table::take -> (); one plain()
  FileInfo; Kind::HandleInfo merged; one fixture::session_with. #8 declined.
- Editor: README.md and GETTING-STARTED.md say file operations run on the machine; budgets.md sums
  21,264 / 10,346 / 10,089 and the bound 524 (init-boot prints 524 on both widths; the page's 508
  was stale); files.md links wire.md's column, mtime sentence, Follow-up line gone; beamlet.md
  wording; files.md status 19.
- Evidence on 1eedb90c9 (the last full run): host beamlet/init/wire/client/rt exit 0; fmt clean;
  24 build cases and 24 cases PASS both widths. Then on 489c740b7: beamlet-redoubt exit 0,
  redoubt-client exit 0, docs/size-budget/unsafe/no-cruft PASS, prebuilt 0, beamlet-files and
  beamlet-console rv64 PASS, userland-boot rv64 and rv32 PASS.
