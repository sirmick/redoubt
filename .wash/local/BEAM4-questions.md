# BEAM4: four design questions before serve, labels, budgets and launch

Read: the brief, beamlet.md Natives, native.md, wire.md, sessions.md, budgets.md, objects.md Mint,
serving.md, processes.md "Exit notices", libs/client aio/launch/ns, libs/rt server, the steward on
wp-STEWARD2. Each question names the page, the rule, the options and the one I recommend. I go on
meanwhile with what none of them touches: the `System` trait, `ns_lookup/bind/ns`, handles as
resources, the generated Elixir clients.

## Q1 `labels/0`: `budget_usage` carries no labels, and a VM under init holds no budget

budgets.md "`budget_usage`": six slots (`USAGE_SLOTS`), page/process/weight limits and usage; no
label set. init.md: "No server gets a budget handle", so beamlet under init holds none (lib.rs
`BUDGET_PAGES` says the same). The brief's "read once at start from its budget (`budget_usage` on its
own handle)" cannot be built.

- (a) **Recommended.** Read it from the kernel's stamp: every message carries its sender budget's
  label set (ipc.md; `Caller::labels`). At start the VM sends one word to its own wake endpoint
  through a badge it mints for itself, and takes the labels off the delivery. Kernel truth, no ABI
  change, works under init and the steward alike. Fixed thereafter.
- (b) Add the labels to `budget_usage`'s record: a kernel/ABI change (K23 owns libs/sys and the ABI
  page now), and still needs a budget handle.
- (c) A launcher argument: forgeable by a launcher, refused.

## Q2 Where the VM's budget handle comes from (budget natives, `launch/1`, the shell's commandlet)

The steward on wp-STEWARD2 hands a session its own budget as the named handle `budget` (steward.rs
`launch.handle("budget", budget)`), as sessions.md's figure shows. init grants none and refuses a
manifest that does. So under init `budget_create/1` and `launch/1` have no parent budget, and the
brief's "beamlet-natives (a VM under init ...): budget_create a child with a deadline" and
`beamlet-launch` cannot run as written.

- (a) **Recommended.** The natives take the parent budget from the named handle `budget`
  (`labels/0` does not need it, Q1a); with none, `budget_create` and `launch` are refused
  `no_budget`. The machine cases run beamlet under a small tester in init's place (as
  `launcher-orphan`'s), which starts the servers it needs and launches beamlet with a carved budget
  handed as `budget`, the steward's shape. `budget_create/1`'s first argument is then the parent
  budget resource (the one `Redoubt.Budget.own/0` returns), so a child budget can carve again.
- (b) Wait for STEWARD2 and run the cases as steward sessions (login, sshd or the UART session):
  heavier cases, and couples BEAM4's gates to STEWARD2's merge.
- (c) Let init hand beamlet a budget handle: departs from init.md's rule; refused.

## Q3 `call/3` "through the hub": the hub carries only 9P

aio.rs `Hub::submit` takes a 9P `Body`; every typed protocol (keyd, ninep_common's `new_connection`,
littlefsd's ops) is an IPC `call` with words, a lend and handles, which no hub carries. So every
`call/3` a generated client makes is a typed call.

- (a) **Recommended.** `call/3` is always the blocking call on the VM thread, with a bounded timeout
  (`CALL_TIMEOUT_US`, 5 s, stated on the page), the result sent at once as `{:reply, ref, result}`
  so the Elixir shape is the asynchronous one the page gives and a hub path can come later without
  changing a caller. beamlet.md "Natives" names it: every typed call blocks the scheduler, as
  `rename` and the console's size do.
- (b) A thread per outstanding call: no scheduler stall, but a thread per request, which the hub
  exists to avoid; bounded at a few.
- (c) `call/3` also takes a raw 9P T-message on a multiplexed connection, through the hub: nothing in
  BEAM4 needs it.

## Q4 `serve/1`: the serving library's multiplexed loop is the 9P skeleton

serving.md "Multiplexed connections" and R77 are `ninep_mux` under `NineServer`: 9P T-messages over
the completion call. A typed endpoint (what a second program `call`s, the brief's beamlet-serve) is
served by `serve`/`typed`, with `admit` (R26) and `parked` (R28) for held calls; there is no typed
multiplexed loop. Requests must reach the VM while it idles on its wake endpoint, and an open call
can be answered only by the thread that holds it.

- (a) **Recommended.** `serve/1` starts one serve thread for the endpoint (receive right held by the
  VM): it admits each call by the library's `Admission` per (account, labels) with a share per badge
  (R26), holds it as a `parked` call with the library's deadline (R28: an unanswered request is
  answered `timeout` and its admission released, never held by the VM), and hands the VM `{:request,
  ref, badge, account, labels, {words, buffer, handles}}` through a wake-up; `reply/2` goes back to the
  thread by a send on a badge the VM minted for itself on that endpoint. A `send` to the endpoint is
  delivered as `{:request, ref, ..}` with `ref` nil. R77 is not claimed (no 9P); R26 and R28 are.
- (b) `serve/1` serves 9P through `NineServer` with the Erlang side as the file system (T-message
  bodies up, R-messages back): R77 holds as written, but needs a 9P server codec in Elixir, which is
  what pipes need (native.md, M2), not BEAM4.

## Also (no answer needed unless you disagree)

- The job's exit notice: each job has its own exit endpoint (native.md: no PID to tell notices
  apart), so each running job has one thread receiving its notice and waking the VM, as a hub waiter
  does; at most `MAX_JOBS` (4) at once, `too_many` past it.
- `launch/1` needs the loader stub's bytes as well as the image's: read from `/boot` as the image is.
  STEWARD2 adds `Launch::streamed` to libs/client; I build on `Launch::new` and read both whole
  (bounded by the VM's budget), unless you want the streamed path from STEWARD2's branch.
- The natives' Erlang-facing glue (argument checks and term building) goes in `vm/src/bif/system.rs`
  beside `file.rs` and `port.rs`, over the `System` trait in platform.rs; everything that touches the
  kernel, the hub or the client library is beamlet-redoubt's. That is how I read "registration
  only".
