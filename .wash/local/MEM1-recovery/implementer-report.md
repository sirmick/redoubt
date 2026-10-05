# MEM1 implementation report (in progress)

Branch: `wp-MEM1`. Base: `e9f2fcb95`.

## Page edits prepared for the docs window

- `docs/servers/init.md`: in the manifest table's `servers` row, change the end to "and arguments, and its stack in pages (`stack_pages`, 16 if absent, at most 128)". After Names add: "**Stacks.** A server's stack is the pages its first thread starts on, charged to its budget; `init` refuses 0, more than 128, or a stack its budget cannot hold. The bench measures each server's peak and holds the declared stack at twice it ([the memory budget](../testbench.md#the-memory-budget))." In Launching through the loader stub, step 2, change "the stack" to "the stack, painted with a pattern the bench reads ([the memory budget](../testbench.md#the-memory-budget))".
- `docs/kernel/memory-layout.md`, Launcher placement: the stack is at most `MAX_STACK_PAGES` (128) pages below `STACK_TOP`, so unmapped pages always separate it from the startup block. Also reconcile the User space regions table and memmap, which still show only the loader's 32-page stack despite launched stacks now reaching 128 pages.
- `docs/userland/native.md`, The client library: document `stack_pages` and `stack_tag`, and that every launch paints.
- `docs/kernel/budgets.md`: retain the `INIT_PAGES` stack-batch bullet; add that a server's stack is its manifest's `stack_pages`, charged to its own budget. Reconcile the hard-coded image bound of 446, stale against current test expectation 508 even before changed declarations, using the final manifest bound.
- `docs/testbench.md`: after The size budget, add `## The memory budget` with the `memory` key, paint, stopped QMP RAM scan, per-server line, twice-the-peak verdict and residual that only driven paths are measured. Add status references to new host scanner cases and machine case.

## Findings so far

- `tests/data/init/bound.json` had eight synthetic 16-page budgets, now 17 so each exceeds its default 16-page stack while retaining the old root-bound verdict. The root bound counts launch copies, not child budget ceilings.
- New `tests/init-refuses-stack.toml` overlays keyd with a 0-page stack and requires init refusal plus system-failure poweroff before any server starts; its verdict is from init and firmware.


- Direct-launch MAX_STACK_PAGES=128 boundary now has a host test that checks all 65,536 paint units through the final u16 index; 129 is refused before any kernel call. The obsolete fuzz Machine field removal is explicitly approved.
- Source confirms the launcher stack's fixed placement. `map_anon` uses the 0x6000_0000 default area; it cannot place memory immediately below stacks at 0x8000_0000. First-thread stacks are separate from additional threads' stacks, which are not measured here.
- The paint is public and can be forged in guest RAM. The scanner is evidence of case-driven paths, not a security proof of every path against hostile guest code.
- `servers/init/src/fuzz.rs` constructs the removed Machine field. Requested ownership of the one-line removal from the orchestrator.
- `stub/src/main.rs` enters the stub on the child's painted stack and jumps to the runtime entry without testing stack contents; `libs/rt/src/start.rs` parses only the startup page and does not assume a zeroed stack.
- A memory case's verdict is formed after console checks from QMP `stop` and `pmemsave` of guest RAM. Synthetic scanner cases fail a missing tag, a duplicate unit, and less than two times peak. `init-boot` has no server restart and forbids exits, so the duplicate check has a stable meaning.

## Affected summaries checked

- `README.md`: the current-state paragraph already says `init` boots the server set and does not claim stack sizing; no edit.
- `GETTING-STARTED.md`: its bench instructions already direct readers to `docs/testbench.md`; no stack claim to correct.
- `docs/plan/m1-separation.md`: progress already says `init` and the client library are built; no claim of fixed or absent stacks; no edit.
- `image/README.md`: it points the manifest and init-boot case to their owning pages; no stack claim; no edit.
- `docs/servers/init.md`, `docs/kernel/{budgets,memory-layout}.md`, `docs/userland/native.md`, `docs/testbench.md`: changes prepared above.

## Gate log

- `cargo testbench client-host-tests` via direct `docker run redoubt-dev`: exit 0; rerun after boundary test exit 0.
- `cargo testbench host-tests`: exited 137 when I stopped the run after `init-host-tests` failed to compile on the removed field; focused reruns cover affected host code.
- `cargo testbench init-host-tests`: initial exit 1 on `fuzz.rs`; second exit 1 on one explicit test Server constructor; third exit 0 after fixes; rerun after image declared fields exit 0.
- `cargo testbench init-build`: exit 0 (rv64 and rv32).
- `cargo testbench client-build`: exit 0 (rv64 and rv32).
- `cargo testbench formatting`: initial exit 1, then exit 0 after nightly rustfmt; rerun after boundary test exit 0.
- `cargo testbench unsafe-budget`: exit 0; client and init 0 unsafe, stub 7, all undocumented 0.
- `cargo testbench no-cruft`: initial exit 1 on local PAGE_SIZE, then exit 0 after using redoubt-sys's constant.
- `cargo testbench size-budget`: exit 1; requires ceilings stub 366, client 987, init 1957 (old 361/966/1952). Requested shared-file ownership before editing.

- `cargo testbench host-tests`: exit 0, including 382.6s model-host-tests.
- `cargo testbench --list`: initial command with missing container `rg` exited 127 and caused broken pipe; direct rerun exit 0 and parsed every case.

Pending: QEMU timing window for the two-width `init-boot` measurement and whole bench; docs window; size gate update; final image values, peaks, docs render/check, commit and final-head gates.
