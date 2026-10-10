# PIPE1 design checkpoint: native programs' standard streams and pipes

M2 'On Redoubt' step 3. Base: main 8fa39c5ad. No code written. Read: docs/plan/m2-usable-shell.md,
docs/userland/native.md (launching, standard streams and pipes, killing a job), docs/userland/shell.md
(the shell in a session, native programs and pipes, interrupting and killing jobs), docs/userland/beamlet.md
(Natives), docs/servers/serving.md (minted connections, parked calls, multiplexed connections, the 9P
skeleton), and the code they name (userland/shell/lib/redoubt/process.ex, userland/otp/redoubt/src/{files,
jobs,system}.rs, libs/client/src/ns.rs, libs/rt/src/server/ninep.rs).

**Two decisions are needed before code** (section 2): who serves a pipe, and the stream names. Both are
Open on native.md; the package is Tier A, so they are the Architect's. My recommendation departs from
native.md's recommended server (the session's VM) and from the plan node's wording ("files the session
serves"): a small Rust `piped` per session, which the session starts and owns.

## 1. What is already built

| Piece | Status | Cases |
| --- | --- | --- |
| Launch from a session: image, budget, namespace, handles, args from Elixir; end as `{exit, Job, Cause, Code}` | built (native.md "Launching from a session") | bench:beamlet-launch, bench:steward-vault-launch, host:beamlet-redoubt::a_launch_takes_what_it_is_given_and_its_end_is_an_event, host:beamlet-vm::a_launch_takes_everything_from_its_caller_and_its_end_arrives_as_a_message |
| The shell's `exec`: `Redoubt.Process.run/3` carves 256 pages, mints a console connection as the child's `/dev/cons`, waits, destroys the budget | built | bench:beamlet-launch |
| Budgets: `budget_create/1`, `budget_destroy/1`, `budget_usage/1` (carved from the VM's own `budget` handle only; no nesting from Elixir) | built (beamlet.md Natives) | host:beamlet-vm::budgets_are_carved_read_and_destroyed, bench:beamlet-natives |
| `serve/1`, `reply/2`: serve an endpoint handed to the VM; 5 s `REQUEST_WAIT_US`, `MAX_SERVED` 2; no native creates an endpoint or mints a badge | built | bench:beamlet-serve |
| `MAX_JOBS` 4 jobs at once, one watcher thread each (4-page stack), started lazily and reused | built (jobs.rs) | as launch |
| The VM's files: every 9P request through the hub, only the asking Erlang process waits; a waiting read holds no pool thread | built (files.rs) | bench:beamlet-files |
| `bind/2` replaces a binding at the same prefix; there is no unbind | built (ns.rs, system.rs) | host:beamlet-redoubt::a_bind_is_the_files_namespace_and_one_connection |
| Rust 9P skeleton: `Read::Wait`/`Write::Wait` parked (`serve_parking`), `FOREVER` deadlines, multiplexed connections, `Minted` connections rooted at a path (`new_connection(root, quota)`, never above the caller's root), `disconnected` hook, admission per (account, label set) with a share per badge | built (serving.md) | host:redoubt-rt::* (parked, minted, mux), fuzz:redoubt-rt/ninep_server, bench:aio-many-reads |
| Native programs in the bundle | none for users: only tests' programs (`beamlet-hello`, `beamlet-caller`, `echo-client`) | — |
| Standard streams, pipes, `pipe/1`, `Redoubt.Cmd`, `Job` | planned (native.md, shell.md: "Status: planned · M2") | none |

M1 residual this package closes: an `exec`'d program holds a console connection of its own and can read
typing the shell would have read, and its output reaches `/dev/cons` unguarded (no `Redoubt.Term.Text`).

## 2. The two Open choices

### Stream names: `/dev/stdin`, `/dev/stdout`, `/dev/stderr` (native.md's recommendation; agree)

Three namespace entries, each a connection rooted at one end of one pipe, so `startup.resolve("/dev/stdout")`
gives the connection and an empty rest. A stage's namespace is exactly these three plus what its launcher
names explicitly (`Cmd` options); no `/dev/cons`, no `/`. `/fd/N` buys nothing without descriptors.

### Who serves a pipe: recommended `piped`, a Rust 9P server per session (departs from native.md)

Serving pipes from the session's VM needs, on the VM's side, all of: a native that creates an endpoint and
one that mints badged send rights (neither exists); serve requests that wait without bound, where the
serve thread now ends any request at 5 s (`REQUEST_WAIT_US`; a reader waits on its writer for as long as
`sort` takes); more than `MAX_SERVED` 2; a 9P server codec in Elixir; and, since `redoubt-rt`'s client
reads through the hub, the multiplexed protocol (sends, completion calls, tags, flush) reimplemented in
Elixir. Every byte of every pipe would then go through the interpreter, and the codec would sit in the
VM's heap while a pipeline runs. Each of those is Tier A VM surface.

`piped` is the existing skeleton with one small `FileServer`: `Write::Wait`/`Read::Wait` parking, `FOREVER`
deadlines reclaimed by abandonment, mux, minted connections, admission and the conformance vectors are
built and attacked already. The one platform change it needs: **`launch/1` takes `serve: Name`**, the
platform makes an endpoint, hands the receive right to the child as the named handle `Name`, and answers
`{ok, Job, Connection}` with a send right the VM may `bind`. That is narrower than a general
`endpoint_create/0` (the VM never holds a receive right it does not serve; it already makes an endpoint
per launch for the exit notice). Its argument checks get the hostile-argument tests every native has.

Options for the Architect: (a) `piped` with `launch(serve:)` (recommended); (b) the VM serves, with the
natives and Elixir mux above; (c) `piped` started by the steward beside the console at login (costs every
session pages whether or not it pipes, and touches the steward: no).

## 3. The served-stream design (assuming `piped`)

**Lifetime and budget.** The session starts `piped` at its first pipeline (`launch` with `serve: "serve"`,
no namespace, no other handles), in a budget of its own carved from the session's (proposed: 64 pages,
1 process; the pipes' buffers are charged to it), and binds its connection at `/dev/pipe`. It lives for
the session; if it ends (exit notice), the next pipeline starts another. Nothing at the prompt.

**Files.** `/dev/pipe/<job>/<n>` is pipe `n` of job `<job>`; it holds two files, `w` (mode 0222) and `r`
(0444). The session's own connection, the root, creates a job's directory (`Tcreate`) and removes it
(`Tremove`) when the job ends; no other connection can create. A stage gets, for each stream, a connection
the session mints with `new_connection("<job>/<n>/r" | "<job>/<n>/w", 0)`: rooted at one end file, below
which there is nothing, so it reaches exactly that end. A read end refuses an open for write and a write
end an open for read (`not_permitted`). `new_connection` through a minted connection mints only below its
root, so it can reach nothing new.

**Who serves which stream.**
- `stdin`: the read end of the pipe before the stage. For the first stage: a pipe the session writes
  (`Cmd.source(path)`: the session reads the file and writes it in; `data |> pipe(...)`: the lines;
  neither, foreground: the lines the person types, edlin-cooked, Ctrl+D the end). Interactive input is
  native.md's decided rule: a pipe the session feeds from the console.
- `stdout`: the write end of the next pipe; for the last stage, a pipe the session reads, giving `pipe/1`
  its lines or, for a bare `exec`, drawing them to the console through `Redoubt.Term.Text`.
- `stderr`: one pipe per job whose write end every stage holds through a connection of its own; the
  session drains it while the job runs and draws it through `Redoubt.Term.Text`. So no native byte ever
  reaches `/dev/cons` but through the guard.

**Flow control.** Each pipe holds at most one page (4096 bytes) of piped's budget. A write takes what
fits, at least one byte, and answers the count taken (a 9P short write; `redoubt-rt`'s write loop
continues); a write with no room is `Write::Wait`, parked with `FOREVER`. A read answers what is buffered
up to its count; with nothing buffered and the write end held, `Read::Wait`. Each parked call holds one
`InFlight` of its badge's share (R28), so a stage can park at most its share. The session's own reads and
writes go through the hub and park the same: only the Erlang process that asked waits, no pool thread.

**End of stream.** A write end is gone when its holder's connection is disconnected (the launcher
disconnects a stage's connections at its exit notice, as `exec` does for the console today; piped's
`disconnected` hook) or, for an end the session holds, when it clunks its write fid. With the write end
gone and the buffer empty, a read answers 0 bytes, and every parked read is answered so. A clunk by a
stage is not the end (a stage that has not yet opened stdout must not end its reader's stream).
Residual: a stage cannot end its output before it exits.

**The read end gone.** When the read end's holder is disconnected (or the session clunks its read fid),
the buffer is dropped and every parked and later write is answered `Rerror` `state` (`enotconn`), the
table's name for a connection no longer there. A program then fails its write and, written well, exits.

**A stage that never reads.** Its upstream fills one page and parks; nothing else is held for it but that
parked call's admission and the page. The pipeline does not wait on it (next section).

## 4. Pipelines

**Joining.** `pipe(~w(a x | b | c))`: one job id; one budget per stage carved from the session's (default
256 pages, 1 process, weight 1; `Cmd` takes a spec per stage); pipes 0..n (0 the session's source when
there is one, n the session's sink) plus the stderr pipe; per stage, minted connections for its three
streams; then every stage is launched, last first, so each reader exists before its writer writes. Every
launch takes only what `Cmd` names: nothing is inherited, the session's own handles are never passed on.

**Back-pressure** is the parked write, end to end: `zcat` parks on pipe 1 until `sort` reads, and the
session's own source writer parks on pipe 0 the same.

**A stage exiting early.** Its exit notice comes to the job's owner (below), which disconnects its
connections: its reader sees end of stream, its writer gets `state`. **The job is complete when its last
stage ends**: then nothing more can reach the value, so the owner destroys every other stage's budget,
disconnects everything, removes the job's directory from piped, and returns the sink's lines with each
stage's ending. A middle stage that never reads, or never exits, cannot hold the pipeline past its last
stage. A last stage that never ends holds the line: that is the interrupt's (JOB1).

**The job's owner.** One Erlang process per job holds the budgets, the minted connection ids and the
pipes, receives the exit notices (a launch's end goes to the process that launched, so it launches), and
traps exits from the evaluating process: if the line's evaluation dies (the host-built interrupt ending a
line, or a crash), it destroys every stage's budget and cleans up. So PIPE1 leaves no orphan stage, and
JOB1's `Job.kill` and Ctrl+C are calls into it.

**`exec` moves onto this.** `exec(name, args)` becomes a one-stage pipeline whose stdin is the console
feed and whose stdout and stderr are drawn through the guard: no stage gets `/dev/cons` any more. This
changes `beamlet-hello` (it writes `/dev/stdout`) and the lines bench:beamlet-launch and
bench:steward-vault-launch expect (drawn by the session now).

**Limits.** `MAX_JOBS` (4) counts every running program, piped among them: a 3-stage pipeline would be the
most. Proposed: `MAX_JOBS` 16 (threads stay lazy, 4 pages each only when used) and a pipeline cap of
`MAX_JOBS - 1` stages, refused by name (`too_many`) before any launch.

## 5. What JOB1 then needs

- One budget per stage: delivered by PIPE1 (carved from the session's; Elixir cannot nest budgets, so a
  job's kill is one `budget_destroy` per stage; a stage has 1 process and no `budget` handle, so it can
  carve nothing that outlives the loop).
- The job owner of section 4: JOB1 adds `Job` (`kill`, `status` from the exit notices, `await`), the
  interrupt from the driver (Ctrl+C and 0x1C over the console; sshd already maps `signal`/`break` to 0x1C)
  sent to the foreground job's owner, and `follow()` stopping on it.
- No stage holds the console: delivered by PIPE1 (stdin is the session's feed), so JOB1's "no program
  swallows the interrupt" case can be written without further plumbing.

## 6. Attack and behaviour cases (each `arch = ["rv64", "rv32"]`)

Verdicts come from the session or the kernel (rule F). A hostile stage has no `/dev/cons`, so it cannot
print a verdict-shaped line: everything it writes reaches the console only through the session's guard.

1. **`pipe-no-authority`** (the M2 attack "a pipe carries no authority"). A hostile stage between two honest
   ones, while a second job's pipe holds a canary, tries: `resolve` of any name but its three streams;
   `..` and longer walks from each stream's root; open stdin for write and stdout for read; read its stderr;
   `new_connection` through each stream naming `..`, another pipe and the canary job; send on its
   connections with forged words. Verdict, by the session: the honest stage after it received exactly the
   bytes the hostile stage wrote to stdout, the canary pipe holds exactly its own bytes, the canary job's
   reader saw no foreign bytes, and every stage budget is empty after the job. The hostile stage's own
   report of each refusal reaches the session through its stdout and is printed as evidence only, never
   judged. The namespace is the launcher's: the session prints the entries it passed.
2. **`pipe-end-of-stream`**: a writer that exits after N bytes, the reader sees N bytes then 0; a reader
   that exits first, its writer's next write answered `state`; a three-stage pipeline's value is the
   sink's lines. Verdict: the session's.
3. **`pipe-never-reads`**: a producer writing forever into a stage that never reads, then a last stage that
   exits at once. Verdict: `pipe/1` returns; `budget_usage` on the session's budget shows every stage
   budget gone and the session's pages back to what they were before the line.
4. **`pipe-hostile-output`**: a stage writes OSC 52, OSC 8, a title report and a bare ESC to stdout and
   stderr, and `exec` of it. Verdict: the bytes the session wrote to `/dev/cons`, as in the existing hostile
   text cases.
5. **`pipe-interrupted-line`** (orphans): a line running a pipeline whose stages never end is ended by the
   host-built interrupt path (or a crashing evaluation); verdict: every stage budget destroyed, the session
   running, the next line's pipeline works.
6. Host tests for piped's `FileServer` on the runtime's fake kernel: end selection, short writes, wait and
   end of stream, the read end gone, a minted connection that cannot mint above itself, admission per share
   under a flood of parked writes; the 9P conformance vectors run against piped (`r4-host-tests` list).
7. Native argument tests for `launch(serve:)` (wrong types `badarg`, a name past `MAX_NAME`, with
   `MAX_START_HANDLES` counting it).

Labels: piped and every stage are carved from the session's budget, so all carry the session's labels; a
pipe between label sets cannot be built by one session, so R1's half of native.md is argued, not attacked.

## 7. Prompt footprint

Target: no page at the prompt (margin ~36 of 11,008).
- Elixir: `Redoubt.Cmd`, the job owner and the `pipe`/`exec` changes load when first called; nothing new
  at start. `Redoubt.Process` stays the same size or shrinks (the console mint goes).
- VM Rust: `launch(serve:)` and the `MAX_JOBS` constant: an estimated few hundred bytes of the beamlet image,
  which every session copies (no shared text), so at most 1 page; job threads remain lazy.
- piped: its own budget, carved only at the first pipeline: its image (estimate 25-40 pages copied) plus
  stack, heap and buffers, inside the proposed 64 pages; then each stage's 256.
- Measured, not assumed: bench:beamlet-footprint before and after (the bench's memory scan is the verdict),
  and the session's `budget_usage` after the first pipeline reported in the package's results.

## 8. Pages that move

native.md (standard streams and pipes: planned to built, both Open items out, the figure's server; the M1
paragraph replaced; killing a job unchanged), shell.md (the shell in a session's `exec`; native programs and
pipes: status, Open out), beamlet.md (Natives: `launch(serve:)`, `MAX_JOBS`), a new docs/servers/piped.md
(or a section on an existing page, the Architect's call), m2-usable-shell.md Progress, the security
register if piped adds a row, and README/GETTING-STARTED claims about launching. Hotspot: `docs/` has one
writer at a time.

## Questions for the orchestrator / Architect

1. Who serves a pipe: `piped` with `launch(serve:)` (recommended), or the VM (needs endpoint and mint
   natives, unbounded serve waits, Elixir mux)?
2. Stream names `/dev/stdin|stdout|stderr`: confirm.
3. `exec` losing `/dev/cons` for the console feed in PIPE1 (changes two bench cases' expected lines), or
   keep it until JOB1?
4. End of stream at disconnect only (a stage's clunk is not the end): acceptable residual?
5. `MAX_JOBS` 4 to 16 and a pipeline cap of `MAX_JOBS - 1`: acceptable?
