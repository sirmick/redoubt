# CONW1 design: one console writer on several harts

Base: origin/main 3206c43b4, worktree .worktrees/CONW1 (branch wp-CONW1, no code yet).
Spec (plan node): kernel lines and programs' UART writes are line-atomic through one lock or one
path; budget-destroy-kills' verdict does not move; then sched-latency-tcg runs at 2 harts. Tier A
(kernel), the kernel red.

## 1. Who writes the UART today

One 16550 (the device tree's stdout-path; the loader's first MMIO `Devs` entry, kernel/boot.md).
Two kinds of writer reach it, with nothing between them:

| Writer | Path | When | Its own serialisation |
| --- | --- | --- | --- |
| the kernel | `print!` -> `debug::console` -> `SbiConsole::putc` -> `sbi_rt::console_write_byte`, one ecall per byte; the firmware (RustSBI) writes the same UART from M-mode | boot, kills (`ptable.rs:178` `[!] Terminating process with PID n`), faults, device and DMA notices, `system_reset`, the sched-trace dump at power-off (thousands of lines), panics (`print_stateless`) | `KERNEL_LOCK` (every print today runs under it), and `IN_PRINT` for a print inside a print |
| init | `Out::Uart`: the console MMIO mapped (`init.rs:875`) | its own lines before consoled serves | none needed (one thread) |
| consoled | MMIO, `Console::write` -> `Lines::write` -> `uart.put_all` | every console line under init | one serving thread |
| a kernel case's first program (log-server, or proc-test, device-test, the scheduler bench) | MMIO, `tests/programs/src/console.rs` `line`/`relay`/`Console`, one whole line under its `busy` spin flag | every program line in the kernel cases | `busy`, between its own threads only |

So each side keeps its own lines whole, but the two sides interleave byte by byte: under icount the
harts take turns, so it is rare; under TCG two harts run at once, so it happens in nearly every
killing run (docs/kernel/scheduling.md, "Residual risks": "The console has two writers").

## 2. Options

| | What | For | Against |
| --- | --- | --- | --- |
| A | Keep both writers; **a console hold** in the kernel. The UART holder takes it around each write chunk (a syscall pair). A kernel line is printed at once when nobody holds it, else queued in a kernel line ring and printed when the holder lets go | the kernel **never waits on user mode** (no stall under `KERNEL_LOCK`, no hang from a holder that never lets go); kernel lines keep their place before any user line begun after them; programs keep their own MMIO writes, so consoled's large frames do not pass through the kernel | two syscalls per write chunk in each holder; a small ABI addition; cooperative: a holder that never takes it is as today |
| B | Every program write through the kernel (`console_write(bytes)`) | one writer, exact order | consoled's output (screen frames, KiB per frame) serialises through the kernel byte by byte as SBI ecalls; a device the kernel owns no longer a userspace server's (kernel/README "No drivers") |
| C | Kernel lines handed to the UART holder (a kernel ring consoled or log-server drains) | one writer | rule F: kernel-only lines (`[!] Terminating`, no `[pid` prefix) would be printed by a process; nothing prints when there is no holder or it is dead (boot, panics, kernel cases without log-server); a wake path |
| D | A shared lock word in memory both sides see (no syscalls) | cheapest per line | the kernel either waits on a word user mode holds (a stall under `KERNEL_LOCK`, or a hang) or does A's ring anyway; a kernel page mapped into user memory, or a pinned user page the kernel writes: new ownership rules (ABI2) for a few hundred nanoseconds |
| E | The kernel on another device (a second UART, virtio-console) | no sharing at all | QEMU virt has one 16550; the bench reads one console; FPGA boards likely the same |

**Recommended: A.** It is the spec's "one lock" with the property the kernel needs on several
harts: no kernel path ever waits for user mode.

## 3. A in detail

- **The hold.** Kernel state, with its own spinlock (not `KERNEL_LOCK`, so a print from outside it
  after SMP4/IRQ1 is fine):
  `holder: Option<Pid>`, `ring` (fixed, e.g. 4 KiB static, whole lines), `dropped: u32`.
- **The call.** One new call, `console_hold { device: Handle, hold: bool }`, in libs/sys's call
  table, the kernel's dispatch (redoubt.rs), the fake kernel (accepts, does nothing) and an rt
  wrapper. The handle must be a device object for the console's MMIO (the kernel marks it at boot:
  the loader's first MMIO entry, which kernel/boot.md already pins), else `WrongObject`. Only init
  holds it at boot; it hands it to consoled. In kernel cases, the first program. A process that has
  no handle cannot hold, and so cannot delay kernel lines.
  - `hold: true`: if the ring has lines, print them first, then `holder = pid`. If another process
    holds it: `Busy` (cannot happen with one owner; a second owner gets no hold and writes as today).
    Taking it again is a no-op.
  - `hold: false`: `holder = None`, then print the ring's lines before returning, so they come
    before the holder's next line.
