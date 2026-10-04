# INIT1: the loader loads only the kernel and init

Tier A (the loader, the kernel's boot and ABI, the bench's builder), size L. Needs GATE1 and
API1; it starts on an override. `wp-init1` rebases onto GATE1's merge before acceptance.

Every cargo and bench command runs as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree `/home/mcloonan/redoubt/.worktrees/init1`. Base it on main at `a9eed8300` or later:
that commit and `4a24c6f8b` settle the tester's program list and budgets on testbench.md.

## The package

Today the loader loads every bundle entry as a process, and the kernel builds the boot and log
endpoints between them. After INIT1:
- The loader loads exactly two images: the kernel, and the bundle's second entry in `init`'s
  place.
- It maps the whole verified bundle read-only into that process. The bundle's address is in `a0`
  and its length in `a1`.
- The process runs in `root`, which keeps one process and `init`'s weight for it, and gets
  `init`'s handoff.
- The kernel gains `device_info`, its 27th call, which says which device a handle names.
- Kernel cases run a trusted tester in `init`'s place. When the tester is the log server, it
  starts the case's other programs itself:
  - from the bundle's pages, through the loader stub;
  - each in a budget carved from `system`;
  - with the budgets the case names.

  `TAKE_GIFTS` goes.
- `bench-bundle-file` turns from a refusal into a read-back.

The real `init` is INIT2's. INIT1 builds nothing of `init` and no manifest.

## Rules first

- `.wash/SWARM.md`: "The implementer" (all ten rules) and "Staging, commits and handoffs":
  - stage by path;
  - never `git add -A`, `git commit -a` or `git stash`;
  - small commits;
  - a clean branch at acceptance.
- `CONTRIBUTING.md`: Tests, Documentation, Formatting, Commits.
- The design is read-only. A gap is a blocking question to the Architect, naming the page, the
  rule and the options.

## Reading list, in order

