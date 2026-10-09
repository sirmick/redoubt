# piped

`piped` serves one session's pipes. A session starts it when a pipeline needs it and none is
running, in a budget carved from its own, and destroys that budget when its last pipeline ends, so
its pages are the session's again between pipelines. Through the one connection
it was given the session makes a directory per pipe, holding its two ends, and mints each native
stage a connection rooted at exactly one end: that stage's standard input, output or error
([native programs](../userland/native.md#standard-input-and-output-and-pipes)).

## Purpose

Redoubt has no pipe object, no file-descriptor table and no inheritance, so a pipe is a 9P file
somebody serves. A stage reads its standard input as a file and writes its output as one, and the
server of those files decides what a write does when nobody has read yet and what a read sees when
the writer is gone. The session's VM could serve them only through new natives that make endpoints
and mint badges, with a serve thread that ends every request at 5 s while a pipe's reader waits on
its writer for as long as the writer takes, and with the multiplexed protocol and a 9P server
written again in Elixir, every byte then going through the interpreter. `piped` is the
[9P server skeleton](serving.md#the-9p-server-skeleton) with one small file server, so all of that
is already built and attacked.

## Interface

### Serving pipes

<details><summary>Status: built · tested (16)</summary>

- bench:piped-build
- bench:piped-host-tests
- host:redoubt-piped::the_limits_a_session_passes_fit_the_budget_and_admission_takes_them
- host:redoubt-piped::a_pipe_carries_one_stages_bytes_to_the_next_and_ends_when_its_writer_goes
- host:redoubt-piped::a_full_pipe_holds_its_writer_until_the_reader_takes_some
- host:redoubt-piped::a_writer_whose_reader_has_gone_is_refused_even_while_it_waits
- host:redoubt-piped::a_multiplexed_read_of_the_sessions_waits_for_a_stage_to_write
- host:redoubt-piped::a_write_takes_what_fits_and_waits_when_nothing_does
- host:redoubt-piped::a_read_waits_until_the_write_end_goes_then_reads_the_rest_and_the_end
- host:redoubt-piped::a_write_is_refused_once_the_read_end_goes_and_what_was_buffered_goes_with_it
- host:redoubt-piped::the_sessions_open_fid_holds_an_end_until_its_own_clunk
- host:redoubt-piped::a_connection_is_minted_only_at_one_end_and_holds_it
- host:redoubt-piped::the_tree_is_the_root_the_pipes_and_their_two_ends
- host:redoubt-piped::pipes_are_bounded
- host:redoubt-piped::the_conformance_vectors_run_against_piped
- host:redoubt-piped::only_the_sessions_own_badge_attaches_and_its_labels_are_the_pipes

</details>

`piped` is a 9P server over the skeleton, and its endpoint also serves `ninep_common`
([wire](wire.md#ninep_common)); it serves calls and multiplexed requests alike
([serving](serving.md#multiplexed-connections)).

- **The tree.** The root holds one directory per pipe, named by the session; each holds `r`, the
  read end, and `w`, the write end. At most `MAX_PIPES` (32) pipes at once: the longest pipeline
  a session can run needs one more than its stages, and one for their standard error.
- **An end opens only its own way.** `r` opens for reading, `w` for writing; any other mode, and
  `OTRUNC`, is `not_permitted`.
- **A pipe is a buffer of one page** (`PIPE_BYTES`). A write takes what fits, at least a byte, and
  answers how much it took, so a writer's loop goes on; while nothing fits, the write waits. A read
  takes what is buffered, up to its count, and waits while nothing is. The offset means nothing: a
  pipe is read in order.
- **Waiting is parking.** A call that waits is parked ([serving](serving.md#parked-calls)), and a
  multiplexed request waits in the skeleton, both with no deadline: a reader waits on its writer for
  as long as the writer takes. What reclaims one is its caller giving up or dying, which arrives as
  an abandoned-call notice. After any call that moves a pipe (bytes in or out, an end let go, a
  pipe removed) every waiting call is served again, longest wait first.
- **Holding an end.** A connection minted at an end holds it until it is disconnected; a fid of the
  session's open on an end holds it until that fid is clunked. A stage's own clunk lets go of
  nothing: its connection holds the end until its launcher disconnects it, at its exit notice.
- **The end of the stream.** Once a write end has been held and is let go, a read takes what is
  left and then reads 0 bytes, and so does every read waiting then. Before the write end is first
  held a read waits, so the order the session starts its stages in does not matter.
- **The read end gone.** Once a read end has been held and is let go, what was buffered is dropped,
  and every write, waiting or new, is refused `state` (`enotconn`): the stream has nowhere to go.
  Before the read end is first held a write is taken, up to the buffer.
- **A pipe removed** answers every call on it `removed`; a fid left on it reaches no later pipe,
  since a pipe is named by an id never reused.

### Started by a session

<details><summary>Status: built · tested (5)</summary>

- bench:pipe-carries
- bench:pipe-eight
- bench:pipe-no-authority
- host:redoubt-piped::only_the_sessions_own_badge_makes_and_removes_pipes
- host:redoubt-piped::a_stage_reaches_only_the_end_it_was_given

</details>

A session starts `piped` with `launch` and `serve => "serve"`
([beamlet](../userland/beamlet.md#natives)): the platform gives `piped` the endpoint's receive right
and the session the one connection nobody minted, badge 1 (`ROOT_BADGE`). Only that badge attaches,
at the root and with no name; only it makes and removes pipes, and only in the root; any other badge
below the minted range is refused. The session mints each stage's connections through it with
`new_connection`, at `NAME/r` or `NAME/w`; a mint at the root or at a pipe's directory is refused,
so every connection the session hands out is rooted at a file. A connection rooted at a file
reaches nothing else: no walk leaves a file, `..` at the root is the root, and a `new_connection`
through it mints at its own root or not at all.

Its arguments are `buckets=N`, as every shared server's are, and a session passes 2, the fewest
admission allows.

- **Admission** ([R26 (admission fairness)](serving.md#r26-admission-fairness)). A session's stages
  carry its account and labels ([R8 (accounts)](../kernel/budgets.md#r8-accounts)), and the session
  minted their connections through its own, so the session and every stage are one client: one
  bucket, and one share in it, which may take half the bucket. A bucket's caps are 16 parked
  calls, 128 fids, 64 connections, 64 multiplexed requests and 4 pages they brought, so the share
  holds 8 parked calls: 7 stages each parked on one stream, a stage reading or writing one at a
  time, and the session's completion call. A parked call holds its caller's lend, 64 KiB at
  worst, so two buckets at their caps are 2,260,992 bytes, which the program checks against its
  allowance (`BUDGET`) before it serves; the buffers, 32 pages at most, are beside it. The session
  carves `piped` 768 pages for all of it: the lends at worst are 552 pages, the buffers 32, and
  its image, stack, heap and records the rest; 512 would not hold the two buckets admission needs
  at the least. After `pipe-carries`' pipelines it holds 86 pages on rv64 and 83 on rv32: the
  rest of the carve is what its clients' parked calls may lend it at worst. The carve is held
  only while a pipeline runs: `Redoubt.Pipes` destroys it when the last process holding `piped`
  lets go or ends, and starts another at the next pipeline, which `pipe-carries` times at about
  60 ms under QEMU (about 200 ms for the session's first, which loads the shell's modules too).
  No budget of `piped`'s outlives its pipeline: after each of eight pipelines in one session the
  session's budget holds what it held before them, processes, weight and all (`pipe-eight`).
- **Labels.** `piped` runs in a budget carved from the session's, so it carries the session's
  labels, and the kernel lets no other set's message reach a server with no exemption
  ([R1 (flow)](../kernel/ipc.md#r1-flow)): its files carry the label set of the session's first
  attach, which is every caller's.

## Authority

Status: built · tested: host:redoubt-piped::a_stage_reaches_only_the_end_it_was_given

`piped` holds its endpoint and the connections it minted, and the bytes buffered in its pipes. It
holds no other handle, makes no calls and reads no file. A stage holding one of its connections can
read or write one end of one pipe and nothing else; the session can make, remove and read or write
every pipe of its own, and no other session's, since each session starts its own `piped`.

## Security properties

Status: built · tested: host:redoubt-piped::a_stage_reaches_only_the_end_it_was_given, host:redoubt-piped::a_connection_is_minted_only_at_one_end_and_holds_it, host:redoubt-piped::the_conformance_vectors_run_against_piped

`piped` claims no rule of its own. It is how a pipe carries no authority
([native programs](../userland/native.md#standard-input-and-output-and-pipes)): every connection
it lets the session hand out is rooted at one end file, and the serving library's walk and mint
rules ([the 9P server skeleton](serving.md#the-9p-server-skeleton)) keep a connection below its
root. It keeps the serving library's R25 (the label check), R26 (admission fairness) and
[R28 (parked-call accounting)](serving.md#r28-parked-call-accounting), each stated on its owning
page.

## Failure and restart

Status: built · partly tested: a `piped` that ends while a pipeline runs is argued, not attacked

- **No `buckets=N`, or one its allowance cannot hold**, and `piped` exits before it serves.
- **`piped` ends** (its budget destroyed, a fault): its endpoint dies, and every stage's call on it
  fails, as a read or write error; the session sees the exit notice, and starts another `piped` at
  its next pipeline. The session destroys its budget itself when no pipeline holds it
  (bench:pipe-carries, bench:pipe-never-reads, bench:pipe-interrupted).
- **A connection whose reply is lost** is rolled back ([replies and rollback](serving.md#replies-and-rollback)).

## Residual risks

- **A stage cannot end its output before it exits.** Its clunk is not the end of the stream, so a
  stage that closes its standard output and runs on leaves its reader waiting until it exits. A
  stage that has not yet opened its output must not end its reader's stream, and the two cannot be
  told apart without a rule the stages would have to keep.
- **The stages of one session share one share.** The session and its stages are one client
  ([admission](serving.md#r26-admission-fairness)), so a stage that parks calls on many connections
  of its own can fill the share and make another stage's wait, or the session's, refused
  `too_many`. Every stage is the same session's, with the same authority.

## Why

- **One server per session.** A shared `piped` would hold several principals' bytes in one process
  and need the label exemption a system server has; one per session, in the session's own budget,
  is a user-level server the kernel's flow check already confines.
- **A connection per end, not a file per stage.** A stage given a directory could open the other
  end, or another pipe; a connection rooted at one end file cannot, whatever it sends.
- **Short writes.** A write that waited for room for all its bytes would hold a page-sized buffer
  hostage to one large write; taking what fits keeps every write's wait to the reader's next read.
