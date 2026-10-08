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
  the interrupt reaches the shell whatever the stage does, and Ctrl+\ whatever a screen takes; a
  runaway allocation on the heap, by a line, anything it spawns or a screen program, is ended by
  that process's heap limit, and the session's processes together by its budget, which ends the
  session and nothing else.
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

Two tracks: the shell and several harts. They touch different code, the userland and the kernel,
so they run side by side. The shell's host steps need nothing
[M1 (sessions over SSH, kept apart)](m1-separation.md) builds, so they start ahead of it, as the
kernel on every hart under one lock does (below). The shell's steps on Redoubt and the rest of
several harts follow M1 (sessions over SSH, kept apart), the several-harts steps first: the kernel
has no other work in this milestone, and every shell case built after them then runs on several
harts too. The milestone is done only when the bench's cases pass booted with several harts as
well as with one.

### The shell

The shell itself is built on a host first, ahead of M1 (sessions over SSH, kept apart), where each
step is one a person can use, in this order:

1. **The terminal:** the host console, the terminal library, the shell's driver under `group` and
   `edlin`, line editing and history, and the interrupt ending a line
   ([beamlet](../userland/beamlet.md#the-console-on-a-host),
   [the shell](../userland/shell.md#the-terminal-library),
   [the shell](../userland/shell.md#line-editing-and-history)).
2. **Screens:** the screen natives, then `Redoubt.Screen`, its layout, and `pick` first
   ([beamlet](../userland/beamlet.md#screen-natives),
   [the shell](../userland/shell.md#full-screen-programs)).
3. **The pager, help drawn in it, and completion**
   ([the shell](../userland/shell.md#the-pager),
   [the shell](../userland/shell.md#completion)).
4. **The widgets:** the menu bar, dialogs, the input, the lists, the table and the canvas
   ([the shell](../userland/shell.md#widgets-focus-and-themes)).
5. **The editor**, then **the file manager** ([the shell](../userland/shell.md#the-editor)).
6. **Resource use** ([the shell](../userland/shell.md#resource-use)).

On Redoubt, after M1 (sessions over SSH, kept apart), in this order:

1. **Parking a typed call** in the serving library, then the console's `consol` protocol (`size`,
   `resize`) on it ([serving](../servers/serving.md#parking-a-typed-call),
   [consoled](../servers/consoled.md#the-consol-protocol)).
2. **The commands on Redoubt:** files through beamlet's platform, the session's own commands, and
   binds ([the shell](../userland/shell.md#session-commands),
   [files](../userland/files.md#copying-moving-removing-and-binds)).
3. **Native programs and pipes**: standard streams as served files, pipelines of native stages
   ([native programs](../userland/native.md#standard-input-and-output-and-pipes),
   [the shell](../userland/shell.md#native-programs-and-pipes)).
4. **Jobs**: one budget per native stage, `Job.kill`, the interrupt key over the console and SSH
   ([native programs](../userland/native.md#killing-a-job),
   [the shell](../userland/shell.md#interrupting-and-killing-jobs)).
5. **A native program's screen:** its `cells` frames read and drawn by the session
   ([the shell](../userland/shell.md#a-native-programs-screen-and-the-sessions-key)).

Beside them, **named contexts** ([sessions](../userland/sessions.md#contexts)): a context that
outlives its SSH connection, reattached or taken over, its console kept while detached by a relay
in its own budget, a cap per label set and an idle expiry; then the shell's commands to list,
detach and end contexts.

### Several harts

A second hart changes every rule that assumes one running thread in the kernel: completions, TLB
flushes, instruction fences and the scheduler's queue. So the steps go from coarse to fine, each
attacked before the next, in this order:

1. **Any boot hart.** The firmware may choose any hart, and nothing assumes hart 0. The loader
   already takes the boot hart's own PLIC context ([boot](../kernel/boot.md#the-argument-block)).
2. **Per-hart kernel state:** a trap stack and the current process and thread per hart, reached
   through `sscratch`, and scheduling on every hart.
3. **One big kernel lock** taken at trap entry, and one global run queue
   ([scheduling](../kernel/scheduling.md)). The lock is a FIFO ticket lock, so kernel entry is
   fair across harts ([R78 (fair kernel entry)](../kernel/scheduling.md#r78-fair-kernel-entry)):
   test-and-set would let one budget's harts starve another's. Every kernel global is reached
   only under it; the one-hart kernel is its uncontended case. The run queue stays global at
   every step below: each hart picks from the one stride queue, so a budget's share is judged
   across harts as on one.
4. **Cross-hart interrupts, shootdowns and fences.** Inter-processor interrupts to reschedule.
   A process's translations carry its PID as their ASID, and a hart keeps them across switches.
   A hart running the process when its tables lose a mapping is sent a shootdown, flushes that
   ASID and acknowledges before the page is reused; any other hart that ran it flushes that ASID
   before it next runs it, so a shootdown goes only to the harts running the process now. It
   comes in two stages: first at a destruction, while a budget runs on one hart at a time; then
   at every unmap, lend and return, once one process's threads run on several harts at once, as
   a beamlet VM's schedulers do.
   An instruction fence goes to the harts running a process when a page of it becomes
   executable, and a hart fences before it runs a process; a thread that moves needs no fence of
   its own ([memory](../kernel/memory.md#residual-risks),
   [memory layout](../kernel/memory-layout.md#residual-risks)).
5. **Lock to decide, unlock to do.** Frame zeroing, large copies and the wait for shootdown
   acknowledgements move outside the lock, on frames no other hart can name: a frame leaves the
   free-frame bitmap into an in-flight state before the lock is dropped, and enters a process
   only after the work is done and the lock is taken again. That invariant is a rule beside
   [R11 (memory)](../kernel/memory.md#r11-memory), with an attack case (a second hart racing to
   allocate, map or free the in-flight frame) before any work relies on it.
6. **Per-hart frame magazines,** each refilled from the bitmap under the lock and drawn from
   with interrupts off, so a hart's common allocation takes no lock at all; frames are zeroed as
   they enter a magazine, by step 5's rule.
7. **Finer locking:** one trap-entry lock whose guard owns a token, so that the kernel's globals
   become token-guarded cells and a compile-time lock order exists if the lock is ever split;
   the per-process thread-context pages first, and anything finer only once the steps above are
   stable and attacked.

Steps 1 to 3 and the first stage of step 4 are the package that puts the kernel on every hart
under one lock; the second stage of step 4 and steps 5 and 6 are the package that runs one
process's threads on several harts; step 7 waits on their measurements (lock hold time and
contention), as the [platform page](../beyond/fpga-platform.md#measurement-gates) says.

## Progress

On the host, ahead of the milestone: the shell's loop, its commands (the file and text commands
and `table`), and help, tested by the shell's own suite on beamlet and on the BEAM
([the shell](../userland/shell.md#the-loop)); the cell protocol, in Rust and in Elixir, held to
one set of vectors; and beamlet's raw terminal, the shell's driver under `group` and `edlin`
with `Redoubt.Term` drawing for it, line editing and the session's history, the interrupt ending
a line, and hostile text drawn visibly on every path the host has
([the shell](../userland/shell.md#line-editing-and-history)); and screens on beamlet:
the screen buffer's natives, `Redoubt.Screen` with its layout, and `pick`
([the shell](../userland/shell.md#full-screen-programs)); the widgets: the menu bar, dialogs,
the input, the lists, the table, the Braille canvas, focus and themes
([the shell](../userland/shell.md#widgets-focus-and-themes)); the pager, with help drawn in it
([the shell](../userland/shell.md#the-pager)); Tab completion of commands, variables, modules and
paths ([the shell](../userland/shell.md#completion)); and resource use, `free`, `uptime`,
`ps` and `top`, over the session's own budget and its VM, `df` not yet
([the shell](../userland/shell.md#resource-use)); and the editor, `ed`, with syntax
highlighting, and the file manager, `fm`, host-tested ([the shell](../userland/shell.md#the-editor)).
On Redoubt it runs only on the fake kernel (`./shell --fake`). What the milestone builds on: the serving
library's parked calls ([serving](../servers/serving.md#parked-calls)), `consoled`'s 9P console
([consoled](../servers/consoled.md)), and budget destruction as the only way to end a process
([budgets](../kernel/budgets.md#r10-destruction)). For several harts: every hart runs user code under one FIFO kernel lock, and a destruction's
shootdown is attacked (`bench:smp-boot`, `bench:smp-evict`). One process's threads run on
several harts at once, and every unmap, lend and return shoots the process down on the others
(`smp-shootdown`, `smp-fence`). For named contexts: a login names its context, `ssh alice.work@box`, the
steward holds one session per context at a time, and every refusal before a session reads the same
([sessions](../userland/sessions.md#contexts)).
