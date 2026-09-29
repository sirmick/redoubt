# M2 (usable shell)

## Goal

The Elixir shell is a working environment. A person logged in over SSH does everyday work without
writing Elixir by hand:
- a command mode, where bare words stand in for quoted arguments;
- file operations (copy, move, rename, delete, make a directory) and binds;
- viewing and searching files;
- native programs with standard input and output, joined by pipes;
- interrupting and killing jobs, by budget destruction, with no signals;
- line editing, history, completion and help;
- showing resource use;
- full-screen programs drawn as cells through the session;
- the editor and file manager.

And the kernel runs user code on every hart, not only the boot hart.

## Attack suite

Every convenience is attacked for the authority it might add. Each case runs a hostile input or
program against a real session and is judged by the session or the kernel, never by the hostile
party's own output.

- **Command mode runs nothing hidden.** `cat #{File.rm("x")}` reads a file of that name and runs
  nothing; a local binding or import named like a command does not change what the command runs
  ([the shell](../userland/shell.md#command-mode)).
- **A job ends, and only it.** `Job.kill` and Ctrl+C destroy the budgets of the job's native stages
  and everything they started, never the session
  ([R10 (destruction)](../kernel/budgets.md#r10-destruction),
  [the shell](../userland/shell.md#interrupting-and-killing-jobs)).
- **No program swallows the interrupt.** A foreground native stage never holds the raw console, so
  Ctrl+C reaches the shell whatever the stage does; a runaway evaluation is ended by its heap limit
  before it takes the session's budget.
- **A pipe carries no authority.** A native stage reaches only what its launcher bound into its
  namespace; its standard streams are served files, and nothing is inherited
  ([native programs](../userland/native.md#standard-input-and-output-and-pipes)).
- **Resource use shows only one's own.** `top`, `ps` and friends list only the caller's own
  (account, label set): another principal's work, or a vault session's from an ordinary one, never
  appears ([the shell](../userland/shell.md#resource-use)).
- **Completion and help reveal nothing unreadable.** Completion lists only entries the caller's
  labels may read, launches nothing and writes nothing
  ([the shell](../userland/shell.md#completion)).
- **Hostile text never drives the terminal.** A file, a file name or a program's output holding
  control sequences (OSC 52, OSC 8, a title report, a bare ESC) reaches `/dev/cons` only as
  visible characters; the case judges the bytes the session wrote
  ([the shell](../userland/shell.md#the-terminal-library)).
- **A full-screen program holds nothing.** `render` and the editor never reach `/dev/cons`, and a
  hostile `render` or a file crafted against the editor reaches only what that program was bound
  ([the shell](../userland/shell.md#full-screen-programs)).
- **A paste is one event.** Pasted text arrives as one bracketed event and never triggers
  completion ([the shell](../userland/shell.md#the-terminal-library)).
- **Binds add nothing.** A bind makes a held capability appear at another path and cannot point
  anywhere the process could not already reach
  ([files](../userland/files.md#copying-moving-removing-and-binds)).
- **A parked console call is accounted.** The console's `resize`, parked as a typed call, holds
  exactly one admission slot of its caller's share
  ([R28 (parked-call accounting)](../servers/serving.md#r28-parked-call-accounting)).
- **Several harts add no reach.** A page unmapped, lent or returned on one hart is never reachable
  through another hart's TLB, and a page made executable on one hart never runs stale on another
  ([memory](../kernel/memory.md#residual-risks)); completion races between harts on one call are
  attacked, where on one hart they are argued
  ([kernel attack gaps](../todo/kernel-attack-gaps.md)); and every scheduling case, rerun with
  several harts, shows that no budget gains share by being spread across them
  ([R12 (scheduling)](../kernel/scheduling.md#r12-scheduling)).

## Remaining work

Two tracks, after [M1 (separation and containment)](m1-separation.md). They touch different code,
the userland and the kernel, so they run side by side from the milestone's start, with the
several-harts track begun first: the kernel has no other work in this milestone, and every shell
case built after it then runs on several harts too. The milestone is done only when the bench's
cases pass booted with several harts as well as with one.

### The shell

In this order:

1. **Parking a typed call** in the serving library, then the console's `consol` protocol (`size`,
   `resize`) on it ([serving](../servers/serving.md#parking-a-typed-call),
   [consoled](../servers/consoled.md#the-consol-protocol)).
2. **The terminal library**, line editing and history
   ([the shell](../userland/shell.md#the-terminal-library)).
3. **The helpers and file operations**, viewing and searching, then binds
   ([the shell](../userland/shell.md#helpers-and-file-operations),
   [files](../userland/files.md#copying-moving-removing-and-binds)).
4. **Command mode**, as a preprocessor ahead of the Elixir parser
   ([the shell](../userland/shell.md#command-mode)).
5. **Native programs and pipes**: standard streams as served files, pipelines of native stages
   ([native programs](../userland/native.md#standard-input-and-output-and-pipes),
   [the shell](../userland/shell.md#native-programs-and-pipes)).
6. **Jobs**: one budget per native stage, `Job.kill`, the interrupt key over the console and SSH
   ([native programs](../userland/native.md#killing-a-job),
   [the shell](../userland/shell.md#interrupting-and-killing-jobs)).
7. **Full-screen programs**: `render` with the cell backend, the vendored crates it needs
   ([the shell](../userland/shell.md#full-screen-programs),
   [libraries](../userland/native.md#libraries-for-native-programs)).
8. **Completion, help and resource use** ([the shell](../userland/shell.md#completion)).
9. **The editor and file manager** ([the shell](../userland/shell.md#the-editor)).

### Several harts

A second hart changes every rule that assumes one running thread in the kernel: completions, TLB
flushes, instruction fences and the scheduler's queue. So the steps go from coarse to fine, each
attacked before the next, in this order:

1. **Any boot hart.** The firmware may choose any hart, and nothing assumes hart 0. The loader
   already takes the boot hart's own PLIC context ([boot](../kernel/boot.md#the-argument-block)).
2. **Per-hart kernel state:** a trap stack and the current process and thread per hart, reached
   through `sscratch`, and scheduling on every hart.
3. **One big kernel lock** taken at trap entry, and one global run queue
   ([scheduling](../kernel/scheduling.md)).
4. **Cross-hart interrupts, shootdowns and fences.** Inter-processor interrupts to reschedule; a
   TLB shootdown (SBI remote fences, by address-space id) on unmap, lend and return before a page
   is reused; an instruction fence on every hart when a page becomes executable, and when a thread
   moves ([memory](../kernel/memory.md#residual-risks),
   [memory layout](../kernel/memory-layout.md#residual-risks)).
5. **Finer locking:** the per-process thread-context pages first, and anything finer only once
   the steps above are stable and attacked.

## Progress

Nothing of this milestone is built. What it builds on: the serving library's parked calls
([serving](../servers/serving.md#parked-calls)), `consoled`'s 9P console
([consoled](../servers/consoled.md)), and budget destruction as the only way to end a process
([budgets](../kernel/budgets.md#r10-destruction)). For several harts: a two-hart spike, in which a
second hart started through SBI's hart management contends with the first on the kernel lock
without losing updates (`bench:smp-spike`, a checked build), and a few cases booted with two or
four harts, where the extra harts stay parked.
