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
- the editor.

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
- **A paste is one event.** Pasted text arrives as one bracketed event and never triggers
  completion ([the shell](../userland/shell.md#the-terminal-library)).
- **Binds add nothing.** A bind makes a held capability appear at another path and cannot point
  anywhere the process could not already reach
  ([files](../userland/files.md#copying-moving-removing-and-binds)).
- **A parked console call is accounted.** The console's `resize`, parked as a typed call, holds
  exactly one admission slot of its caller's share
  ([R28 (parked-call accounting)](../servers/serving.md#r28-parked-call-accounting)).

## Remaining work

In this order, after [M1 (separation and containment)](m1-separation.md):

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
7. **Completion, help and resource use** ([the shell](../userland/shell.md#completion)).
8. **The editor** ([the shell](../userland/shell.md#the-editor)).

## Progress

Nothing of this milestone is built. What it builds on: the serving library's parked calls
([serving](../servers/serving.md#parked-calls)), `consoled`'s 9P console
([consoled](../servers/consoled.md)), and budget destruction as the only way to end a process
([budgets](../kernel/budgets.md#r10-destruction)).
