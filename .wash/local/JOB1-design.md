# JOB1 design checkpoint: jobs, one budget per native stage, Job.kill, the interrupt

M2 step 4 ("On Redoubt"). Base: origin/main 864a2ac82 (PIPE1 merged). No code written.

Read:
- docs/plan/m2-usable-shell.md: the goal, and three attack lines: "An interrupted line loses only
  itself", "A job ends, and only it" and "No program swallows the interrupt".
- docs/userland/native.md, "Killing a job".
- docs/userland/shell.md: "Interrupting and killing jobs", "Session commands" (follow) and "A
  native program's screen and the session's key".
- .wash/local/PIPE1-design.md section 5.
- The code: userland/shell/lib/redoubt/{pipeline.ex, pipes.ex, process.ex, shell.ex,
  shell/evaluator.ex, shell/driver.ex}.
- docs/kernel/budgets.md R10.
- docs/servers/sshd.md: "A pty session".
- tests/sshd-loopback-interrupt.toml.

## 1. What is built, and the gap

| Piece | State |
| --- | --- |
| One budget per native stage, carved from the session's; every stage budget destroyed when the last stage ends | built (PIPE1: `Redoubt.Pipeline`) |
| A job's owner process: launches the stages, gets their exit notices, watches the line that asked and destroys every stage budget if that line ends first | built (PIPE1; bench:pipe-interrupted kills the line's process by hand) |
| Endings `{exited, Code}`, `{faulted, Cause}`, `{killed, 0}` from the exit notices | built (beamlet `launch`, Pipeline endings) |
| sshd turns the channel's `signal` (INT) and `break` into 0x1C | built (CONS1; ssh-loopback:sshd-loopback-interrupt) |
| The driver: Ctrl+C or Ctrl+\ drops the line being *edited*; ends a screen | built |
| **The interrupt while a line evaluates** | **not built.** `Driver.key/3` drops the key when no line is open ("ending that is not the driver's yet"). Under an open feed (exec reading the console), `feed_key` drops it too. So `x = 1; loop()` cannot be interrupted today, and neither can a foreground pipeline. |
| `Job.kill/1`, `Job.status/1`, `Job.await`, `jobs()`, background jobs | not built |
| `follow(path)` | not built; deferred here by RCMD1 because nothing could end it |

The gap is small. The killing machinery (budgets, owner, endings) exists. JOB1 adds the
interrupt's path from the driver to the evaluating line, the `Job` handle, and background jobs if
ruled in.

## 2. What a job is

**A job is one pipeline**: an `exec` (one stage) or a `pipe/1` (up to 7), with its owner process,
its stage budgets and its pipes. Its state is `:running` until its last stage ends, then
`:exited`, `:faulted` or `:killed` (the last stage's ending; every stage's ending stays readable).
The owner is the job: `Job.kill` and `Job.status` are calls to it.

**The Elixir line is not a job**, but the interrupt ends it. It is the foreground evaluation (the
`Redoubt.Shell.Evaluator` process) and whatever is linked to it.
- The interrupt kills it with `:kill`, which is untrappable, and the linked processes go with it.
- Every foreground job the line started goes too: its owner already watches the line's process
  (PIPE1), so it destroys its stage budgets when the line dies.
- An unlinked `spawn` survives, as shell.md states ("background work belongs in a Job").
- Nothing changes in how a line's own processes are counted. They are the session VM's, in the
  session's budget, so no budget is destroyed for them. Killing the evaluator is the whole
  mechanism for Elixir work, as shell.md says.

**Everything the stages started.** A stage gets no `budget` handle (PIPE1: its namespace and its
three streams only), so it can carve nothing and launch nothing. Its threads are its own process's.
Destroying the stage's budget therefore ends all it can start (R10, recursive anyway).
- The case still checks the side effects the kernel and piped clean up: piped's holders for the
  stage's connections go to 0, and the session's process and weight counts come back.

## 3. The interrupt's path

1. **The driver.** The session's own key (0x1C, which is also what sshd's INT and break become)
   is always found in raw bytes, before decoding: this is built for the editor and for screens.
   0x03 is found the same way unless a screen takes Ctrl+C as a key. With no line open and no
   screen in front, a line is evaluating, and the driver now:
   - draws `^C`, as at the prompt;
   - sends the interrupt to the evaluating shell.

   The feed changes the same way: an interrupt byte under an open feed interrupts the line, where
   today it is dropped. With a screen in front, nothing changes: the key ends the screen, and a
   second one then ends the line.
