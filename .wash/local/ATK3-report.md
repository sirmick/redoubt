# ATK3 — attack cases for devices.md's seven partly-tested lines

Base: main 985ec0047. Branch wp-ATK3, worktree `/home/mcloonan/redoubt/.worktrees/ATK3`.
Head: 0e650b847 (one commit on wp-ATK3; tree clean)

## Delivered

Three new boot cases, both widths, checked builds, every verdict the kernel's answer or line:

- `device-object-page` (tester `tests/programs/src/bin/device-object-page.rs`, feature
  `dma-reset-deaf`): `system`'s usage at boot is exactly one page per device object it owns
  (25 on QEMU virt, counted by `device_info` over the handles; the kernel's own line says
  `Devices: 25 objects`); a driver in a budget carved from `system` *before* the measure
  allocates a run through an empty virtio-mmio slot and faults, the slot's reset does not confirm,
  the quarantine destroys the object: `system` goes from N to N−1 pages exactly and the slot's
  handle is `BadHandle`.
- `dma-max-runs` (tester `dma-max-runs.rs`): a child gets 32 one-page runs through one slot and
  `OutOfMemory` for the 33rd (exit code = runs got); its places come back at its end, so the first
  program gets 32 and `OutOfMemory` for the 33rd and for a 2-page run, while `root` has ≥ 64 pages
  of room and a run through a second slot is `Ok` (the refusal is the device's, not the budget's
  nor the pool's); a second child gets 0 runs through the full device (the limit is the device's,
  not the caller's).
- `device-owner-destroy` (tester `device-owner-destroy.rs`): the first program, in `root`,
  parks a thread in `receive` on the console's IRQ and destroys `system`; the waiter gets `Dead`;
  all 25 device handles answer `BadHandle` (`device_info`, `map_device`, `receive` on the IRQ,
  `system_reset` on the Reset right) and so does `budget_usage(system)`; the console mapping made
  before survives (that is how the tester prints), and a byte the bench sends after the
  destruction (`[[input]]`) reaches the UART by polling. No power-off: the Reset right dies with
  `system`; the case ends on its lines.

Files: `tests/{device-object-page,dma-max-runs,device-owner-destroy}.toml`, the three testers,
`tests/programs/Cargo.toml` (three bins), `docs/kernel/devices.md`, `docs/SECURITY.md` (R5 row's
residual text), `docs/todo/kernel-attack-gaps.md`.

Kernel and loader: unchanged (the plants below were reverted, `git status` clean of `kernel/`).

## The seven lines

| Line | Fate |
| --- | --- |
| The page a device object costs its owner | **closed**: `device-object-page`, one page per object at boot and one back at a quarantine, exact |
| A 17th DMA device gets no object | **left**: QEMU virt has eight virtio-mmio slots; the bench has no way to hand QEMU a device tree (`qemu_args` fixed per target, no `dtb` key); the plan's "as the boot cases pass bad blocks today" is not so: no case passes a crafted block or tree. Written on the page (Residual risks, "Crafted boot inputs are not attacked"). A bench `dtb` key would reach it; not built (not kernel/loader, but outside the package's files) |
| `dma_alloc`'s `MAX_RUNS` | **closed**: `dma-max-runs`; status now "tested (5)" |
| The loader's refusal of no console / console without interrupt | **left**: needs a crafted device tree, as above (the loader filters at `dt.rs`, refuses in `emit_devices`). Also `Grnt`: no device tree can produce it; the loader alone emits tags. Stated on the page |
| R5's order (claim completed before the mask) and billing | **argued** on the page, Residual risks: one kernel section, no S-mode interrupt, under the lock; the mask is a priority write with enable bits kept, so the PLIC ignores no completion and even the reverse order loses nothing on this controller; QEMU accepts either, so no case tells them apart. Billing: `bill_irq` charges the owner the IRQ index names (`sched.rs`); attacking it on the target is scheduling.md's open line |
| R18's malformed `Devs` / `Grnt` refusals | **left**: the loader emits neither; no case hands the kernel a crafted block. A `dtb` key would reach the `Devs` ones (a region straddling RAM's start, a non-controller node over the PLIC, an IRQ ≥ 1024 pass the loader's filter) but never `Grnt`. Stated on the page |
| Owner budget destroyed (waiters `Dead`, source masked, handles swept); handle closes with its stamping budget | **closed in part**: `device-owner-destroy` attacks `Dead` and the sweep (and the Reset right's and `system`'s going). **The mask is argued** (Residual risks): nothing a process can see tells a masked source from an unowned one; the kernel's `[!] Masked IRQ #n, which no device object owns` line prints only under `debug-print` (far too noisy for a case: every switch logs), and the checked build's claim count prints only at `system_reset`, which the destruction removes. **The stamp line is unreachable by design**: every device handle is stamped `root` (boot), copies keep stamps, `mint` takes endpoints only; stated on the page |

