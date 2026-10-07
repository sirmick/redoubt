# BEAM8 report: the VM's code is compact

Branch wp-BEAM8 on main fdafcf2cb, head 53bfafc8c. Two commits:

1. a3afe163d `beamlet: a module's code is 8-byte instructions over one operand array`:
   userland/otp/vm/src/{module,interp,loader,memory,vm}.rs, docs/userland/beamlet.md.
2. 53bfafc8c `image: beamlet budgeted 10,880 pages`: image/manifest.json,
   servers/init/tests/manifest.rs (fit sum), tests/beamlet-footprint.toml,
   tests/data/boot-profile/manifest-unverified.json, docs/testbench.md, docs/kernel/budgets.md.

The lookup-host fix (EROFS1's find) was my first commit until PACK1 merged the same fix
(877f7f482); dropped at the rebase as instructed. beamlet-lookup-host passed both widths with it.

## The design as built

- `Instr { op: u8, count: u8, start: u32 }`: 8 bytes, both widths. genop.tab's largest arity is
  8, so `count` is a `u8`.
- `Arg` 16 bytes, both widths. `Arg::List { start: u32, len: u32 }` indexes the module's one
  array `Module.operands: Box<[Arg]>`; an instruction's own operands come first, then its lists'
  items, so the indexed form keeps `Arg` at 16 with `Const(Term)` inside. `Arg` is now `Copy`.
- Sizes pinned by a compile-time assert in module.rs (`size_of::<Instr>() == 8 &&
  size_of::<Arg>() == 16`), which every build of either width checks; this replaces the brief's
  host test (a host test checks only the host's width). Accepted at the checkpoint.
- One accessor: `InstrView<'a>` (`op`, `arg(i)`, `count()`, `items(start, len)`), from
  `Module::instr(pc)`; interp.rs's `arg`/`u`/`label`/`list`/`src`/`dst`/`freg` read through it.
  8 `.args` sites replaced (interp 2, loader 5, memory 1) plus the native-body patch.
- The loader decodes straight into the operand array (placeholders, then each operand in place;
  a list reserves its items at the array's end), resolves labels per instruction in the old
  operand order (recursing into lists), so the same inputs give the same faults; both arrays are
  `into_boxed_slice`d (no spare room). A new `Malformed("code too large")` covers an operand index
  past u32 (unreachable below a 4 GiB file).
- Native-body patch: `Module::replace_body(entry, native)` (op becomes NATIVE_BODY, the label's
  one operand becomes the native's index). It lives in vm.rs's `load`, not userland/otp/redoubt:
  the one-line vm.rs call was granted by the orchestrator (vm.rs owned for that call only).
- `on_load_entry` reads only opcodes: unchanged.
- memory.rs: `code.instrs` = the instruction array's bytes, `code.operands` = the one operand
  array (lists' items included, no row of their own).
- interp.rs was read by function: the operand helpers, `step`'s fetch, `bs_get`'s destination,
  each function whose signature names an instruction, with the file's outline.

## Measurements (pages; beamlet-footprint on fdafcf2cb, rows rounded to nearest)

| Row | rv64 before | rv64 after | rv32 before | rv32 after |
| --- | ---: | ---: | ---: | ---: |
| code: instructions | 1,944 | 514 | 993 | 514 |
| code: operands | 4,455 | 2,081 | 2,313 | 2,081 |
| accounted | 7,660 | 3,856 | 4,502 | 3,791 |
| runtime held, peak | 7,876, 8,240 | 4,073, 4,537 | 4,616, 5,019 | 3,906, 4,373 |
| not accounted | 216 | 217 | 114 | 115 |
| scan's peak after one command | 9,368 | 5,240 | 5,511 | 5,067 |

Saving: 3,804 pages rv64, 711 rv32 (brief's estimate 4,670/1,165 was against BEAM6's
before-levers table; against today's code rows the estimate is 3,889/796). Code is 2,595 pages on
either width (estimate 2,510: Box<[T]> of a whole page rounds up). The table's runtime peak on main
predated the boot pack (PACK1 did not re-measure it); the page now says the peak above what is held
is the boot pack, held until the prompt, with a load's transient.

## Budget (six runs per memory case per width, 512 MiB, on 53bfafc8c's code at fdafcf2cb)

| Case | rv64 | rv32 |
| --- | ---: | ---: |
| init-boot | 2 to 985 | 2 to 3,396 |
| userland-boot | 5,228 to 5,229 | 5,055 |
| userland-read-only | 5,427 to 5,429 | 5,231 |
| beamlet-footprint | 5,240 | 5,067 |

All 48 PASS. Budget = roundup128(2 x 5,429 + 17 + 1) = 10,880; heap_pages 10,862 (4 over twice).
system free at 512 MiB, probed by a temporary oversized budget (reverted, never committed): rv64
31,652, rv32 31,610 (the page's 31,672/31,626 were stale). Servers need 21,259: spare 10,351 rv32,
10,094 with a 256-page client. Process/ETS limits 680 pages each; a flood (~4 x 680) plus the
prompt's ~4,100 fits under the cap.

## Flags

- Thin cap margin: 4 pages. An rv64 peak of 5,432 fails the scan's cap rule; the fix is the next
  step of 128 (11,008), which fits easily now. The rule as ruled gives 10,880.
- tests/data/boot-profile/manifest-unverified.json is outside my owned paths: init's test
  compares it with the image, so it must move with the budget.
- tests/boot-profile.toml and boot-profile-unverified.toml still run at 1 GiB with a comment that
  the image needs it (since BEAM6): not in this brief; left.
- tests/size-budget.toml has no VM entry (trusted crates only): nothing to move, no Size budget line.
- unsafe: none added; the ratchet unchanged.

## Summaries checked

docs/userland/beamlet.md (subsection table, representation sentence, residual on 256 MiB, lever
text): updated. docs/testbench.md (memory paragraph, beamlet row, rv32 peak, sixteenth) and
docs/kernel/budgets.md (RAM paragraph): updated. image/README.md, README.md, GETTING-STARTED.md,
userland/otp/README.md: no budget number or code representation cited (grep for 20,864/20864/
10,387/Vec<Arg>/operand): no change.

## Gate (head 53bfafc8c, base fdafcf2cb)

All exit 0, via `make -k -f /home/mcloonan/redoubt/scripts/jobs.mk -C <worktree>` after `prebuilt`
(rc=0): build-rv64, build-rv32, docs, rv64/formatting, rv64/size-budget, rv64/unsafe-budget
(count unchanged), rv64/no-cruft; both widths: beamlet-footprint, beamlet-boot, beamlet-console,
beamlet-heap-flood, beamlet-budget-flood, beamlet-lookup-host, beamlet-lookup-cli-host,
userland-read-only, verity-flipped-tree, verity-wrong-root, userland-boot, init-boot,
bench-net-peer, ipc-outcomes, init-host-tests (fit sum). Host: `q run --cores 4 -- cargo test -p
beamlet-vm -p beamlet-redoubt` in userland/otp rc=0 (37 + 6 + 14 + 2 + 2). No case failed beside
other work, so no quiet rerun. Not run: the whole bench (train's), the shell's mix test (untouched).
