# INIT2 handoff 3 (init2-implementer-3 → next)

## State

Branch `wp-init2`, worktree `/home/mcloonan/redoubt/.worktrees/init2`, on `d974e247c` (wp-init1's
tip; do NOT rebase onto main until the orchestrator says). Tree clean. d2 reported complete
(detail: `.wash/local/INIT2-d2.md` in the worktree). Commits after the base:

- `e4cec8e57` docs: cherry-pick of main's 1d109ac5f (cores dropped). Drops out at the rebase.
- `0a5c6cbdb` init: the boot manifest is read and checked whole (d1 + rulings Q1-Q5, + (B), (C),
  the step-2 page line, `servers/init/fuzz` added to tests/formatting.toml roots).
- `4f669c9a7` testbench: a budget's reason line matches its name whole.
- `420cf95ac` rt: Heap::fix (fixed arena).
- `8cce2e862` signing: DEV_PUBLIC_KEY in libs/signing.
- `15b151ba9` init: fuzz corpus kept and replayed.
- `cc4e712c5` init: bootfsd's args carry the public list.
- `2e946f2e2` kernel: system_reset kind 3 (ruling A).
- `924d96691` rt: first_entry! / start_first, the bundle's view (ruling D).
- `1fb7dd477` rt: Registers::unmap (ruling E).
- `08bda1ac0` testbench: `[disk] partitions = N` writes a GPT with blkd's image builder
  (tools/testbench now depends on redoubt-blkd).