The plan body's "judged by ... the oracle's masked source": no such oracle exists; see above.

## Negatives (one planted kernel build, three plants, each caught by its own case; reverted)

Plants (`.tmp/ATK3/negative/plants.diff` in the worktree):
1. `device.rs` `free_device`: the owner is never uncharged →
   `FAIL device-object-page [rv64]: forbidden output ... system 128 -> 128 pages, one page back`.
2. `dma.rs` `MAX_RUNS = 33` →
   `FAIL dma-max-runs [rv64]: ... a child ... got Some(133) and then OutOfMemory` (133 = OTHER+33:
   the child got 33 runs and the tester stopped asking).
3. `message.rs` `destroy_device`: `fail_each(irq_waiters)` skipped →
   `FAIL device-owner-destroy [rv64]: forbidden output /PANIC/: PANIC in PID 2: panicked at
   kernel/src/mem.rs:829:9` — the checked kernel's I1 audit (`object_phys`: "is no object frame")
   trips on the waiter left naming the freed frame: the kernel's own verdict.
Logs: `.tmp/ATK3/negative/*.log`.

Not planted: the mask at destruction (`disable_irq` skipped) — the case would pass, which is the
point of the page's statement; not spent a rebuild on.

## Gates

All on the clean prebuilt after the plants were reverted (`.tmp/ATK3/gates.log` in the worktree):
- `make -f scripts/jobs.mk prebuilt`: rc=0 (rv64 302 cases, rv32 288, 0 failed).
- The three new cases, `make -j4 set ... WIDTHS=rv64,rv32`, three runs: 18/18 PASS, 0 FAIL
  (device-object-page ~0.7 s, dma-max-runs ~0.7 s, device-owner-destroy ~1.7 s).
- Fixture-sharing set (device, irq-attack, dma-rules, device-info-attack, dma-reset-quarantine,
  dma-reset-reuse, dma-destroy-quarantine, irq-first-receive, legacy-gone, budget-destroy-kills,
  device-exec-refused, uart-irq, receive-bad-record, pid-reuse-authority, init-boot,
  init-console-forgery, init-driver-restart, init-handed-revoked, init-programs,
  init-quarantine-reboot, init-reboot, init-refuses-* (16), init-restart, init-rollback,
  init-servers, loader-rejects-kernel-address, loader-rejects-kernel-entry,
  loader-rejects-truncated-elf, rustsbi-boot), both widths: 79 PASS, 0 FAIL, 0 SKIP (the rest
  rv64-only by their toml).
- `make -f scripts/jobs.mk docs`: PASS (after one citation fix, `R9 (stamps)`).
- `rustfmt +nightly --check --config skip_children=true` on the three testers: clean.
- unsafe count: no new `unsafe` outside the testers' documented UART mapping and the planted
  fault store (the same lines the quarantine tester carries); kernel/loader untouched.

## Summaries checked

- `docs/kernel/devices.md`: six status lines updated (Device objects, `dma_alloc`, Devices handed,
  R5, R18, Failure and restart), the Device objects prose names the measure, Residual risks: the
  R5 bullet rewritten with the arguments, three bullets added (crafted boot inputs; the mask at
  an owner's destruction; a device handle's stamp is `root`).
- `docs/SECURITY.md` R5 row: residual text "Two halves" → "Three parts ... argued" to agree with
  the page; R18 row unchanged (still partly tested, residual list still true).
- `docs/todo/kernel-attack-gaps.md` devices section: two lines removed (closed), five rewritten
  with what remains and why.
- `docs/kernel/boot.md` lines 153, 300 (the same crafted-block/tree gaps from boot's side): ATK4's;
  unchanged, consistent.
- `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/kernel/README.md`,
  `docs/plan/m2-usable-shell.md`, `kernel/README.md`: grep for the lines' terms found no claim; no
  change needed.

## Open

- A `dtb` key in the bench (QEMU `-dtb`, a tree built host-side since the host has no `dtc`;
  QEMU still adds the initrd to a supplied tree but not the rng seed, which the tree would carry)
  would make four of the left lines attackable. Suggest a small follow-up package, shared with ATK4
  whose boot.md lines are the same gaps.