2. **Who to tell.** `Evaluator.eval` registers the evaluation with the driver: the shell's pid,
   and that a line is running, cleared when it returns. The driver is found the way `open_feed/0`
   already finds it. A VM with no driver (the host suite's group-less runs) registers nothing, and
   an interrupt there is dropped, as now.
3. **The shell** is the process blocked in `Evaluator.eval`'s receive. On `{:redoubt_interrupt}`
   it:
   - kills the evaluator with `:kill`;
   - waits, bounded (proposed: 2 s), for the line's foreground job owners to report that their
     budgets are destroyed;
   - prints "interrupted";
   - keeps the bindings from before the line, and reads the next line.

   The bounded wait makes "the next line runs with the session's processes back" deterministic.
   Without it, a pipeline typed straight after the interrupt can be refused `out_of_processes`
   while the old stages are still being destroyed. Owners report through the job registry
   (section 4). A late one is only late; the next prompt does not wait for it past the bound.
4. **Over SSH** nothing is new: INT and break are 0x1C at the console. One bench case sends a
   real `break` through the bench's SSH client to a session on the machine (section 6).

**What the interrupt cannot touch:**
- The session: its VM, and the driver, shell, piped and the job registry, which are not linked to
  the evaluator.
- Background jobs (section 5), whose owners do not watch the line.

## 4. `Job` and the registry

`Redoubt.Jobs` is a GenServer started at the first job, never at the prompt. It holds each job's
owner by number (`%Redoubt.Job{id, owner, command}`) until the job ends and its result is taken.
The API:
- `Job.status(job)`: `:running | :exited | :faulted | :killed`, with each stage's ending once known.
- `Job.kill(job)`: the owner destroys every stage budget, collects the notices (bounded, as
  `complete/3` does), lets the connections go and removes the pipes. It returns `:killed`, or the
  ending if the job had already ended. Killing one job touches only its own budgets: they are
  separate carves of the session's, and the owner holds no other job's.
- `Job.await(job, timeout \\ :infinity)`: `%{stdout:, endings:}`.
- `jobs()`: the commandlet listing the session's jobs, as id, command and state.

`pipe/1` and `exec` keep their behaviour: start a job in the foreground and await it. A foreground
job is listed while it runs. The interrupt ends it through its owner watching the line.

Pipeline's owner gains a `:kill` message and a `:status` call. Today it handles exit notices and
its caller's `DOWN`, so it already has a receive loop for these. Its existing `wait/4` and
`complete/3` are reused.

## 5. Background jobs in M2

**Recommended: yes, minimal.** `Job.start(stages, opts)`, or `pipe(words, background: true)`,
returns the `Job` at once.
- **The owner** does not watch the line, so the interrupt and the line's end leave the job running.
- **Input:** the job reads no console: its first stage gets the lines it is given, or the end of
  its input at once. The feed belongs to the foreground only, so a background job can never take
  typing.
- **Output:** the last stage's output and standard error are kept by the owner, bounded (proposed
  64 KiB each; past that the newest bytes are dropped and the count is kept). `Job.await` returns
  them. Nothing is drawn while the job runs, so a background job never writes over the line being
  edited.
- **End:** it ends by itself, by `Job.kill`, or with the session. Its budgets are carved from the
  session's, so the session's end destroys them (R10), and piped goes with the session's VM.

Why in M2:
- shell.md already says "background work belongs in a Job".
- The attack line "A job ends, and only it" needs two jobs at once to say "only it".
- `Job.kill` on a foreground job is reachable only from another process, which is a background
  question anyway.

The alternative is foreground only: `Job` as a handle the interrupt uses, `Job.kill` meaningful
only from a spawned process, and the "only it" half shown by an interrupted foreground pipeline
beside a second, Elixir-spawned one. It is smaller, but it leaves shell.md's sentence untrue and
no `jobs()`.

**Not in M2 either way:**
- `fg` and `bg`, i.e. moving a running job between foreground and background, and a job's own
  terminal. There are no process groups to move, and the feed is foreground-only by design.
- Suspending (Ctrl+Z): there are no signals and no stop state.

## 6. `follow(path)`

`follow(path)` prints the lines added to a file as they come, in the evaluating line, until the
interrupt.
- It polls the file's size (proposed: every 500 ms) and reads what was added. A 9P read past the
  end answers 0 at once, and no Redoubt file server notifies changes.
