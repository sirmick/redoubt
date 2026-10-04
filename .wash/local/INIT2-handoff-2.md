# INIT2 handoff 2 (init2-implementer-2 → next)

## State
Branch `wp-init2`, worktree `/home/mcloonan/redoubt/.worktrees/init2`, on `d974e247c` (wp-init1's tip;
INIT1 not yet merged; do NOT rebase onto main until the orchestrator says). Clean tree. Commits:
- `e4cec8e57` docs: cherry-pick of main's 1d109ac5f (cores dropped). Drops out at the rebase.
- `d74416f34` init: the boot manifest is read and checked whole (d1 + rulings Q1/Q2/Q4/Q5 + the
  ruled page lines in init.md and budgets.md + init unsafe budget + size row).
- `8bb7669c6` testbench: reason line matches a budget name whole (names with ": ").
- `bdeed50d3` rt: Heap::fix / redoubt_rt::fix_heap (fixed arena; rt unsafe 11→13).
- `5afb77b37` signing: DEV_PUBLIC_KEY lives in libs/signing; verify.rs uses it by name.
- `c352d908a` init: fuzz corpus (384 set-cover inputs + 4 hand seeds; 3601 s, 169M runs, 0 crashes).
- `57d17d702` init: bootfsd's args = its own (buckets= only) + `public` (check::args, ruling (a)).
- `e80486db7` WIP init: the program reads and checks the manifest on the machine (fold later).
Checkpoint reported (detail `.wash/local/INIT2-checkpoint.md` in the worktree). Progress log:
`/home/mcloonan/redoubt/.wash/local/INIT2-progress.md`.

## Gates last run (all green)
init-host-tests, init-build rv64/rv32, rt-host-tests, rt-build, rt-miri, host-tests,
bundle-mapped rv64/rv32, unsafe-budget, size-budget, `cargo +nightly fmt --check`, doccheck.
Whole bench not run yet. Run every cargo command as `/home/mcloonan/redoubt/.wash/local/in-dev …`.

## Rulings in force (all answered)
- d2 (a) public appended to bootfsd args: DONE. (b) fixed heap: DONE (runtime switch, not a cargo
  feature; orchestrator told). (c) device_info via `redoubt_sys::syscall` directly: used in the
  bin. (d) budget_usage(root) before fix_heap: done in the bin.
- Q1–Q5: done in code and pages. Q3 key move: done.

## Open questions with the orchestrator (sent, unanswered at handoff)
- (A) Ruling 4 wants refusals to power off with the SBI system-failure status (QEMU exit 255);
  `system_reset` has only kind 1 (off) and 2 (reboot). REC: kernel kind 3 (K16's paths, or leave
  for me). The bin uses kind 1 meanwhile, in `refuse()`.
- (B) Exit watching: one exit endpoint per server + a thread each costs root an IPC page and a
  stack per server, not in the bound. REC: add per server 1 IPC page + 4-page stack + tables to
  bound.rs (pin to objects.md "thread IPC page" in tests/bound.rs).
- (C) init's own root badge at keyd/consoled/bootfsd: REC smallest badge ≥1 not used by a handed
  item at that endpoint (compute in check.rs, put in Plan).
- (D) stub embedded by build.rs (DONE), bundle reader safe in lib (DONE), one unsafe in the bin
  (DONE, budget 0→1 with its line).
- (E) Unmapping the UART: rt `Registers` has no unmap and `handle::unmap` is private. REC: add
  `Registers::unmap(self)` in libs/rt/src/handle.rs with a fake-kernel test (needs leave).

## Next steps (deliverable 2, in the brief's order)
The bin `servers/init/src/bin/init.rs` today: root usage → fix_heap(ARENA_PAGES) → map UART
(handle 5) → bundle entries (`redoubt_init::bundle`) → device_info on handles 4.. → Machine →
read + check → prints the bound and powers off. Replace the last two lines with the boot:
1. For each server's `receives`: `Endpoint::create()` (init keeps the receive right; the child gets
   a copy via its slot: process_start copies). Per server: `Budget::from_handle(SYSTEM)
   .create_child(spec)` (pages/processes/weight), an exit endpoint, handed items minted from the
   receive right with their badge (`Endpoint::mint(NonZeroU64, None)`), device handles from
   `plan.placements[i]`, args from `check::args(m, s)`.
2. Launch with `redoubt_client::launch::Launch::new(STUB_BIN, image, budget, exit)` +
   `.handle(name, h)` / `.namespace("/dev/cons", conn)` / `.arg(a)`: safe, no ELF parsing.
   Images: `entries` (bundle) by `s.program`.
3. Order: keyd first, then `holds(key)` for each of `plan.keys` via
   `redoubt_client::typed::call::<keyd protocol>` (wire table libs/wire/tables/keyd.md opcode 4,
   `Holds { key }` → `held: u32`; see servers/keyd/tests/keyd.rs ~line 131/159). A yes refuses.
4. Unmap the UART (needs (E)), start consoled with `uart`/`uart-irq`; then init writes through its
   own connection (`redoubt_client::file::Connection::attach` on a root-badged handle to
   consoled), and each child's `/dev/cons` is `Grants::connection(lend, &console, "", 0)`
   (fresh connection per child), printing the child's connection id bare (ruling 6: fix one
   format, state it on consoled.md and testbench.md; d3).
5. Then bootfsd, blkd, netd, ipd (manifest order after consoled), then push public entries:
   bootfs `add {name, offset, data}` (opcode 16, lend) in chunks, then `seal` (17, inline); see
   servers/bootfsd/tests/bootfsd.rs `setup()`.
6. Exit watch per (B); print exits under the manifest name; no restart (INIT3).
7. Any charge failing during the boot is a bound bug: refuse (power off), never a partial boot.
Then d3 (consoled `[con N]` prefixes + host tests + announce line), d4 (testbench: boot the real
init with a manifest for the servers' cases; refusal attack cases; root usage after boot ≤ bound;
the todo/server-bucket-counts page and Sizing's "init does not exist yet" go with the bucket
case), d5 (`./mkimage` from image/boot.toml), d6 pages per ruling 3.

## Traps
- Never `git stash`, nor `git rebase --autostash` (it uses the stash). Fold with
  `git commit --fixup=<sha>` (or an `amend!` commit for a new message) then
  `GIT_SEQUENCE_EDITOR=: git rebase -q -i --autosquash d974e247c` on a clean tree.
- The rv64 target dir is `riscv64imac-unknown-none-elf`.
- Budget lines: `Size budget: <name>: <reason>` / `Unsafe budget: <name>: <reason>` in the commit
  that raises; the testbench now matches names with ": " whole.
- K16 owns kernel, loader, libs/sys, libs/layout, libs/paging, libs/stride, model, libs/rt/fake:
  no edits there; constants by name, no process/handle counts on pages.
- Page points still open with the orchestrator: init.md sharing list reordered (confirm); step 2
  "mints the badged handles the server's arguments name" contradicts Q1 (proposed line in the
  checkpoint file).