- `4e5fd20a5` WIP init: the whole boot. HOLDS: the binary (servers/init/src/bin/init.rs, the
  launch sequence below), the boot's lib additions (Refusal::KeyHeld / Failed / Watchers,
  Why::NoKeyd, check `init_calls` (keyd required; keyd/consoled/bootfsd must receive on
  something), the MAX_THREADS - 1 watcher limit, Plan.keys carry their manifest path, bound
  INIT_ENDPOINTS + LEND_PAGES), tests/init-boot.toml, tests/init-refuses-public-manifest.toml +
  tests/data/init/public-manifest.json, init.md lines (step 1 MAX_THREADS clause, step 4 "Without
  a `consoled` entry, `init` keeps the UART"), devices.md status line for kind 3, testbench.md
  `[disk] partitions` + bench:init-boot, size row servers/init 1547, init unsafe budget 0. Its
  MESSAGE IS STALE (still says "reads and checks", and an "Unsafe budget: redoubt-init ... one raw
  slice" line that is no longer true): rewrite and split before acceptance (lib checks into d1
  or their own commit; the boot + cases as "init: the boot starts the servers from the manifest").

## Rulings: all applied and committed

- (A) kind 3: committed 2e946f2e2; devices.md status line naming the test is in the WIP.
- (B) watcher thread per server in the bound (IPC page pinned to objects.md, WATCH_STACK_PAGES 4,
  tables): in d1. Follow-up ruling (watchers ≤ MAX_THREADS - 1, check refuses): in the WIP.
- (C) Plan::init_badges, smallest from 1 unused at the server's first receives: in d1.
  Follow-up ruling (no keyd refused; keyd/consoled/bootfsd with no receives refused,
  Why::Unknown at servers[i].receives; no consoled allowed): in the WIP.
- (D) committed 924d96691. NOTE rt budget went 13 -> 14 (ruling said 11 -> 12; Heap::fix had
  already raised it to 13). Reported.
- (E) committed 1fb7dd477 (fake does not track device mappings; the test checks the call and
  the kernel's refusal handed back; compile_fail doctest for use-after-unmap).
- Page lines: step 2 replaced verbatim (d1); bullet order already right. Nothing owed.

## Launch sequence: state (all runs on QEMU, rv64 and rv32, bench case init-boot PASS)

1. root usage, fix_heap, map UART, `init: up`, Lend (LEND_PAGES), bundle, device_info, read,
   check -> plan; refusal = line + system_reset kind 3 (init-refuses-public-manifest PASS, 255).
2. every receive endpoint created (init keeps the receive right); init's own handles minted at
   keyd/consoled/bootfsd from Plan::init_badges; one reports endpoint (handle in static REPORTS).
3. keyd started (no /dev/cons), then `holds` for every plan key via typed::call keyd::Protocol;
   yes -> Refusal::KeyHeld{at}. Prints `init: keyd holds none of the N keys`.
4. UART: Registers::unmap, output Nowhere, consoled started, Connection::attach on init's own
   handle, open "" OWRITE; init writes through it (`init: started consoled, and writes through
   it`).
5. the rest in manifest order; each child gets `new_connection` at consoled as /dev/cons (id
   printed raw: `init: started X, console <u64>`), its receives, minted handed badges, devices
   from plan.placements, args via check::args. Then each bootfsd: `add` per page chunk, `seal`.
   `init: the boot is done: N servers`.
6. exits: each server's own exit endpoint, a watching thread (arg = index<<16 | handle) that
   `send`s [index, pid, cause, code] to REPORTS; main thread prints `init: NAME (PID p)
   exited|faulted|was killed, code c` and disconnects the child's console. No restart (INIT3).
   Any failing step: Refusal::Failed{at, step}, kind 3.
Not done: steward/sshd (step 6 of the page) are not in the image manifest; public push is
exercised only with an empty list in a boot case (host-tested in bootfsd); fsd/volumes not
started specially (no volume handling in the launcher).

## d3, d4, d5, d6

- d3 (consoled `[con N]` prefixes, announce line, the connection-id format of ruling 6):
  UNTOUCHED. init currently prints the raw 64-bit connection id; d3 decides the format.
- d4: two cases exist (init-boot, init-refuses-public-manifest). Owed: the servers' cases under
  init (a test program as a servers entry reading /boot, reporter by [con N]), every refusal
  case in the brief's list (R33, R34 x5, R35 x2, devices x4, system fit, buckets, public
  manifest (done), INIT_PAGES bound), the forgery case, root usage after boot ≤ bound, the
  todo/server-bucket-counts page and Sizing's "init does not exist yet".
- d5, d6: untouched.

## Gates last run

Whole bench before the last WIP fold: 293 PASS, 1 FAIL bench-ssh-loopback-openssh (podman not
installed; host env). After: init-boot, init-refuses-public-manifest (both widths),
init-host-tests, init-build rv64/rv32, size-budget, unsafe-budget, docs, formatting PASS.

## Traps

- Run every cargo/bench/fmt as `/home/mcloonan/redoubt/.wash/local/in-dev <cmd>` from the worktree.
- Never git stash / --autostash. Fold: `git commit --fixup=<sha>`, then
  `GIT_SEQUENCE_EDITOR=: git rebase -q -i --autosquash d974e247c`. To move commits, a python
  sequence editor (I used one moving the WIP last: /tmp/seqed.py is gone after reboot; it just
  reorders todo lines).
- Budget trailers need a blank line before them; `$(...)` in printf eats it.
- size-budget counts code lines (not comments/blank); the init row is raised in the WIP's
  message line "Size budget: servers/init: ...".
- unsafe-budget renames: a commit renaming a budget needs an `Unsafe budget: <old name>: ...` line.
- K16 owns kernel/libs/sys etc.; only ruling (A) touched them.
- Don't edit sources while a background whole bench runs (it builds per case).
- System `processes` limit is 15 today, so the MAX_THREADS refusal is only reachable with a
  roomier system (the host test sets processes_limit).

## What consumed my context (avoid it)

- Reading large files in full: libs/sys/src/call.rs (335 l.) and kernel/src/redoubt.rs (266 l.)
  to commit a 3-line change; servers/init/src/check.rs (550 l.) whole; libs/rt/src/start.rs and
  handle.rs large spans; libs/client/src/launch.rs and grants.rs whole. Use grep -n + targeted
  ranges; check.rs is the main one, read only the function you change.
- The QA thread files (`.wash/qa/*.md`) embed a big base64 checkpoint at the end: `cat` them only
  up to the "wash-qa-checkpoint" comment (e.g. `sed '/wash-qa-checkpoint/q'`).
- `cargo testbench --list` prints long descriptions: use `| awk '{print $1}'`.
- Boot logs (target/testbench/*.log) start with kernel argument hex dumps: `grep -v` or tail.
- Writing the 450-line init.rs twice (a stale-read error forced a second Write): Read the file
  just before writing it.
- Rejected 2000-byte reports: keep under ~1900 bytes.