1. QA `INIT1-design` (`.wash/qa/INIT1-design.md`): the cut and the owner's four decisions. A
   tester in `init`'s place, with no R33 exception. `device_info` as a kernel call, not a
   loader-written list. consoled prefixes (INIT2's).
2. `docs/kernel/boot.md`:
   - "The loader loads only the kernel and `init`";
   - "Devices handed to the first program": the slots stay, and the boot and log endpoints leave
     the kernel's handoff;
   - "What the loader does";
   - "The argument block".
3. `docs/kernel/devices.md#device_info`, and "Which process gets which device".
4. `docs/kernel/budgets.md#the-tree-from-the-boot-manifest`: `root` keeps one process; `system`
   gets 15 and `users` 47; the weights stay.
5. `docs/testbench.md`:
   - "Starting a case's programs": the in-`init`'s-place half only, including the `programs`
     data entry and the per-program budgets, `4a24c6f8b` and `a9eed8300`;
   - "Data entries for `init`": the tester's read-back from the bundle's pages;
   - "Rule F".
6. An example of the work: `ab4eea14b`, "kernel, redoubt-sys, model: map_fixed". A new call
   across `libs/sys` (call, ret, error, tests), `kernel/src/redoubt.rs` dispatch, the model
   (syscall, trace, gen, kernel, mutation, a contracts test) and an attack case. `device_info`
   follows it file for file.
7. The code:
   - `loader/src/main.rs` ~180-275: the table of initial processes, the entry loop, `IniE`, the
     `grants` refusal, the stack lines;
   - `loader/src/args.rs`;
   - `loader/src/verify.rs`: read only, since it is the loader's verification hotspot.
   - `kernel/src/budget.rs`: `boot_budgets` (~584), `boot_endpoint`, `boot_log_endpoint`.
   - `kernel/src/ptable.rs` ~222: the `IniE` count.
   - `kernel/src/device.rs`: `boot_devices`, the device object's fields.
   - `kernel/src/redoubt.rs` ~136: `Call::MapDevice`, the dispatch.
   - `libs/sys/src/call.rs`: `MapFixed = 26` is the last.
   - `tools/testbench/src/build.rs`: `bundle()` ~227. `case.rs`: `Boot`, `Program`.
   - `tests/programs/src/bin/log-server.rs`, `logsrv.rs`, `rd.rs` (`take_gifts`), `spawn.rs`.
   - The 8 programs calling `take_gifts`: `budget-syscall-attack`, `lender-touches-lent`,
     `budget-destroy-attack`, `budget-table-attack`, `budget-carve-attack`, `kernel-half-attack`,
     `budget-forge-attack`, `legacy-gone`.

## Owned paths

- `loader/src/main.rs` and `args.rs`, but not `verify.rs`.
- Kernel:
  - `budget.rs`: `boot_budgets`, `boot_endpoint`, `boot_log_endpoint` only;
  - `ptable.rs`: the boot table;
  - `device.rs`: `device_info`;
  - `redoubt.rs`: one dispatch arm;
  - `arch/riscv/process.rs` `setup_loader_process`: `a0`/`a1` only, after K14 merges.
- `libs/sys`: the call.
- The model: `syscall.rs`, `trace.rs`, `gen.rs`, `mutation.rs`, a new
  `model/tests/device_info_contracts.rs`, and only the new call's arm in `model/src/kernel.rs`.
- `tools/testbench`: `build.rs` `bundle()`, `case.rs` (`budgets` on `Program`), `main.rs` where
  it packs.
- `tests/programs`: the log server (the `TAKE_GIFTS` fixture goes), `rd.rs`, the 8 programs and
  their `tests/*.toml`, and the new cases.
- Pages:
  - `docs/kernel/boot.md`, `devices.md`, `abi.md` (the call and error tables), `budgets.md`;
  - `docs/testbench.md`: the two sections above, planned to built;
  - `docs/SECURITY.md`, the rows whose status changes.

## Hotspots: do not touch, or coordinate first

- `kernel/src/message.rs`: IPC2's, then K13's.
- K14's lines, which it is changing now:
  - `kernel/src/process.rs` `process_map`;
  - `kernel/src/mem.rs` `ensure_range_exists`;
  - `kernel/src/arch/riscv/process.rs` `setup_loader_process` and `DEFAULT_STACK_SIZE`;
  - the loader's `USER_STACK_TOP` and `USER_STACK_PAGES` lines in the entry loop.

  INIT1 needs `setup_loader_process` (for `a0`/`a1`) and restructures the loop around the stack
  lines. Do deliverable 2 only after K14 merges, and rebase onto it. Before then, ask the
  orchestrator. Keep K14's stack lines as K14 leaves them, and move them as one block.
- `kernel/src/budget.rs` `destroy_subtree` and `process.rs` `index_process`: K15's.
- `model/src/kernel.rs`: IPC2 is changing R2 there. Add only the new call's arm, and rebase onto
  IPC2 before the model commit.

## Deliverables, in order

Every case runs on rv64 and rv32. Every verdict comes from the program in `init`'s place, which
owns the console and holds Reset, or from the kernel. Never from a program it started (rule F).

1. **`device_info`.**
   - The 27th call, with the result and errors devices.md gives: `BadHandle`, then
     `WrongObject`. It is constant-time and maps nothing.
   - The ABI and error rows go on abi.md.
   - The model gets the call, a contracts test and a mutation the family catches (for example
     `DeviceInfoWrongKind`).
   - New case `device-info-attack`:
     - the first program reads every device handle it holds, and checks the console's MMIO base
       and interrupt against QEMU `virt`'s;
     - then it tries a budget, an endpoint, a closed handle and an out-of-range index, and each
       gets the error in order.

   This needs no loader change. **Early checkpoint here.**
2. **The loader and the kernel's boot.** After K14 merges.
   - The loader loads the kernel and the second entry only. It parses no later entry: they are
     data, so `IniE` goes. If a case expects the `grants` refusal, ask before you change it.
   - It maps the bundle read-only into the second entry's process, outside its link range, and
     the kernel charges those frames to `root`. They are never freed.
   - `a0`/`a1` carry the bundle's address and length.
   - The kernel:
     - `root` keeps one process, `system` 15 and `users` 47;
     - the process runs in `root`, on `INIT_WEIGHT`;
     - it gets the same slots as today (budgets 1-3, Reset 4, the console 5-6, the devices on);
     - no boot or log endpoint.
   - Cases: every existing single-program kernel case passes unchanged. A new
     `bundle-mapped` case reads the bundle at `a0`/`a1` and finds its own entry's bytes. It also
     shows that a write to the mapping faults.
3. **The tester in `init`'s place.**
   - The builder packs the case's first program as the second entry, then the `programs` data
     entry, then the other programs and the files.
   - The log server, as tester:
     - creates the boot and log endpoints;
     - reads `programs` from the bundle;
     - starts each program through `spawn` from the bundle's pages, in its own budget: an equal
       share of `system`, weight 1,000;
     - gives each the slots of today and the budgets its line names;
     - badges by place, 2 on.
   - `TAKE_GIFTS`, `rd::take_gifts` and the op go. The 8 programs get `budgets = [...]` in their
     tomls.
   - Kernel lines that print a PID need a pattern in each case's `expect`.
   - Attack: a `programs` line naming an unknown budget or program refuses the boot, and is
     judged by the tester.
4. **Data entries.**
   - `bench-bundle-file` becomes a read-back: the tester compares the injected entry's bytes
     from the bundle's pages.
   - The other half, through `/boot` under `init`, is INIT2's.

## Gates

- `cargo testbench`: the whole bench, both widths.
- rv32 builds: the kernel, the loader, `redoubt-sys`, `redoubt-rt`, the servers.
- `cargo fmt --check` per CONTRIBUTING.
- The unsafe ratchet: it only falls. A new mapping's `unsafe` states its invariant.
- The size budget: the loader and the kernel grow. State the reason in the size table: a
  27th call, and the bundle mapping.
- `cargo run -q -p redoubt-doccheck`: clean, with every status line moved in the commit that
  lands its test.

Report each command with its exit code.

## Early checkpoint

After deliverable 1 is committed and green on both widths, report (member_update, at most 2000
bytes):
- the call's result encoding;
- the case's lines;
- the mutation's name and that it is caught;
- the unsafe count.

Wait for the go-ahead before deliverable 2.
