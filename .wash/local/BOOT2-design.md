# BOOT2 design checkpoint (2026-10-08)

Branch wp-BOOT2 on main 3b049cacc, worktree /home/mcloonan/redoubt/.worktrees/BOOT2. Scratch
/home/mcloonan/redoubt/.tmp/BOOT2/ (profile-rv64/rv32.console.log: boot-profile's stamped lines).

## Measured today (boot-profile, verified volume, icount shift=3, seed 1)

| point | rv64 | rv32 |
| --- | --- | --- |
| init has started its servers (sshd) | 0.57 s | 0.71 s |
| steward started (init pushed beamlet's 4 MB public entry to bootfsd in between) | 3.15 s | 3.96 s |
| the session VM has read its boot pack (1.88 MB, 94 entries) | 7.66 s | 9.13 s |
| first console read (SHELL2's driver, before its banner) | 13.68 s | 15.68 s |

So: the push costs 2.6 s (rv64) / 3.3 s (rv32); steward start -> pack read is 4.5 s / 5.2 s, which
is the carve, the launch's stream of the image from bootfsd and the VM's pack read (the page's 3.6 s
verified for a 2.46 MB pack; smaller now), so the stream is ~0.9-1.7 s; the shell's own start
6.0 s / 6.5 s is not this package's. The prompt is already under the page's 15.4 / 17.6 s (the
pack shrank since), and 1.3 s over main's pre-steward 12.4 s on rv64.

## Where the time goes: per-call cost, not bandwidth

- init pushes the entry with `add` calls of CHUNK = one page (init.rs), through its 2-page lend
  (bound.rs LEND_PAGES = 2, counted in INIT5's bound): 1024 calls in 2.6 s = 2.5 ms a call.
- The steward streams the image through its 2-page lend (steward.rs LEND_PAGES = 2, "a page at a
  time"): a 9P read is at most the lend's iounit (~8 KiB), so ~512 reads per session launch, in
  launch batches of 64 pages (PLACE_PAGES): ~2.5 ms a read again.
- A call with a 2-page lend costs ~2.5 ms of guest time whatever it carries (kernel lend
  bookkeeping, 9P/wire framing, the server's copy): the fix is fewer calls, not fewer bytes.

## Options

A. **Bigger lends (recommended first step, size S).** init's push in 16-page chunks through a
   16-page lend (MAX_LEND_PAGES; init's bound grows by 14 pages, within its 550-of-1024 margin);
   the steward's lend 16 pages, so a 64-page batch is 4 reads. 64 + 64 calls: ~0.3 s in all,
   saving ~3 s on rv64 and ~4 s on rv32. No protocol, authority or budget change; bootfsd keeps
   its role (the image's one copy, served to the steward and to sessions' launchers over 9P,
   R46 untouched); K23's restarted steward streams as before; nothing K26 touches.
B. **The steward caches the image once per life** (on top of A): one 1024-page buffer read from
   bootfsd at its start (64 reads, 0.15 s), every session launched from memory (Launch::new, ~10
   ms). Per-login cost falls from ~0.15 s to ~0.01 s; the steward's budget grows by 1024 pages
   (image/manifest.json 1024 -> ~2100, and boot-profile's 1 GiB stays); K23's restart re-reads
   once. Worth it only if logins must be that fast; not needed for the prompt target.
C. **init hands the steward what it already read** (the plan's other wording): ruled out by
   budgets: init's bound is 1024 pages (INIT5), so it cannot hold a 4 MB copy to transfer, and
   transferring the bundle's own pages loses them for K23's restart (the dead steward's pages are
   freed, their content gone). The reverse (the steward lends 16 pages to init to copy into) needs
   an init-served call the steward has no endpoint for: a new capability, the Architect's.

## Targets and pages

With A the verified prompt should be ~11 s rv64 / ~12 s rv32, under main's pre-steward 12.4 s,
with the three spans re-measured and written on beamlet.md "beamlet on Redoubt" (the table row,
the breakdown paragraph) and steward.md's sessions section; the boot-time target rule stays
("the slowest measured prompt plus a tenth, rounded up to 5 s": 15 s, as the node says).

## Found on the way: boot-profile is broken on main since SHELL2

The case expects `Redoubt shell, on Elixir` before `beamlet: first console read`; SHELL2's
driver reads the console before it draws the banner (lines 58 then 60 of the log), so the
ordered list stalls and both widths time out at 900 s on main 3b049cacc, with every line present.
The stamp's meaning holds (the first read is the driver's, the prompt follows within ms, and the
case's input queued at that point is read by the line editor: 55 prints). Fix: swap the two
expects (and -unverified's). It is a prerequisite for measuring BOOT2; I propose to carry it in
this package's first commit, or as a B-node if you prefer.

## What K23 and K26 mean here

K23 (merged): a restarted steward redoes its start (stat of /boot/beamlet, the carve, the
console session's launch) — unchanged by A; B would re-read the image once. K26 (wp-K26, in
flight): changes the steward's receive loop (held `watch` calls, the probe's timing); A touches
only LEND_PAGES and the push's chunking, outside that loop; a rebase over K26 is trivial.