- It returns when the file shrinks or goes away (`:truncated` / `:removed`), or when the line is
  interrupted.
- It is pure Elixir in the line: the interrupt's kill ends it, and nothing is held after.

## 7. Cases (each `arch = ["rv64", "rv32"]`; verdicts the session's or the kernel's, rule F)

1. **`job-interrupt-line`** ("An interrupted line loses only itself"). The case types into the
   console session:
   - `x = 1`;
   - a line that loops forever in Elixir (`Stream.cycle([1]) |> Stream.run()`), then 0x03;
   - `x + 1`, then a line that loops again, then 0x1C;
   - `x + 1` again.

   Verdict: the session prints 2 both times, the prompt numbers go on, and the session still runs.
2. **`job-interrupt-native`** ("No program swallows the interrupt").
   - The case `exec`s a stage that reads its standard input forever. The open feed is the path
     that drops the key today. It types 0x03.
   - It runs a 3-stage pipeline of stages that spin on CPU (pipe-stage `spin`, new, a busy loop
     that never yields), and types 0x1C.
   - Verdict, the session's, after each: its process count and carved weight are back to before
     the line; `{error, not_running}` from `Pipes.usage`, since piped went with the last
     pipeline; the next line runs.
   - The spinning stages show the VM gets the CPU to take the key: weights 1 against the
     session's.
3. **`job-kill`** ("A job ends, and only it"):
   - start two background jobs: `yes | count-forever`, and a `gen` writing a known byte stream
     slowly;
   - `Job.kill(a)`;
   - then a foreground pipeline interrupted with 0x03 while b runs.

   Verdict, the session's:
   - a is `:killed` and its budgets are gone;
   - b is still `:running`, and after the interrupt `Job.await(b)` gives exactly its whole
     stream with `{exited, 0}`;
   - the carved weight is b's alone, then nothing.
4. **`job-interrupt-ssh`**: a steward session over SSH (the bench's client) runs the spinning
   pipeline and sends a channel `break`. The verdict is as in case 2, printed by the session.
   This is the one SSH case; the INT request is already covered by sshd-loopback-interrupt.
5. **`follow`**: `follow` of a home file while a background job appends three lines, then 0x03.
   Verdict: the three lines, then the prompt.
6. **Host:**
   - the driver's ExUnit tests: the interrupt with no line open reaches a registered evaluator,
     under a feed, and not under a screen that takes Ctrl+C;
   - `Evaluator`: an interrupt keeps the bindings and waits for the reported owners (bounded);
   - `Job`'s API on the fake kernel where the suite can launch;
   - `follow` against a fake file.

## 8. Footprint

Target: no page at the prompt.
- `Redoubt.Jobs`, `Redoubt.Job` and the `follow` commandlet load at first use.
- The driver's and evaluator's changes are a few clauses in modules loaded at the prompt (estimate
  under 1 KiB of code). The evaluator's registration is one message each way per line.
- Measured with bench:beamlet-footprint before and after.

## 9. Pages that move

- shell.md:
  - "Interrupting and killing jobs": planned to built;
  - "Session commands": `follow` built;
  - "Native programs and pipes": `Job`, and `Redoubt.Cmd` if ruled in (question 4).
- native.md, "Killing a job": built.
- m2-usable-shell.md: Progress, and step 4.
- The security register, if `Job.kill` adds a row. It should not: it adds no authority.

## Questions for the orchestrator

1. **Background jobs in M2**: yes, minimal as in section 5 (recommended), or foreground only?
2. **After an interrupt**, does the shell wait, bounded at 2 s, for the line's jobs' budgets to be
   destroyed before the next prompt? Recommended yes, for determinism.
3. **The interrupt under an open feed** (exec reading what is typed): it interrupts the line
   (recommended, as shell.md says), where today it is dropped. Confirm.
4. **`Redoubt.Cmd`**, the explicit builder shell.md shows (`new |> source |> pipe |> run`): in
   JOB1 as a thin builder over `Job.start` (about 60 lines), or later with the docs keeping it
   "not built"? Recommended: later. JOB1's explicit form is `Job.start(stages, input:,
   background:)`.
5. **The case list** in section 7, including one SSH-over-the-machine case (break) beside the
   built loopback one: acceptable?
6. **Tier**: the assignment says Tier A, so the red reviews. The code is Tier B shell code: it
   destroys only budgets the session carved, with the session's authority. Keep A for the
   interrupt's path, or B with the red as the one reviewer?
