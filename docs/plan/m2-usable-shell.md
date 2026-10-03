# M2 (usable shell)

## Goal

The Elixir shell is a working environment. A person logged in over SSH does everyday work without
writing Elixir by hand:
- commands: short, typed Elixir functions, each with its help, for the everyday work;
- file operations (copy, move, rename, delete, make a directory) and binds;
- viewing and searching files;
- native programs with standard input and output, joined by pipes;
- interrupting and killing jobs, by budget destruction, with no signals;
- line editing, history, completion and help;
- showing resource use;
- full-screen programs drawn as cells through the session, from menus and dialogs to the pager;
- the editor and file manager.

And the kernel runs user code on every hart, not only the boot hart.

## Attack suite

Every convenience is attacked for the authority it might add. Each case runs a hostile input or
program against a real session and is judged by the session or the kernel, never by the hostile
party's own output.

- **Nothing runs at start.** A `.iex.exs`, or any other file, planted in the directory a session
  starts in is neither read nor run ([the shell](../userland/shell.md#the-loop)).
- **An interrupted line loses only itself.** Ctrl+C in the middle of `x = 1; loop()` ends the
  evaluation; the bindings of every earlier line are still there
  ([the shell](../userland/shell.md#interrupting-and-killing-jobs)).
- **A job ends, and only it.** `Job.kill` and Ctrl+C destroy the budgets of the job's native stages
  and everything they started, never the session
  ([R10 (destruction)](../kernel/budgets.md#r10-destruction),
  [the shell](../userland/shell.md#interrupting-and-killing-jobs)).
- **No program swallows the interrupt.** A foreground native stage never holds the raw console, so
  Ctrl+C reaches the shell whatever the stage does; a runaway allocation on the heap, by a line,
  anything it spawns or a screen program, is ended by that process's heap limit, and the
  session's processes together by its budget, which ends the session and nothing else.
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
- **A screen writes no control sequence.** Hostile text drawn by a screen program reaches
  `/dev/cons` only as visible characters, the screen buffer's natives refuse a control character,
  and a native program with a screen can send the session nothing but cells
  ([the shell](../userland/shell.md#full-screen-programs),
  [beamlet](../userland/beamlet.md#screen-natives)).
- **The screen natives hold under hostile arguments.** Every screen native, given any sizes,
  rectangles, text and styles, answers or raises `badarg` within its bound; none panics, and a
  buffer answers only the process that made it ([beamlet](../userland/beamlet.md#screen-natives)).
- **The editor turns no file into action.** A file crafted to look like a modeline, a script or a
  path is shown and edited as text, and saving writes only the file the editor was asked to; the
  case judges what is on the volume, not what the editor reports
  ([the shell](../userland/shell.md#the-editor)).
- **The file manager acts only inside its panel.** A 9P server that lists `..`, `a/../..` or a
  name holding a NUL gets each name shown and refused, and a copy, move or removal from the panel
  touches nothing outside the listed directory ([the shell](../userland/shell.md#the-editor)).
- **A paste is one event.** Pasted text arrives as one bracketed event, never triggers
  completion, and runs nothing until the person presses Enter, however many lines it holds
  ([the shell](../userland/shell.md#line-editing-and-history)).
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

Two tracks, side by side. The shell itself is built on a host first, where each step is one a
person can use, in this order:

1. **The terminal:** the host console, the terminal library, the shell's driver under `group` and
   `edlin`, line editing and history, and the interrupt ending a line
   ([beamlet](../userland/beamlet.md#the-console-on-a-host),
   [the shell](../userland/shell.md#the-terminal-library),
   [the shell](../userland/shell.md#line-editing-and-history)).
2. **Screens:** the screen natives, then `Redoubt.Screen`, its layout, and `pick` first
   ([beamlet](../userland/beamlet.md#screen-natives),
   [the shell](../userland/shell.md#full-screen-programs)).
3. **The pager, help drawn in it, and completion**
   ([the shell](../userland/shell.md#session-commands-and-the-pager),
   [the shell](../userland/shell.md#completion)).
4. **The widgets:** the menu bar, dialogs, the input, the lists, the table and the canvas
   ([the shell](../userland/shell.md#full-screen-programs)).
5. **The editor**, then **the file manager** ([the shell](../userland/shell.md#the-editor)).
6. **Resource use** ([the shell](../userland/shell.md#resource-use)).

On Redoubt, in this order:

1. **Parking a typed call** in the serving library, then the console's `consol` protocol (`size`,
   `resize`) on it ([serving](../servers/serving.md#parking-a-typed-call),
   [consoled](../servers/consoled.md#the-consol-protocol)).
2. **The commands on Redoubt:** files through beamlet's platform, the session's own commands, and
   binds ([the shell](../userland/shell.md#session-commands-and-the-pager),
   [files](../userland/files.md#copying-moving-removing-and-binds)).
3. **Native programs and pipes**: standard streams as served files, pipelines of native stages
   ([native programs](../userland/native.md#standard-input-and-output-and-pipes),
   [the shell](../userland/shell.md#native-programs-and-pipes)).
4. **Jobs**: one budget per native stage, `Job.kill`, the interrupt key over the console and SSH
   ([native programs](../userland/native.md#killing-a-job),
   [the shell](../userland/shell.md#interrupting-and-killing-jobs)).
5. **A native program's screen:** its `cells` frames read and drawn by the session
   ([the shell](../userland/shell.md#full-screen-programs)).

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
4. **Cross-hart interrupts, shootdowns and fences.** Inter-processor interrupts to reschedule.
   Every address-space switch flushes the whole TLB, so a process's translations live only on
   the harts running it now, and a TLB shootdown goes to exactly those harts, which flush and
   acknowledge before a page is reused. It comes in two stages: first at a destruction, while a
   budget runs on one hart at a time; then at every unmap, lend and return, once one process's
   threads run on several harts at once, as a beamlet VM's schedulers do. There are no
   address-space ids: they would widen the harts to flush to every hart that ran the process.
   An instruction fence goes to the harts running a process when a page of it becomes
   executable, and when a thread moves ([memory](../kernel/memory.md#residual-risks),
   [memory layout](../kernel/memory-layout.md#residual-risks)).
5. **Finer locking:** the per-process thread-context pages first, and anything finer only once
   the steps above are stable and attacked.

## Progress

On the host, ahead of the milestone: the shell's loop, its commands (the file and text commands
and `table`), and help, tested by the shell's own suite on beamlet and on the BEAM
([the shell](../userland/shell.md#the-loop)); the cell protocol, in Rust and in Elixir, held to
one set of vectors. On Redoubt it runs only on the fake kernel (`./shell --fake`). What the milestone builds on: the serving
library's parked calls ([serving](../servers/serving.md#parked-calls)), `consoled`'s 9P console
([consoled](../servers/consoled.md)), and budget destruction as the only way to end a process
([budgets](../kernel/budgets.md#r10-destruction)). For several harts: a two-hart spike, in which a
second hart started through SBI's hart management contends with the first on the kernel lock
without losing updates (`bench:smp-spike`, a checked build), and a few cases booted with two or
four harts, where the extra harts stay parked.