- **A kernel line.** Formatted as today. With no holder it goes out at once (today's path). With a
  holder it is appended whole to the ring. With the ring full it goes out at once anyway
  (`dropped` counted, a residual): the kernel never waits and never loses a line, and a holder
  stuck holding costs only the lines' wholeness, as today. Panics and `print_stateless` stay
  direct.
- **A holder that dies holding** (killed mid-write): `terminate` drops its hold and prints the ring
  after a `\r\n`, so the kernel's lines start a line of their own after the cut-off one.
- **The holders' change.** One hold and one release around each write chunk:
  - consoled: around `Lines::write` in `FileServer::write`. One 9P write is bounded by its lend
    (`MAX_LEND_PAGES`).
  - tests/programs console.rs: inside `locked`, after `busy` is taken, so threads keep their order.
  - init's `Out::Uart`: per line, before consoled starts.
- **Order, and budget-destroy-kills.** `[pid 4] [destroyer] destroying system` is written by
  log-server (held, released) before the destroyer's `budget_destroy`. The kills' lines then go
  out at once if log-server holds nothing, or at its next release otherwise, which comes before
  it starts the `[server] ... ended` lines (they follow the exit notices the kills send). The
  expect list's order is unchanged; the lines are whole. The ring's lines keep kernel order. The
  case's patterns and its rule-F reading (only the kernel prints `[!]` lines) are untouched: the
  kernel still prints them itself.

## 4. Cost

- **The kernel's print:** an uncontended spinlock and a branch, plus a copy into the ring when
  held. Never a wait on user mode. The SBI byte writes are as today, done in the releasing
  holder's call when deferred: charged to that thread's kernel section, at most a ring's worth
  (4 KiB, about 80 lines).
- **A holder:** two syscalls per write chunk, each taking `KERNEL_LOCK` as any call does. That
  is about 2 x (trap + lock) per console line in kernel cases, and per 9P write in consoled.
  - To measure: sched-latency(-tcg) at 2 harts (log-server's lines during the sweep), steward
    session cases (consoled's frames), and the boot profile (init's lines).
  - If consoled's cost shows, the hold can span one serving turn's writes.
- **Contention:** the kernel's console spinlock is held for a ring copy or a print. Its holders are
  the kernel's own prints, already serialised by `KERNEL_LOCK` today.

## 5. Tests

- **Host:** the hold and ring as a pure module (kernel lib or a small crate like redoubt-ipclist):
  - print with no holder: out at once;
  - held: queued, out at release in order;
  - full: out at once and counted;
  - holder death: `\r\n` then the queue;
  - re-take is a no-op; a second pid is `Busy`.
- **Bench, both widths:**
  - **`console-one-writer`** (TCG, 2 harts, no icount): the first program writes long lines in a
    tight loop on one hart while children are killed in a loop on the other. Every
    `[!] Terminating process with PID n` line and every program line must match its anchored
    pattern, N of each; a torn line fails the count. The verdict is the kernel's own lines and
    the checker's count, not the attacker's (rule F).
  - **`console-hold-stuck`** (attack, 2 harts): a first program holds and never releases, then
    kills. Each kill line still appears (ring, then full -> direct), and the system still serves
    (log-server's DONE from another reporter).
  - A death while holding: covered in the host test, and in the bench case if cheap.
- **budget-destroy-kills:** unchanged expect list, run at 1 and 2 harts.
- **Then sched-latency-tcg:** drop `keep_smp`, run it at `smp = [2]`, and sweep (both widths,
  several runs) for no broken `[latency]` line. scheduling.md's residual risk ("two writers") is
  removed. Its keep_smp table loses no row (the case is not in it).
- **The rest:** the kernel set at 2 harts (`--smp 2`) for any expect a deferred kernel line might
  reorder against a program line; the init and steward sets for consoled's hold.

## 6. Files and coordination

- kernel/src/debug/console.rs (hold, ring), platform/sbi/mod.rs (unchanged API), redoubt.rs and
  arch/riscv/syscall.rs (the call), device.rs (mark the console's device object at
  `boot_devices`), ptable.rs (`terminate`: drop a dead holder's hold, one call).
- libs/sys/src/call.rs, ret.rs, error.rs and tests.rs (the call); libs/rt (wrapper, fake kernel);
  servers/consoled; servers/init (`Out::Uart`); tests/programs/src/console.rs.
- Docs:
  - kernel/README.md ("No drivers": the console's hold);
  - kernel/scheduling.md (the residual risk leaves);
  - the page that owns the call table (kernel/syscalls or the ABI page);
  - consoled.md (its writes hold the console);
  - testbench.md if a case class changes.
- SMP4 (destruction outside the lock) and IRQ1 (PLIC on every hart) edit the kernel at the same
  time. I asked both which of these files they touch and whether any `println!` moves outside
  `KERNEL_LOCK` (the ring's own spinlock covers that either way). Answers go into this note
  before code.

## Questions

1. Option A (a kernel console hold with a deferred line ring; the kernel never waits) over B–E?
2. On a full ring, print at once (lines whole unless a holder is stuck, never lost) rather than
   drop or wait?
3. The call's shape: `console_hold { device, hold }` on the console's device handle (one new call
   number), rather than a capability of its own minted at boot?
4. The hold's span in consoled: one 9P write (simplest), with "one serving turn" kept in reserve
   if the cost shows?
