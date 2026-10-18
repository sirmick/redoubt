# Architect rulings, 2026-10-18 wave (architect-5)

Brief: `.wash/local/architect-wave-2026-10-18.md`. Plan file: `.wash/local/plan-2026-10-18-wave.toml`.
Owner questions: `.wash/local/owner-questions-2026-10-18.md`. Each node below has a brief at
`.wash/local/<ID>-implementer.md`. Territory is said per ruling (SWARM "The Architect").

## The provisional rulings: all confirmed

- **STEWARD3V Q1–Q3 (a):** the server subtracts the launched stack's bound (128) since a program
  cannot read its own stack; the derivation form on walfsd.md names it; an uncapped heap counts 0;
  consoled keeps 3Z's constant. 3Q's rule (an admission constant is derived from the manifest, the
  derivation on the page, held by a bench host test) is kept whole; a wire field (b) would buy
  exactness of at most 128 pages per entry, a wire move for nothing an entry needs. Confirmed.
- **STEWARD3V Q4 (a):** measured N = 4, recorded the N = 16 bound. Confirmed; the lease term is
  STEWARD7 below.
- **STEWARD4 Q17** (as the orchestrator ruled it on the thread): confirmed.
- **B82's scope** (a kernel case pins what a deadline does between create and start; no kernel
  change): right: the kernel did what R10 says; the steward's deadline from the batch's start is
  the stated rule (a lease's deadline is from its request). Confirmed.
- **B83 option A** (the bench's `ended` step): confirmed; the rule on testbench.md is the right
  owner of it.
- **STEWARD3U (b)** (a call's lines drained before its reply, the rest at the turn): confirmed;
  the bench's order of records is a claim the pages make (`Login` before `sshd`'s line), so the
  pre-reply drain is the rule, not a convenience. Its cost is measured (6.5 ms p50 at one hart).
- **B88's scope and fix:** confirmed; the rv32 two-hart N = 3 and 4 rows stated missed with the
  cause named; the target unmoved. The two residuals it names are STEWARD6 and STEWARD8.

Nothing overturned, so no new node arises from this section.

## F1. The loop's bound inside a timer-wake window → STEWARD5

**Finding.** 3Y put every cleanup step at the loop's top within the steward's send bound
(100 ms); 3W made a step persist. The targets the loop is measured against are 15 ms p50 and
50 ms p99 for its timer wake. One attempt at a busy server, landing in a sample's window, is a
100 ms wait before the receive: the p99 is missed by construction, one sample per round at
N = 3 and 4 on both widths at one hart (69–93 ms) and at two on rv64 (148 ms). The number is
not a tuning: 3W ruled the bound stays unless a measurement shows it too short; this
measurement shows it too long for the targets, which is the other way to move it.

**Options weighed.**
- (a) A bound derived from the targets (the p99 less the wake's own worst, about 25 ms). A
  25 ms wait in a window still gives samples of 30–50 ms against a 15 ms p50; a server's own wake
  at two harts is itself up to 25 ms, so an idle server would time out and be retried, each
  retry a wait. The bound would be short enough to hurt and long enough to miss.
- (c) Non-blocking attempts with completion as an event. A `disconnect` is a 9P call: a reply
  the caller must take; making the client library's calls asynchronous is a new client shape
  for one caller.
- **(b) The loop never waits on a shared server (chosen).** The cleanup steps run on a thread of
  the steward's, as its crossing watchers and relay hands already do (`work.receive` threads in
  `bin/steward.rs`); the loop's top does the idle timer's ends, the console lines (3U) and the
  receive, and makes no call to a shared server. The rule 3R began (the destroy is the cutoff;
  cleanup only frees records) ends here: cleanup is not the loop's at all.

**The mechanism (mine: inside the steward's loop; no wire, kernel, target or page claim moves).**
1. A **cleanup thread**, started with the steward, owns `cleanup::Queue` (due, retries, parked
   steps: 3W's rule unchanged). The loop hands it each step as its owner's end releases it
   (`disconnect(id)` at the server that minted it, `ended` on `sshd`'s channel console), and
   `wake(server)` when a mint of the loop's at that server was answered, `forget(server)` on
   `reconnected`. The thread runs each step with the client library's own call bound
   (`RELEASE_TIMEOUT`, 1 s): the loop pays nothing, so the step may wait what a call waits.
   `RETRY_US` and `TRIES` stay; a parked step still has no timer.
2. **Outcomes come back as events.** The thread tells the loop a step's outcome (done, parked
   with its attempt count, failed `Dead`/refused) as a message on the loop's own endpoint; the
   loop says the console line (3U's queue), so every line still goes through the one fid in order.
3. **A mint at a server with parked steps** (3W's rule: the mint needs the record a step frees):
   the loop sends `wake(server)` and mints; if the mint is refused and that server had parked
   steps, the loop waits for the thread's report of them within one mint bound (STEWARD6's) and
   retries the mint once. So a mint still pays at most one bound, a batch's rule; 3W's test (a
   mint succeeds where it was refused) is kept. A mint answered (taken or refused) still wakes
   the server's parked steps.
4. **Two threads on one endpoint.** The thread's `disconnect` and the loop's `new_connection`
   may be in flight at once on one kept connection: each is a kernel call with its own lent page
   and reply; the server admits them in the steward's share (`in_flight`, 6 in the image: one more
   in flight than today, stated on walfsd.md's derivation if the number is touched). The thread
   has its own `Lend` page and stack; the implementer shows the client's `Connection` makes no
   assumption of one caller (no shared tag or fid state across calls on an unattached connection).
5. **What it costs:** one thread (stack pages measured as sshd's are: twice the measured peak,
   stated), one lend page, no new connection or State at any server. The steward's entry grows
   by the arithmetic; the image and every memory-checked manifest by the same.

**What stays.** The console queue drains at the loop's turn within `CONSOLE_US` (3U): consoled is
the UART's server and the measured cost is 6.5 ms per reply with no drop seen; a console write
landing in a window would be a finding of its own, measured first.

**Pages.** steward.md "Failure and restart": the cleanup bullet rewritten (the thread, the
outcomes, the order of the loop's turn without the queue's turn), its status line; serving.md's
residual unchanged; scheduling.md "With the real steward": the N = 3 and 4 timer-wake cells
re-measured, the sentence attributing the misses to the cleanup turn replaced by the measured
figures. SECURITY.md R41's row if its tests change.

**Cases.** `steward-latency` N = 3 and 4 at one hart meet the timer wake's p99 on both widths,
and the reply-to-receive p99 carries no 100 ms retry (the acceptance); the steward set and
`steward-server-restart` unchanged in verdict. Host: the loop's turn completes with no call out
while a server never answers (the fake kernel's trace of calls); the five 3W tests kept; the
outcome lines said in the order the steps ended.

Tier A, size M, reviewer steward-red-4; after STEWARD6 (same file).

## F2. N = 16: a principal's leases from the manifest → STEWARD7

**Finding.** 3Q's per-account caps add a lease term of 1 (`LEASES` in the bench's host test,
"no manifest names it"); one session with N leases holds 2 + N at `erofsd:system` and 1 + N at
its home and `/boot`, so the caps (11, 8) admit N <= 7.

**Ruling (mine: an admission derivation, 3Q's rule applied; no manifest field, wire or target
moves).** The lease term is derived as the context cap is (R82: "what the label set's share of
the principal's budget holds"): a principal's live leases are bounded by what its unlabelled
share holds in `agent` budgets, pages and processes both, the smaller:
`leases(p) = min(floor(share_pages / sizes.agent.pages), floor(share_processes / sizes.agent.processes))`
with `share` the unlabelled sub-budget as `init`'s `holds` derives it. It is an upper bound: the
sessions carved from the same share only lower what is left for leases, and a sponsored
principal's sessions are leases of that share too. The kernel's carve (R7) is what refuses the
lease past it, so the steward gains no rule and the core no row; steward.md "Leases" states
the bound as arithmetic (image: alice 40,959 / 5,248 → 7, processes 32 / 2 → 16: 7). The bench
host test replaces its constant with the derived term, per principal, largest over the
manifests; the servers' `limits` take the term; the entries follow 3V's rule where a bucket grows.

Not (b) (a constant raised to what a case needs: a case's N in a compiled constant, which 3Q
forbade) and not a `leases=N` manifest field (a new input where a derivation exists).

**The latency case.** N = 16 is reached when the latency manifest's alice affords it by the
rule: 16 leases of 5,248 pages beside her console session's 11,392 → an unlabelled share of at
least 95,360 pages, doubled by her two label sets: about 190,722 pages (745 MiB) under the users
budget; the memory rule's arithmetic on the toml decides whether `memory_mib` 1536 holds that
beside the five other principals, or is raised with the number. If the machine the owner
affords cannot, the largest N the rule gives is recorded with its arithmetic beside the
stand-in's column at 16 (an owner question in the file, conditional). The round's cost: 15
spinner launches and 32 probe leases; D shrinks first if the run exceeds its class (Q9's rule).

Tier A (admission caps in every shared server; the bench's derivation), size S–M, reviewer
steward-red-4; in parallel with STEWARD5/6 (paths disjoint but the latency toml and
scheduling.md's table: the last to land re-measures the table).

### F2 Q5 (STEWARD7, thread STEWARD3V-budgets): `in_flight`'s lease term meets R4a's headroom

The implementer is right that the console + cap + leases + 1 sum, applied to `in_flight`
(walfsd/littlefsd: one parked completion call per VM connection at the home, a 64 KiB lend
each), puts six-principals' `walfsd:data` at 10 × 20 = 200 open calls, over `MAX_OPEN_CALLS −
OPEN_CALL_HEADROOM` = 192, and N = 16's manifest at 10 × 23; memory is not the wall.

**Ruled: (c).** `in_flight` counts parked completion calls, one per live VM of the account at
that server, and sessions and leases are carved from one share, so the sum console + cap +
leases double-counts: the joint bound is what the share carves at once in VMs,
`floor(share_pages / min(sizes.session.pages, sizes.agent.pages)) + 1` (the spare 3Q gives
every derivation; processes could only tighten it, so pages alone is the stated upper bound).
`state` keeps F2's sum (cheap, conservative: a session holds two connections at `erofsd`).
Image: alice 40,959 / 5,248 → 7 + 1 = 8 (now 6); latency's alice at N = 16: 95,361 / 5,248 →
18 + 1 = 19, 10 buckets × 19 = 190 <= 192; six-principals unchanged. A new bench assertion
guards R4a across the tree: for every manifest and every server, the sum of `in_flight` over
its buckets (defaults and overrides) is at most `MAX_OPEN_CALLS − OPEN_CALL_HEADROOM`, so a
manifest past it fails the bench, never `Unsized` at boot. Not (a) (records N = 12 for a wall
that is the sum's double-counting, not the kernel's) and not (b) (a manifest tuned to a case).
Pages: walfsd.md's and littlefsd.md's derivation (the `in_flight` form and the image's
arithmetic), serving.md "`admit`" one clause, ipc.md R4a unchanged (cite its headroom).

### F2 Q6 (STEWARD7): weight is the third carve dimension; the latency alice's weight

Accepted both. (i) The term takes every dimension R7 carves: leases(p) = min(share_pages /
agent.pages, share_processes / agent.processes, share_weight / agent.weight), share_weight the
budget's weight over its label sets as init's `over` reads it; the image's numbers do not move
(alice 500 / 50 = 10 → still 7; bob still 6); in_flight's pages form stands (pages are the
binding dimension for a VM's parked call; weight could only tighten, and the bound is stated as
an upper bound). (ii) The latency manifest's alice gets weight 2,000 beside pages 190,756 and
processes 96: her top budget and sub-budgets run no thread, and the one flat stride queue uses
the free weight of budgets with runnable threads, so the workload's weights (spinners 50, the
session 100, the steward 1,000) and the stand-in comparison are unchanged; only the carve room
grows; `users` is raised by the same so init's `over` check holds, the arithmetic in the toml's
comment. The per-budget page (`sizes.cost`, BUDGET_PAGES) belongs in the arithmetic as the
implementer states it. The caveat is right and is STEWARD8's: a launch beside 15 spinners of its
own weight may outrun the shell's 5 s bound at one hart; if so the N = 16 row says so with the
AgentStarted gaps, a scheduler finding, not admission's. Owner Q3 (memory for N = 16) is moot:
memory and the headroom both afford it under (c); struck from the owner file.

### F2 Q7 (STEWARD7): the home's quota is a fourth carve term

Accepted. An unlabelled lease's root is minted below the home with `sizes.agent.quota` carved
from `home_quota` (walfsd.md "Quotas"), refused past it, so the derivation takes it:
leases(p) = min(pages, processes, weight terms, floor(home_quota / agent.quota)) for a principal
with a home; the quota term is for unlabelled leases (a labelled agent's root is its vault), and
the term stays an upper bound (whichever carve runs out first refuses). Image unmoved (alice
8 >= 7, bob 8 >= 6); the latency manifest's homes alice 18 MiB, agent 4 MiB, bob 8 MiB within
the data volume by init's check, the sum in the toml's comment.

### F2 Q8 (STEWARD7): two alice sessions in the case; six-principals' walfsd:data at its nine

The case holds alice's console session and the SSH session that drives the rounds (22,786 pages
before a lease), so 16 leases need 106,770 of her share; the rerun measured the refusal exactly
(the 14th lease OutOfMemory). Ruled (a): alice sized for both (213,542 pages, 104 processes,
weight 2,200; bob 16,384; users within 1536 MiB, the arithmetic in the toml's comment), her
carve term 20, in_flight 21; six-principals' walfsd:data buckets 10 → 9, the count the rule
derives (nine domains at the volume; the tenth a spare no client of that case uses), 9 × 21 =
189 <= 192 under Q5's bench assertion, which lands first; entries by 3V's rule (3,584 both).
scheduling.md's N = 16 sentence says the N is beside two sessions. Not (b) (a case rewrite to
avoid sizing by the rule) and not (c) (the column was promised and the manifest can carve it).

## F3. The steward's calls to shared servers inside a batch are bounded → STEWARD6

**Finding.** The steward's mints (`unattached` → `Connection::new` → `FOREVER`), its launch
step's image reads from `erofsd:system` (`server(name)` → `Connection::attach`, `FOREVER`) and a
push's source reads wait without bound; disconnects are bounded (3Y) and console writes are
(3U). B88 observed 1.43 s; a server that never answers holds the loop forever, serving nobody,
and `init` sees the steward alive.

**Ruling (mine: a bound on the steward's own calls; no wire or kernel change; the caller's
refusal already exists).** Every call the steward makes to a shared server inside a batch (a
mint by `new_connection`, a launch's image read, a push's `stat` and read, a crossing's) is made
`within` one bound, `SERVER_US`, and a call the server did not take within it fails the step:
the batch's existing failure path answers the caller (`failed` to `start_agent` and `submit`, a
refused login), the core rolls the batch back, and nothing retries inside the steward: the
sponsor or the person retries. 3W's distinction stands: a mint's `Timeout` wakes no parked step.
The bound is measured, not guessed: twice the worst per-call wait the steward's calls out show
under the latency case's load on both widths (the block-trace or a `served`-style line the
implementer adds and removes; B88's 1.43 s is the one number so far), rounded up to a second,
stated on steward.md "Residual risks" beside "a context's relay holds the steward's one thread
for a bounded time" in the same form: a call to a shared server inside a batch holds the loop
at most `SERVER_US`. The relay bounds (`HELLO_US` 2 s, `RELAY_US` 1 s) are the precedent.

What it is not: not a retry loop, not a longer client timeout, not a change to what a batch does
on success. The 30 s `sshd` waits for a login and the 5 s the shell's client waits for
`start_agent` (STEWARD8) stay the callers' own.

**Cases.** Host (fake kernel): a server that never answers a mint fails the login's step within
the bound and the next caller is served; the launch's image read the same; a push whose source
server stalls answers `failed`. Bench: the steward set unchanged in verdict; `steward-latency`
N = 1 figures unmoved (the bound is never reached on a quiet box). Pages: steward.md "Residual
risks" (the bound and its measurement), "Authentication and sessions" one clause (a login whose
server does not answer in time is refused, the person retries), agents.md "Leases" one clause.

Tier A, size S, reviewer steward-red-4. First: STEWARD5 shares `bin/steward.rs` and builds on it.

### F3, the measurement (thread STEWARD6-bound): SERVER_US = 4 s

Measured (seed 3, both widths, one and two harts; `.worktrees/STEWARD6/.tmp/STEWARD6/measure/`):
inside completed rounds the worst single call is `walfsd:data` mint-below 1.66 s (rv32, one
hart, N = 4 beside three spinners); the worst op a 3-call home stat 1.62 s. Ruled reading (a):
2 × 1.66 → 4 s. The post-death figures (home stats of 4–10 s after a round's loop died and
three spinners ran unopposed) are a wedge, not the box serving its load, and a bound admitting
them (21 s) would hold the loop longer than every caller's own bound (5 s, 30 s), inverting the
rule. The residual on the page: a hart saturated by the sponsor's lease VMs puts a server's
call past the bound; the step fails and the caller is told. The rv32 one-hart N = 4 round dying
on the shell's 5 s `start_agent` bound (5.1 s in B88, borderline) is F4's: STEWARD8's one-hart
rows are in scope too (its brief to be amended when the report names the log).

## F4. A lease's launch under load: a measured quantity with a derived bound → STEWARD8

**Finding.** On rv32 at two harts a lease VM's launch beside two spinners takes 5.3–5.7 s and the
shell's `start_agent` answers `timeout`: the 5 s is `@timeout 5000` in
`libs/wire/elixir/lib/client.ex`, the Elixir client's bound for every steward call, a platform
constant no page states as a target; no page bounds a launch at all. The launch is
compute-bound (B88): the launching VM's share beside its sponsor's other leases.

**Ruling (mine: a bound derived from a measurement, on the page; the weights are the
owner's).** A lease's launch becomes a stated, measured quantity, as `end_lease`'s 589 ms is on
steward.md "Residual risks": the `start_agent` call's `served` time alone and beside N − 1
spinners (the latency case's rounds), both widths, one and two harts, from the `steward: served
start_agent in N us` lines. The shell's `start_agent` passes the client a bound of its own,
derived: twice the worst launch measured beside the spinners the case runs, rounded up to a
second (the client's `call/5` takes a timeout; the 5 s default stays for every short call). With
the bound derived, the rv32 two-hart rounds at N = 3 and 4 run and their rows are measured; a
miss there is then a scheduler finding, not a timeout's.

Under the weights as the image has them (session 100, agent 50, steward 100; the case's steward
1,000): the number the measurement gives is what the owner's weight decision would change, not
the design. Whether a launching lease should outrank its sponsor's spinning leases (a weight
step at launch, lowered after start: a steward mechanism the kernel supports, "the lead follows
the weight") is a capacity choice: asked in the owner file with my recommendation (no: a
lease's share is the manifest's `agent` weight for its life; the bound follows the measurement).

Tier B (the Elixir client, the shell's call, a toml, pages; a `served` line already exists), size
S, one reviewer (the editor; the red team if the steward is touched). Measures after STEWARD5
and 6 land, so the table is re-measured once.

## The `sshd` step and SSH3

sshd.md "approve@box" and sessions.md "Vault sessions" are built with their cases
(`steward-approve-box`, `steward-vault-session`, `steward-vault-launch`): the step's two items
are done; "Files in and out" is M4's. Recommendation to the orchestrator: mark `sshd` done with
those status lines as evidence, so `agent-suite` is unblocked. One follow-up the page's residual
names ("Connections are first come, first served"): **SSH3**, `sshd`'s connections bounded in
time before login and sized by the manifest. Mine (a serving rule; no wire or protocol change):
a connection that has not authenticated within a bound is closed (the bound measured: twice the
slowest login the bench records, `steward-ssh-two-principals`'s 2.3 s → the arithmetic on the
page), so a slot is held before login for at most that; the slot count follows the manifest's
`buckets=N` for `sshd` as 3Q/3V have every server's limits, bounded by `ipd`'s parked calls a
bucket (5: the accept and one parked read per slot), with the arithmetic on sshd.md; after
login a principal's channels are already bounded by R82 (a login past the cap is refused, the
connection ends). Tier A (sshd), size S, reviewer steward-red-4 (or the orchestrator's sshd
red). Lowest priority of the wave: behind the agent-suite if slots are short.

## BEAM5: gen_tcp over `/net`

Scope is clear from beamlet.md ("TCP is Plan 9's `/net` ... framing stays in Erlang") and
ipd.md "The `/net` tree"; the loopback-login attack row needs it (m3-agents.md, R59). Design
(mine: a backend inside the VM over the files the namespace already holds; no new native, no
server change): `beamlet_tcp`'s transport is the session's `/net` when the namespace has one;
every socket is `ipd`'s then, loopback addresses included (R59 is `ipd`'s refusal); without a
`/net` (a lease, a host test, a VM on the fake kernel) the in-VM loopback stand-in stays, since
one VM is one trust domain. Connect by IPv4 tuple only; a name is `nxdomain` until the resolver.
Tier B unless a native is added (then re-tiered); size M; one reviewer, the red team (an attack
case lands with it). Brief: `.wash/local/BEAM5-implementer.md`.

## BEAM5 Q1: a `data` read parked longer than the hub waits → IPD1

**Finding (beam5-implementer, right).** The hub gives a request up at `FILE_WAIT_US` (10 s),
retires its fid and drops a later answer (`files.rs` `give_up`, files.md); `ipd` parks a `data`
read up to `DATA_WAIT_US` (30 s) and answers it with the bytes when they arrive. Bytes a peer
sends 10–30 s into a quiet read go into an answer nobody takes: lost to the stream. `ctl` is
idempotent; `data` is not. files.md's own derivation ("the file server's own longest wait for a
request") is broken by `ipd`, a file server whose longest wait exceeds it.

**Ruling: (A), as a serving rule (mine; no wire, protocol or kernel change).** A server parks a
request strictly shorter than its clients' wait, by a stated margin: `ipd`'s `DATA_WAIT_US` and
`CTL_WAIT_US` come down below the hub's `FILE_WAIT_US`, the margin the kernel's delivery of a
reply (microseconds; take 2 s: 8 s against 10 s, the arithmetic on ipd.md "Pinned and abandoned
calls" and files.md "Files over 9P"). Bytes arriving after `ipd`'s `timeout` wait in its buffer
for the next read, which the client makes at once (ipd.md already says the client asks again);
nothing is lost, and the only dropped answer would be one whose delivery took longer than the
margin. `sshd`'s reader re-asks as it does today, more often. Not (B) or (C): the platform's
hub gains no state for one server's habit; not (D) for a TCP stream.

Node **IPD1** (Tier A: `ipd`; size S; reviewer kernel-red-3 or the orchestrator's ipd red;
`needs: []`): the two constants, their doc comments, ipd.md and files.md's sentences, `ipd`'s
host test of the park bound, `net-tcp`/`net-pinned`/`sshd-loopback-*` unchanged in verdict.
BEAM5 builds on it (its `data` retry shape is the same) and its echo case lands when IPD1 is on
main; the brief's point 1 reads "asked again" as before, now true.

**BEAM5 Q2 (the codec on the fake kernel): accepted as proposed.** `beamlet_tcp` calls the
generated `Elixir.Redoubt.Wire.Proto.NetCtl` (the codec is generated from `libs/wire`'s
tables, so no frame is hand-rolled); the host test puts the pinned toolchain's ebin dirs and a
codec compiled from `libs/wire/elixir` in the VM's module dirs, failing loudly without the
toolchain. One condition: the test's compile is through `scripts/q run` like every build.

## BEAM5's two residuals (the orchestrator's note after the report)

1. **Sockets per session (about three at the image's defaults).** TIME-WAIT counting stands:
   ipd.md's settled rule is that a socket stays charged to its owner until it is gone, lingering
   included, and the unit is real server state (a port, buffers); R26 counts what an account
   holds. The number is a capacity choice, the owner's (how much of a shared server a person's
   session gets), and the mechanism exists already (`limits=BADGE:INFLIGHT:STATE:SOCKETS` per
   scope badge, the entry's heap sized to it): owner Q4 in the owner file, recommendation 32
   for alice's scope in the image, a Tier A size-S node when answered. No node now; the page's
   sentence (what a client sees) stands.
2. **With a pty the bench's `exit` does not end the VM.** Not a finding: closing a pty channel's
   input, once the session has read it, is reported to the steward as the terminal's close and
   detaches the context, which runs on (sshd.md "Ending", sessions.md "Closing SSH detaches").
   A pty case ends its session with `exit` at the prompt and then the bench's `ended` step
   (B83); without a pty the input's end is the shell's end of file, which is why the cases run
   so. No node.

## Order and parallelism (five in flight)

1. STEWARD6 (S) then STEWARD5 (M), one implementer in sequence or two with 5 rebasing on 6.
2. STEWARD7 (S–M) in parallel.
3. BEAM5 (M) in parallel.
4. SSH3 (S) in parallel, lowest priority.
5. STEWARD8 (S) when 5 and 6 are on main: it re-measures the table once.
`agent-suite` opens when the orchestrator closes `sshd`.
