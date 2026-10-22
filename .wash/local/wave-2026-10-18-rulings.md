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

### F1 addendum (thread STEWARD5-handover): the hand-over shape

The implementer is right that a `send` blocks until taken (it carries a timeout), so handing a
step to the thread by send would make the loop wait transitively. The shape confirmed:

- **Confirmed.** A shared `cleanup::Queue` under `redoubt_rt::sync::Mutex`, held only for the
  O(1) take/settle critical sections and **never across a Connection call** (the thread takes a
  step under the lock, drops it, makes the 1 s `disconnect`, re-takes it to record the fate);
  the thread idles in an open call on the loop's endpoint; the loop hands a step over by
  **reply** (non-blocking, R13) and takes fates as they arrive. The loop is the single arbiter
  of when the thread is woken, so no wake is lost. This departs from F1's "events" letter but
  serves its intent — the loop never blocks on a shared server — better than sends, which block.
- **Q1 (reply non-blocking in every state): yes, with one rule.** A reply to an open call never
  blocks the replier; if the thread died its call is abandoned and the reply is dropped locally,
  still non-blocking. So the loop must not *depend* on the thread being parked: it always
  enqueues under the lock, and the reply is only a wake used when the loop knows the thread is
  parked; a thread mid-step picks up the next step itself from the shared queue when it returns,
  before re-parking. A dead cleanup thread is a steward bug handled like any watched child: the
  loop takes its exit notice, restarts it, and the queue (the loop's own memory, under the
  Mutex) persists with its parked steps. Host test: the loop turns while the thread is blocked
  in a step; and the thread restarts with its queue intact.
- **Q2 (await_cleanup): amended — refused as proposed.** A window that receives only the
  thread's events for up to SERVER_US (4 s) reintroduces the stall F1 removes: a decision wake
  (target p99 95 ms) or timer wake (50 ms) arriving in it waits up to 4 s, two orders over. The
  mint-on-parked-steps case is handled event-driven instead: on a fast `Refused` with parked
  steps at that server, the loop signals the thread to run that server's steps first (reprioritise
  the shared queue under the lock) and **retries the mint on later turns when the thread reports
  a freed step**, serving every other event in between; if no slot is freed within SERVER_US of
  the first Refused, the step fails and the caller is told (STEWARD6's rule, same words). No
  thread-only window; the loop's full receive runs throughout. This is what keeps the targets.
- **Q3 (the Mutex on the loop's turn): safe, with the discipline tested.** Because the lock is
  never held across a call, a stuck thread is stuck inside a `disconnect`, not holding the lock;
  the loop's lock acquisitions are O(1). Confirm the lock is dropped before the loop's receive.
  3W's "a mint pays at most one bound, a turn's rule" becomes, off-loop: the mint pays nothing
  synchronously; its slot is freed by the thread or the step fails within SERVER_US.

Cost (3 pages, image entry 1024 → 1027): accepted; the thread stack is 1,432 B rv64 / 1,168
rv32, twice is under one page both widths (verified). budgets.md and testbench.md follow. Module
doc states the single-arbiter invariant and the no-lock-across-a-call rule.

**Addendum 2 (the implementer's refinements, confirmed):**
- Q1, dead thread: accepted correction. A thread of the steward's process cannot die alone —
  the kernel's exit notices are per process, a panic on the cleanup thread ends the steward, and
  init restarts it (reaping `users`), so there is no thread exit to take and no restart path; the
  queue is never orphaned (a restarted steward starts with an empty one, which is right). State
  this in the module doc in place of the "watched child" wording.
- Q2: it is genuinely (a) park-and-retry vs (b) fail-now — "retry the same call on a later turn"
  is parking the call, so my addendum's wording implied (a). **Ruled (b).** (a) breaks the
  steward's load-bearing no-park invariant (protocol.rs: it answers every call as it arrives and
  parks none, part of R26's fairness), double-audits a login (a failed record then a success for
  one login), and keeps consoles. (b) keeps STEWARD6's rule verbatim (a server that does not free
  a slot in time → failed step, caller told) and the no-park invariant; nothing retries inside
  the steward; 3W's test becomes "the login after it succeeds". The caller (sshd, the shell's
  harness, the person) retries, the established pattern. The transient (a slot released to the
  cleanup thread but its disconnect not yet done) only bites if the share is at cap AND a reuse
  comes within one disconnect bound (~1 s); STEWARD7's derived caps plus the latency rounds'
  ≥1.5 s sleeps give the thread its slack, so no spurious `failed` in a reachable row. If
  measurement shows a transient in a reachable row, the case's driver gets a bounded retry (a
  realistic harness), never a design change. Flag to STEWARD7 so it expects no spurious failure.

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

## The agent-suite step (first wave, 2026-10-19): ATK10, ATK11, ATK12

Scope is clear from docs/plan/m3-agents.md: the step turns that page's "not yet" rows into bench
cases and builds Alice's agent under a lease, the agent harness and the escape room. The harness
(`Redoubt.Agent`), the agent loop (`pi`-small) and the escape room are M5-status on agents.md
("Status: planned · M5"); they wait for gatewayd and the model gateway and are **not** this
wave. What is built today and only lacks its attack cases: leases, sub-agents, the powerbox,
crash blame, vault sessions, the loopback refusal (session-tcp-loopback), and the kernel's half
of fairness and launch. So the first wave is three attack-case packages, each a group of
m3-agents.md's rows, each judged by rule F, none touching the steward binary, its loop files or
the latency manifest (STEWARD5/STEWARD7 own those):

- **ATK10** the scripted hostile agent (escape, lease-cut, flood, vault note);
- **ATK11** the scripted hostile user at the shared servers (walfsd/blkd/ipd fairness, R37);
- **ATK12** resources, hostile launch, leaky state (R48, R31/R32, R37/R36).

All three run in parallel beside STEWARD5/7/8: disjoint cases and agent modules; the one shared
file is m3-agents.md's table, each editing only its own rows. No owner decision is needed for
these three: every rule they attack is built and stated. The harness, the loop and the escape
room are a **later wave** of the step, after gatewayd (owner territory: a new egress server,
R65/R66); I will draft them when the orchestrator asks and the gateway's scope is set, and flag
then whether gatewayd itself needs an owner decision before agent-suite can complete. A reviewer
reading these should know: the "not yet" rows for the harness-driven agent (Agent.start/prompt)
stay not yet; these cases drive agents by `start_agent` and `~/project/in` as the built cases do.

### F2 finding (STEWARD7 N=16, one hart rv64): the read path starves under 16 agent VMs

At one hart the fifth round's 30th launch is refused at `erofsd:system`: 20 live connections of
the cap's 28, plus eight ended probe leases' disconnects parked after four attempts — the server
starved of the hart by sixteen lease VMs of weight 50, 3W's residual at scale. **A finding,
stated, not a node, and the row is unreached, not moved** (the implementer is right).

The orchestrator's three candidates, ruled:
- **Not the admission term (c).** A parked disconnect genuinely holds the server's slot: the
  server has not processed it, so the `State` unit is really still held. Counting it free would
  over-admit and the server itself would refuse. Sizing the cap with headroom for in-flight ends
  does not help either: under starvation the zombie-disconnect window is unbounded, so no finite
  headroom suffices — a larger cap just admits more VMs to starve the server further. The term is
  honest.
- **Not STEWARD5 (b).** The cleanup thread keeps the loop responsive while the slots are held
  (the important property: the box stays live and the 30th launch's `failed` is correct, the
  server really being full), but it cannot give a starved server CPU, so the slots still linger.
  STEWARD5 is not the fix; it makes the refusal clean, not absent.
- **It is a weight/capacity matter (a), owner-adjacent.** The read-path servers
  (`erofsd:system`, `verityd`, `blkd`) at weight 100 cannot drain ended leases' disconnects as
  fast as leases end while sixteen VMs of weight 50 (800 total) share one hart. Draining must
  keep pace with ending, which is CPU, which is weight. At two harts every launch completed, so
  the gated two-hart N=16 row is the measured one; one hart with 16 agent VMs is intentional
  oversubscription, and the row is stated unreached with that cause, as rv32-two-hart-N=3 is
  (B88). No target moves.

No node now. Whether a shared server's weight should scale to serve N agent VMs is folded into
the owner's open weight decision (Q1/Q2): a data point added there. If the owner's answer wants
it, the node is a weight change (the image's read-path weights), not STEWARD5's and not the
admission term's. This also confirms the STEWARD5(b) transient reaches `failed` under one-hart
oversubscription; the latency case states the one-hart N=16 row unreached rather than retrying.

### F4 refinements (thread STEWARD8-bound)

(a) **Source: the `served start_agent` line, added by STEWARD5, not STEWARD8.** The implementer
is right that `served_op` prints only `submit` and `end_lease`, and that AgentStarted-gap-less-
sleep overstates (STEWARD7's 6.8 s gap with no 5 s timeout is the shell VM's own share and its
sleep's lag, not the steward's service). The steward's arrival-to-answer for start_agent is what
the client's bound should cover, so it is the source; state B88's gap beside it only where it
differs by more than the sleep's rounding, as a cross-check. The one line `10 => Some("start_
agent")` is a Tier A steward edit and outside STEWARD8's Tier B: fold it into STEWARD5 (in flight,
already in `bin/steward.rs`), or the orchestrator adds it; STEWARD8 measures from it once on main.
My brief's "a served line already exists" was wrong — this corrects it.

(b) **Home: option (1), with a page note.** `launch_timeout/0` beside `timeout/0` in
`libs/wire/elixir/lib/client.ex`, the arithmetic beside the 5 s it is derived against; the latency
case passes it as start_agent's third argument; `timeout/0` (5 s) stays the default for every
call, start_agent's generic path included. Add: agents.md's harness section states that launching
an agent uses the launch bound, so the M5 harness (`Redoubt.Agent.start`) inherits it and does
not reintroduce 5 s. escape.ex's probe (a lease's badge calling start_agent, refused `unknown` at
once) keeps 5 s — it never launches, so it needs no long bound; change it only if it has a real
launch (check). Not (2) now: a per-message default timeout in the steward wire table is the
durable fix and is the Architect's remit (the steward's own table, not the owner's), but it is a
generator and table-format change; deferred until start_agent callers proliferate, noted on the
page as the follow-up. Not (3): no shell lease module exists today.

### F2 findings 4 and 5 (STEWARD7 completion)

**Finding 5 (rv32 one hart N=16: the shell loses its modules at erofsd).** Not a correctness
finding, and not the same as finding 3. The reads are honestly `busy` (R26's fair share
`limit/(n+1)` is honored — each of 17 VMs gets its share), the hub retries and returns `failed`
at its own 10 s; the server loses no read. But it is also not mere CPU starvation: even with
infinite CPU, 17 VMs sharing a 2-page bucket by fair share get ~1 page each, so a page-carrying
read whose need exceeds that share cannot complete whatever the hart does. The real cause is a
**derivation gap**: STEWARD7 gave `state` and `in_flight` a VM-count term but `pages` and
`requests` kept their per-batch constant (erofsd's "the most one batch needs", 2), so they do
not scale with an account's concurrent VM count. Ruled: **stated unreached now, a node only if
the owner's agentic-workload decision commits to many agent VMs of one account reading a shared
volume at once.** Then it is a package in my remit (complete the derivation: `pages`/`requests`
gain the VM-count term, as `state`/`in_flight` did), Tier A, at the cost of every server's heap —
so it waits for the requirement (the gateway/harness wave and owner Q1/Q2), not this
oversubscription. One check for the implementer that would flip it to a node now: if at a
**supported** configuration (the image's per-account VM count, not N=16) a single page-carrying
read's need exceeds a VM's fair share, that is a liveness bug and its own node; confirm it bites
only at N far above the image's concurrent-VM count.

**Finding 4 (rv64 two harts N=16: 29th launch refused, 20 live + 8 parked disconnects).**
STEWARD7's latency runs are from 5923f7bc8, **before STEWARD5's cleanup thread** (train 71), so
finding 4 is the old on-loop cleanup under a two-hart lease-end burst, not a finding about the
current system. Ruled: re-measure under STEWARD5. **CLOSED** (STEWARD8, tree 0d8d9d463, STEWARD5
in): steward-latency PASS at both hart counts, every round reached including N=16, every target
met, 0 `parked after` lines, all 31 launches of the two-hart N=16 round served. The off-loop
cleanup thread (F1) drains the two-hart end burst the old on-loop cleanup could not; finding 4 was
the pre-STEWARD5 model. No node.

**Finding 5 — CLOSED, no node.** STEWARD8's check: a waiting module walk pins one page and the
fair share always allows one (the `.max(1)` floor), so a read's need never exceeds its share at
any VM count; the bucket's ceiling of two page-carrying requests per (account, label set) per
volume is a drain-rate matter, not a liveness bug at the image's nine VMs. Stated unreached as
ruled; the `pages`/`requests` VM-count term stays a would-be node only if the owner's agentic
workload commits to many concurrent agent VMs per account (owner Q1/Q2), unchanged.

(STEWARD8's start_agent `served` figures for F4's bound: one hart 1.70 s alone / 3.13 s beside 15
spinners; two harts 2.67 s / 3.72 s. The launch_timeout derives from the worst, 2×3.72 → ~8 s.)

### F4 — the VM's call ceiling (thread STEWARD8-bound): MAX_CALL_MS is the longest call

beamlet's `call` native refuses a timeout above `MAX_CALL_MS` (5 s, `bif/system.rs`), so the
client cannot pass the launch bound — the 5 s client constant was that cap restated. Ruled
**(1)**: `MAX_CALL_MS` becomes the launch bound, derived as the longest call the system makes (a
lease's launch under load), measured over **both widths** (2× the worst `served start_agent`
rounded up to a second; rv32 included — so the number is the measurement's, 8–12 s, not a picked
12). `launch_timeout/0` and `MAX_CALL_MS` are the **same number** (the client's constant is the
native's ceiling, which is the longest call). Why (1) is sound and small: `MAX_CALL_MS` is the
VM's ceiling on its **own** calls, and a VM is one trust domain — it caps the timeout a caller may
*request*, not any call's default, so short calls still pass their own short timeouts, and a VM
over-using the ceiling holds only its **own** call-pool threads (hurts itself, no cross-domain
reach). The 5 s was a guess; the measurement corrects it. `MAX_QUEUED` unchanged.

Not (b) (the orchestrator's: keep the general cap 5 s, a per-call higher ceiling the launch path
alone elects). It is cleaner than (3) — the caller elects "a long call", the native stays
protocol-agnostic — but the general 5 s cap's independent value is small: a crashed server returns
`Dead` at once (R4b), so the cap only bites when a server is alive-but-slow (overload), where the
VM's operations are already slow; it does not justify a second ceiling and a selector on the
platform boundary. There is one longest call (the launch), and the ceiling is it — the cleaner
invariant for the book, one derived number ("the longest a call waits is the launch bound"). If
interactivity measurement later shows the raised general cap hurts the shell under overload, (b)
is the refinement then, not pre-emptively.

Not (3): a per-opcode cap puts a steward opcode into the platform native, which encodes no
protocol today — a layering violation. Not (2) now: making start_agent asynchronous at the
steward (answer at once, the launch's outcome by a notice) is the durable shape if long
synchronous calls proliferate, but it is a steward **protocol** change out of this node's scope;
recorded as the follow-up (a launch is a long operation that arguably should not be a synchronous
call; when there are several such, (2) is the fix).

Pages: beamlet.md's constants row and the natives' paragraph state the derivation ("the longest
call the system makes: a lease's launch under load, measured") in place of "5 s". The implementer
greps for other dependents of the 5 s / `MAX_CALL_MS` (Tier A). Whose edit: cohesive with STEWARD8
(same number, same measurement), so STEWARD8's second platform edit, with the beamlet reviewer
added for `userland/otp/vm`; the orchestrator's call.

### STEWARD9: the latency case's deadline lease follows the measured launch

STEWARD8's verification found the latency case's 5 s kernel-deadline lease (sized to twice an
older two-hart launch) now dies inside its own launch (launch 6.62 s rv32 two-hart; B82:
NotPermitted, a forbidden line ends the boot), so the rv32 N=3/4 rows that now complete cannot be
judged. Ruled: **a small node, STEWARD9 (Tier B, toml + a page sentence), not folded into
async-start_agent.** The deadline-during-launch is B82's kernel behaviour, independent of whether
start_agent is synchronous, so it stands alone. The case's deadline lease and post-sleep are
derived from the measured worst launch over both widths — the same measurement STEWARD8 took, so
the case's derived numbers (launch_timeout, MAX_CALL_MS, the deadline lease) all come from one
launch measurement: lease = 2× the worst launch (~14 s, one value both widths) so a deadline fires
on a running VM (the notice's quantity); the sleep ~16 s so the notice is recorded before the next
round. State it in the toml comment and scheduling.md; the rv32 N=3/4 (and N=16 where reached)
deadline-notice rows are then measured. needs STEWARD8 (same file). Caveat in the brief: if the
deadline class is a probe constant in the steward binary, not a toml value, it is Tier A and
folds into a steward package — the implementer checks first. Plan: plan-2026-10-19-steward9.toml;
brief STEWARD9-implementer.md. rv64 already PASSes every round (worst launch 3.95 s); this is the
rv32 rows' last blocker.

**STEWARD9 amendment (thread STEWARD8-bound): the sleep is per round, not per lease.** My "sleep
so the deadline fires before the next round" was read as a 16 s serial sleep after each of 16
deadline leases, which inflated the run to ~1746 s of guest time and overran the sched-trace ring
(it stops at ~1394 s on rv32, dumping `dropped 1536145`, so the oracle judges nothing; the ring
records 4.2–7.8 k scheduler-tick records a guest second whatever runs, so it scales with wall
time). Corrected: the deadline leases **overlap** — launch them without a full serial deadline
wait each, let their deadlines fire as a cluster, and the round waits **once** for the cluster's
notices; the deadline notice is still measured per lease (concurrent expiry is, if anything, the
more realistic load, and no target moves). That cuts the deadline phase from ~16×16 s to ~one
deadline per round, so a five-round run fits well inside the ring with the lease at 2× the worst
launch. The ring stays sized for the kernel's cases: it is a kernel resource and must not grow for
one case (and cannot on rv32, memory at 1536). Ruled: **(c)**, this reshaping.

Tier: if the deadline leases' sequencing is the toml's Elixir, STEWARD9 stays Tier B; if it is in
the probe (`bin/steward.rs`, the `latency-probe` arming), that part is Tier A and the orchestrator
re-tiers STEWARD9 — still one small package. **Fallback if no five-round shape fits the ring:**
(b) as a Tier A node, a trace that records only from the first round's marker **and only the event
classes the latency oracle judges** (the destructions for R10 and the wakes), not every scheduler
tick — the trace serving its oracle, the ring size unchanged. **Last resort (d):** the
deadline-notice rows alone stated unjudged with R10 from a per-destruction sub-window (STEWARD4
Q9's precedent), never the whole table. **Not (a):** the ring is not sized by one case and rv32
cannot grow it.

### STEWARD10: the stale-handle-index wrong-destruction (thread STEWARD8-bound)

STEWARD9's concurrent deadline leases surfaced a Tier A correctness bug, and the finding
under-reports it. Verified in the kernel: a handle's `Object::Budget(BudgetRef{frame, id})`
carries a generation `id`, and `budget_at` (handle.rs:152) asserts only that the handle *at the
slot* is self-consistent (I1), **not** that the steward's index still means the lease it intended.
A lease's R10 deadline revokes the steward's handle and frees its table index; `install_handle`
(lowest free index) reuses it; the queued DestroyBudget cleanup (drive.rs:520) then resolves the
stale index to the newer object. The reported `WrongObject` (a non-budget at the index) is the
**loud** form drive.rs:526 fails on. The **silent** form is catastrophic: a different **budget**
at the index (a new lease — the common case, since launches are leases) makes `budget_handle`
return `Ok` and the steward **destroy the wrong live budget with no error**. drive.rs:526's
`BadHandle if !running` was correct only while sequential deadlines freed each index before the
next launch.

**Ruled: a node, STEWARD10 (Tier A, steward-red-5, S–M), NOT option (a).** Swallowing `WrongObject`
hides the loud form and does nothing for the silent one. The steward must never destroy a budget
by a bare index R10 may have revoked and recycled. The cleanup keys on the budget's identity (the
`BudgetRef` id the steward minted) and is cancelled/skipped when it is gone, covering `BadHandle`,
`WrongObject` and the different-budget case uniformly. Prefer the **steward-only** form: on a
lease's deadline-killed exit (the kernel already destroyed it) cancel that budget's pending
DestroyBudget step under STEWARD5's queue lock, the single-arbiter loop cancelling before it
installs any handle that could take the freed index — no kernel change. A kernel "destroy index i
only if it still names id X, non-fatally" is the alternative and is adjacent to the owner's
handle-generations question: raise it to me (then the owner) only if the steward-only cancel
cannot be made race-free. This is a **real production scenario** (several agents' leases expiring
together), not a test artifact. Plan: plan-2026-10-19-steward10.toml; brief
STEWARD10-implementer.md.

**Interaction with STEWARD9:** the overlapping-deadline reshape (c) that fits the ring is exactly
what fires deadlines concurrently and recycles handles, so STEWARD9 depends on STEWARD10. Order
STEWARD10 first (or concurrently); STEWARD9's overlapping shape lands on it, and until then
STEWARD9's deadline rows stay unjudged (the orchestrator's framing). STEWARD10 stands on its own
merits regardless of the test.

**STEWARD10 mechanism amended (implementer's facts).** The cancel-under-lock I ruled does not map
onto the code: DestroyBudget is inline in drive.rs `step()` (Exited, an end_lease batch, forget()),
not a STEWARD5 queue step, and the steward cannot read the kernel's BudgetRef id. Superseded by
the **deadline as the identity witness**, which is better (it covers end_lease/forget served after
d, which a notice-cancel could not) and needs no kernel change: `Made::Budget` carries its
deadline `d`; a destroy reads `kernel.now()`=`t` immediately before, no install between (only the
loop installs); `t >= d` → the kernel owns it, drop the record, never destroy by index (the whole
safety guard: the only unsafe outcome, Ok on a recycled budget, cannot occur); `t < d, Ok` → the
original; `t < d, BadHandle` → benign (gone), a diagnostic only if every legitimate before-deadline
end provably removes its record inline; WrongObject → an unreachable invariant tripwire. Same in
forget() (crossings, CROSSING_LIFE). The kernel "destroy-if-id-matches" alternative is not needed.
Brief updated to this shape.

**STEWARD10 review notes (steward-red-5, OK with notes), both into STEWARD10, not STEWARD9:**
(1) The premise "the loop alone installs handles" is wider than the kernel gives — handles install
at a receiver's receive and a server's reply entry (ipc.md), so a reader thread's consoled calls
or the cleanup thread's disconnect/ended would install one if a reply carried a handle; none does
today. State it as a named premise on steward.md's cleanup bullet (the guard rests on no reply the
steward's threads take carrying a handle; if one ever does, the no-install-in-the-window argument
must be rechecked). This is STEWARD10's own premise on STEWARD10's own page, so it goes in
STEWARD10's page fold now, not a later package. (2) drive.rs:550 `Made::Scope(b) => (b, FOREVER)`
destroys a scope unguarded (FOREVER → always destroy by index); unreachable today (effects.rs
names the budget token, scopes are never destroyed by handle), but apply the guard / a BadHandle
tripwire there too so the invariant "no destroy by a possibly-stale index" holds **uniformly by
construction**, not everywhere-but-one-unreachable-path (which the next person who makes it
reachable reintroduces silently). Fold into STEWARD10 if steward-red-5 re-oks a few lines; else a
named follow-up for the next steward package — **not STEWARD9** (a drive.rs guard is Tier A steward
code, outside the latency-case node). Neither blocks STEWARD10's merge.

### B89: sshd-login-bound orders on the accepts it saw

Train 74 run 2 (rv32): sshd-login-bound (SSH3's) timed out waiting for a fourth close line. The
bench peer held the fourth dial 11.60 s, so only three connections were concurrent (slots 0-2;
slot 3 never used) and the late fourth reused slot 0 after connection 1 closed — no fourth
distinct accept. The case's expect list assumes all four are accepted before any closes, a timing
assumption the loaded rv32 host broke. Ruled a small B node (Tier B, size S): the case serializes
the dials (each after the previous's `on slot` line, so all four are up on slots 0-3 before any
reaches the 5 s bound) or orders on the accepts seen; `buckets=4` is right (slot 3 was simply
never reached), so it is a case-ordering flake, not a slot bug. Escalation named: if, once
serialized, the fourth still cannot get a distinct slot, that is a genuine held-slot SSH3
correctness finding to raise, not paper over. Plan: plan-2026-10-19-b89.toml; brief
B89-implementer.md. The orchestrator assigns the B-number.

### STEWARD9 N=16 spacing and the "session cannot observe its own lease's end" finding

My carve-success-ordered spacing is not implementable in scope (a failed carve is fatal and ends
the boot; lease_ended op 17 is consumed by the shell's notices listener, not session code; no
lease-liveness op; only a contexts poll, which pollutes the measured phase). Ruled: **the derived
wait** — the next measured lease requested 14 s (the deadline it set, from the request per B82) +
1 s (the margin over R10's ≤30 ms destroy that frees alice's pages), guest clock under icount, so
deterministic, not a wall-clock guess (the memory "order on events" rule targets non-deterministic
*host* timing; a guest-time wait derived from the minted deadline + R10's p99 is not that, and is
what the case's Process.sleeps already are). Not (B) the contexts poll (it adds steward calls into
the measured deadline phase). The toml comment states the derivation and why no event is available.

The finding **"a session cannot observe its own lease's end"** is real but not a node now: it is an
M5 agent-harness concern (owner Q5). The harness's design answer is `Agent.status` (a steward
poll, already on agents.md) or the shell forwarding lease_ended to session code — not something to
add for a test. Recorded as a **harness-wave follow-up**; if the owner pulls the harness into M3
(Q5), I design session-visible lease completion then. Do not change the shell or add an op for the
latency case.

### STEWARD9 measurement shape + STEWARD12 (the burst-notice finding)

My clustered shape measured the wrong quantity: a burst of 16 simultaneous expirations inflates
the LAST notice by the serial per-exit dequeue (~N×70 ms), which is not the deadline_notice
target's quantity (a single lease's notice latency). The sequential N=16 round meets the target
(10 ms), proving the quantity is fine measured one notice at a time. Ruled **(A): sequential-all**
— drop clustering entirely, every round's deadline leases run one at a time with a per-round
derived spacing (2× that round's worst launch + 1 s), uniform across N, measuring the per-notice
target (met everywhere); it fits the ring (the hybrid's ~490 s headroom shows it). Not (B) (measure
the kernel's delivery — a narrower quantity that hides the steward's notice latency), not (C)
(fewer D weakens p99 and does not fix the burst), not (D) (the target is NOT missed for its defined
per-notice quantity, so accepting a "miss" would mislabel a burst phenomenon).

The cluster-burst is a real finding and **becomes a node, STEWARD12** (Tier A, steward-red, S–M):
a burst of N expiring leases costs the last sponsor notice ~N×70 ms at the steward's loop, because
the notice is sent after each exit's cleanup/audit/console lines. Fix: send the lease_ended notices
FIRST for a batch of exits, then the per-exit bookkeeping (cleanup is STEWARD5's thread, lines are
STEWARD3U's queue, already off the critical path; hoist the notice ahead of the ~70 ms that
remains). This protects the deadline_notice target under a burst — the human-control promise that
the sponsor is told promptly when a lease ends (several agents' leases expiring together, the
production shape I named at STEWARD10). Not blocking STEWARD9 (which measures the per-notice target
sequentially and passes). Plan: plan-2026-10-19-steward12.toml; brief STEWARD12-implementer.md.
scheduling.md records the burst finding until STEWARD12 lands.

**STEWARD9 follow-ups (FYI, accepted):** with clustering gone at most one deadline lease is live,
so the probe's class grouping and LATENCY_DEADLINE_US are unneeded — the probe reverts to its
original shape and STEWARD9 becomes toml + scheduling.md only, Tier B (steward-red-5). N=2's
derived deadline lease rounds to exactly 9 s, which collides with the probe's own-lease length
sentinel (LATENCY_LEASE_US = 9 s), so N=2's lease is 10 s — a stated deviation from the house
round-up, still ≥ 2× the launch, with the cause in the toml and the row. **Noted finding, not a
node now:** the probe identifies its own lease by a magic length (9 s), which any derived lease
that rounds to 9 s collides with; a robust marker (a distinct token on the probe's lease) would
remove the fragility — a test-infra follow-up for the next latency-probe touch (STEWARD12 touches
steward.rs but its scope is notices-first, so not there unless cheap), not blocking.

### STEWARD9 ring (two harts) + STEWARD13 (the launch holds the loop)

**Ring: (W) two boots.** Sequential-all passes rv64 one hart (deadline_notice 10-14 ms, dropped
0) but overruns the ring at two harts (~14 % over; the hybrid's headroom was the cluster's). Run
the case as two boots — N=1-4 and N=16 (two tomls or one parameterised) — each judged by the
oracle on its own per-boot ring; no kernel change, no target or D moved, the table's meaning
unchanged, the one-hart rows stand. Not (X) trim the sleep (3 % margin, fragile), not (Y) a larger
ring (a kernel resource, must not grow for one case; rv32 maxed), not (Z) fewer D (weakens p99).
STEWARD9's two boots (N=1-4, N=16) share the manifest and recipes (the bench has no rounds
argument). (W) is not pure toml: the probe prints LATENCY-END on a compile-time LATENCY_ROUNDS=5,
so a 4-round boot never ends its window. Ruled: end the window on the `latency-done` marker the
case already writes after its last round — the probe reads the marker instead of counting rounds,
so the **case owns its round count** and the LATENCY_ROUNDS magic constant goes (a robustness gain,
the same fragility class as the length sentinel, here fully retired). That re-tiers STEWARD9 to
**Tier A** for the few probe lines (steward-red-5; no kernel, D, target or ring change), superseding
the earlier probe-revert-to-B.

**The launch-holds-the-loop finding → STEWARD13 (sibling of STEWARD12, not a widening).** The real
deadline_notice delay is a lease's deadline firing while the steward is inside a 2-6 s synchronous
start_agent launch — the notice waits behind the launch. The latency case sequences around it (the
steward idle at each deadline, meeting 40 ms), but in production launches and deadlines overlap, so
the deadline_notice human-control target is unmet whenever a launch is in flight. notices-first
(STEWARD12) does not help when the loop is *inside* start_agent, so STEWARD12 stays narrow (the
simultaneous-deadline burst bookkeeping, still a real independent gain) and the launch fix is
**STEWARD13**: the launch's slow part runs off the loop (a launch thread, STEWARD5's single-arbiter
pattern — the loop serves deadlines/notices meanwhile and replies to start_agent on completion, so
start_agent's synchronous contract is unchanged; NOT the deferred async-start_agent protocol
change). Design question I settle first on the settled base: a launch thread installs the child's
handles, which touches STEWARD10's "no install off the loop" premise (steward-red-5's note 1) — the
deadline-witness guard must be re-shown to hold with a launch thread, or the installs kept on the
loop with only the I/O off it. So STEWARD13 needs STEWARD10 and STEWARD12, and its brief follows
my design. Until it lands, scheduling.md states the residual: deadline_notice is clean with the
steward idle at the deadline (the sequential measurement); a launch in flight delays it up to the
launch bound. Plan: plan-2026-10-19-steward13.toml.

### STEWARD9 rv32 N=16 → (a) + READ1 (the drain-rate finding reaches VM death)

rv64 is complete (both cases pass both hart counts, dropped 0, deadline_notice met). On rv32 at
N=16 a spinner VM dies mid-round: its module reads at the system volume time out (the VM hub's
10 s) behind the other VMs' reads — STEWARD8's finding 5 (the read path's per-account pages/
requests ceiling does not scale with the concurrent VM count) reaching a liveness consequence
(a VM dies, not just slows). Ruled **(a):** the N=16 round is measured on rv64; rv32's N=16 row
states the cause and names READ1. Not (b) (tolerating N=15 passes a case on a dead VM — dishonest;
and "unjudged inside the case" is just (a)'s stated row), not (c) (blocks STEWARD9 on an unsized
package). The bench conditions only at case level (arch), not per step, and N=4 cannot rejoin N=1-3 on rv32
(that is the overrun they split from; rv32 N=1-3 alone is 5.99 M of 8.39 M), so (a) is **three
cases**: steward-latency (N=1,2,3, both widths), steward-latency-n4 (N=4, both widths, fits alone),
steward-latency-n16 (N=16, `arch=["rv64"]`; rv32 blocked by the drain rate, its row on scheduling.md
naming the cause and READ1). rv64 runs three boots, rv32 two. The `arch` field cleanly expresses
"N=16 is rv64-only"; no per-step width condition needed. Evidence sharpening READ1: a spinner VM
`exited 1` (crashed, not killed/ended) before the first N=16 deadline notice — its module reads
past the hub's 10 s give-up — so one READ1 lever is concrete: a timed-out code read **kills** the
VM (it exits 1), and READ1 must decide whether it should (the VM surviving a slow code read) or the
read must complete (the read-path capacity).

**READ1 (drafted, the orchestrator ids it; a read-path package, B88's sibling; needs nothing of
STEWARD12/13).** A lease VM must not DIE from module-read starvation under its account's concurrent
load. Scoped carefully: the goal is NOT "rv32 N=16 passes" (a full rv32 system is beyond M6; rv64
passes N=16) but "no VM death from starvation" + a stated supported concurrent-VM count per
account at the system volume, which could bite rv64 at higher N too. Investigation-first: measure
the fatal N both widths; the levers are the pages/requests VM-count term (finding 5, my remit),
verityd's cache/read-ahead for N loaders (a B88 extension), the read-path weight, and the VM's
resilience to a slow code read (must a timed-out read kill the VM?). The implementer measures and
brings the fix choice back to me. Plan: plan-2026-10-19-read1.toml; brief follows the measurement.

### STEWARD9 rv32 two-hart: SCHED5 (timer_wake) + BENCH1 (per-width smp)

rv32 at two harts has a deterministic N=2 timer_wake p50 miss (31.8 ms vs 15; N=1 5.6, N=3 5.2;
everything else met, dropped 0; lock-wait 249/1000), pre-existing and first judged now (STEWARD8
never reached the reset on rv32 two harts). rv64 both harts and rv32 one hart pass everything.
Ruled **(a):** it is a kernel lock-contention finding (scheduling.md's "a wake can wait for the
kernel lock at 2 harts" residual, R78), **a node of its own, SCHED5** (kernel scheduler,
investigation-first — why N=2/one-spinner specifically; fix or stated residual), not STEWARD9's.
STEWARD9 states the rv32 two-hart rows as findings with their numbers on scheduling.md and gates
rv32 at **one hart** until SCHED5 lands. Not (b) (don't scope-creep STEWARD9 into a kernel
investigation).

Bench mechanics: arch and smp are case-level fields, so "rv32 one hart, rv64 both" needs a
**per-width smp** (BENCH1: `smp = { rv64 = [1,2], rv32 = [1] }`, the list staying the default),
reusable (the latency cases, and B88/STEWARD8 wanted the same), rather than a duplicated rv64-only
two-hart toml. BENCH1 is a small Tier A bench node; STEWARD9 uses it for clean gating (or carries a
stated smp and adopts it when it lands). Both drafted: plan-2026-10-19-sched-bench.toml; the
orchestrator ids them. SCHED5 and READ1 are the two rv32 findings the latency table names.

### STEWARD12 burst finding → SHELLN1 (the real fix is the sponsor's take rate)

STEWARD12's measurement reframed the burst: the ~70 ms per exit is the lease_ended SEND (a
rendezvous), not the bookkeeping (effects 16 ms). The shell takes one notice per '[lease ended]'
print through the relay, so a burst's last notice is ~N x the shell's take after the deadline, and
one notice was LOST (a send past NOTICE_US dropped). So notices-first (STEWARD12) cannot cut the
burst, and the deadline_notice under a burst is bounded by the SPONSOR's consumption, the shell's
to fix. Ruled **(1):** STEWARD12 lands the burst cases recording deadline_notice with no bound and
the residual stated (N x the sponsor's take); notices-first is near-no-op (16 ms of 581 ms) — keep
or revert per the reviewer (I lean revert). The per-notice deadline_notice TARGET still gates via
STEWARD9's sequential rows.

**The real fix: SHELLN1 (Tier B shell, drafted).** The shell's notices listener takes each
lease_ended before it prints (receive, enqueue, print async), so the send completes in ms, a
burst's last notice is within NOTICE_US (no loss), AND the loop is not blocked on the rendezvous
(largely subsuming STEWARD12's point-2 deferral and its (2) sends-off-loop — measure after before
building a notice thread). Not (3) (STEWARD12 does not widen into the shell; SHELLN1 is its own
node). **Kernel finding:** the first notice waits ~590 ms because the kernel destroys N expired
budgets at one entry before any notice (R10 batched); state it as a residual (a burst's first
notice waits ~N x R10), a kernel node (incremental deadline destruction) only if it matters.
Plan: plan-2026-10-19-shell-notices.toml; the orchestrator ids SHELLN1.

## agent-suite wave B (2026-10-19)

The two rows ATK10 left are sound now, as one small package **ATK13** (Tier A, S): (1) the R60
labelled-agent sink row becomes a boot case on `steward-approve-box`'s path (a labelled agent
has no `/net`, the namespace the first wall and R60 the backstop, cited to `net-attacks`); (2)
the R44 keyd relayed-blob row has **no boot arm reachable** in the image (no attacker holds a
keyd badge until M6's keys-in-leases), so it resolves on the page by citing keyd's host tests and
a login's end-to-end host-signature verification. No manufactured case.

Everything else the step needs — the agent harness, the pi-small loop, the escape room — is
M5-status and needs **gatewayd**, an owner decision (owner Q5). Residuals check: the loop-bound
finding is STEWARD5 (in flight), the FOREVER mints are STEWARD6 (done), sockets per session is
owner Q4, SSH3 and IPD1 are merged. So after ATK13, **nothing but the gateway remains** for
agent-suite, and that is the owner's to decide before the second wave.

## Order and parallelism (five in flight)

1. STEWARD6 (S) then STEWARD5 (M), one implementer in sequence or two with 5 rebasing on 6.
2. STEWARD7 (S–M) in parallel.
3. BEAM5 (M) in parallel.
4. SSH3 (S) in parallel, lowest priority.
5. STEWARD8 (S) when 5 and 6 are on main: it re-measures the table once.
`agent-suite` opens when the orchestrator closes `sshd`.

## READ1 gate verdict + WRITE1 (thread READ1, 2026-10-20, architect-6)

**The gate's verdict channel: right.** The first shape (agents writing timing files to `/project`,
the shell counting done lines) made the data volume's write path part of the verdict and hit the
hang below. The reshaped gate: Echo's load writes nothing, loads the 27 modules and exits 0 or
halts non-zero on a refused one; the verdict is the **steward's console** — sixteen
`lease alice/agent-N ... exited 0` lines, one per distinct name, any non-zero exit forbidden — the
session waiting on its sixteen lease-end notices, then the shell's probes at both volumes; N = 16
on the latency manifest, both widths, smp [1, 2]. The steward's line is the party the VM cannot
impersonate (rule F's shape, though this is a capacity gate, not an attack case), and the probes
are the account-wide check decision 5 asked for. Three conditions: (1) the expect list names the
sixteen agents, not sixteen lines; (2) Echo's load must not be able to swallow a refused load as
exit 0 (each module's load checked, non-zero on any failure); (3) the data volume's half of the
derived cap (walfsd 71, the hub's w = 2) is exercised only if the leases still attach `/project`
at launch (their root fids held) — confirm from the launch that Echo's VM attaches the home as
phase 1's `/project attach -> exit 14` showed; if not, say on the page that the data-volume cap
is held by the host test and the shell's probe alone, not by sixteen attached leases.

**The write-completion hang: a node now, WRITE1 (the orchestrator ids it).** An agent that never
returns from a write whose bytes reached walfsd is a liveness bug, not a throughput finding: in
production the lease burns to its deadline doing nothing and its sponsor is told nothing — the
human-control promise weakened without a line. Distinct from decision 3 (slow writes, an answered
`etimedout`) and from READ1's admission death (a refused fid): the write was served and its
completion lost, past the hub's own 10 s give-up, so some path fails to answer its caller. First
measurement, hub vs walfsd, for one hung write: did walfsd send the Rwrite; did the hub give up
at `FILE_WAIT_US` (`give_up` retires the fid and drops a later answer); was the Elixir caller
answered by either. The three outcomes name the owner — walfsd's write path or its per-connection
parked completion call (STEWARD7's `in_flight`); the hub's give-up not answering the caller
(IPD1's rule from the other side: a late reply may be dropped only after the caller was answered);
or beamlet's call path losing an answer. The rule to hold whichever it is: every hub request ends
in exactly one answer to its caller within `FILE_WAIT_US` plus the reply margin. READ1 lands with
the finding stated on walfsd.md "Residual risks" and the data volume out of its verdict; WRITE1
replaces the sentence with the rule. Tier A (files.rs + walfsd), S–M, investigation-first, the
brief after the measurement; `needs = ["READ1"]` for the page sentence only. Plan:
`plan-2026-10-20-write1.toml`. Not folded into READ1: a different mechanism, already M–L.

## STEWARD13 design (2026-10-20, architect-6): the launch off the loop under the install lock

Brief: `.wash/local/STEWARD13-implementer.md`. Read before designing: `drive.rs` 60–180 (the
decide loop, `run`, `Done` pushed at :147), 429–525 (the guard, the `Launch` step);
`bin/steward.rs` 1025–1150 (`Machine::launch`); `libs/client/src/launch.rs` 173–303 (`start`,
`place`); steward.md 770–789 (STEWARD10's premise).

**The hinge.** The core already takes a batch's outcome as a later event, `Done { object,
result }` (`event.rs:123`), which the drive pushes right after `run`. So a batch can suspend at
its `Launch` step with no core change: the drive defers `Done` until a launch thread reports;
the core sees today's events in today's order per object.

**The premise re-shown, not weakened.** The thread's installs are `Process::create`, one
`Buffer::new` per page batch (consumed by `map`) and `process_start` — kernel calls, with the
erofsd read in the steward's own closure between them. Rule: one `Mutex` (the install lock) held
by the loop across its `now()`+`destroy` pair and by the thread across `start()` except inside
the read closure, which drops it (STEWARD5's "never across a server call"). No install can
interleave with the pair; the loop's worst wait is one between-reads kernel stretch, measured
and stated. Defence in depth: the thread creates no budget, so the silent form is unreachable
regardless; the lock removes the loud `WrongObject` form. steward.md's premise sentence grows
one clause. No `libs/client` change (the closure is the steward's); if an installing call has no
read to release around, it comes back to me as a design change.

**Shape.** Prepare on the loop (endpoints, handles — cheap installs), launch on one serial FIFO
thread (hand-over by reply, STEWARD5's arbiter), finish on the loop (watch, keep, `made`/`pids`,
remaining steps, then `Done`). Serial launches leave a caller's wait as today (the second call
was not even received before), so STEWARD8's bound stands; `served start_agent` now includes
the queue wait, said where the line is defined. Events of the launching object are held in
order and replayed after the report (default: no core change, today's order) unless the core's
oracle is shown to accept them between a batch and its `Done`; every other object is served.
Re-statement of protocol.rs's no-park: another lease's `end_lease` is still answered ahead and
at once; the launching object's own waits at most the launch bound. A deadline during its own
launch fails the launch as today (B82). Exit notices pending on the endpoint are kept, so the
later `watch_exit` is the same window as today, longer — confirmed from ipc.md, not assumed.

**Residual stated, not fixed:** a lease batch's mints (`Connect`) still run on the loop within
`SERVER_US` (1.66 s worst, STEWARD6), so a deadline inside a mint still waits. The overlap case
puts the deadline inside the launch — this package's quantity. The install lock makes a whole
batch off the loop possible later (a budget install under the same lock); a follow-up I name if
the measurement shows mints bite.

**Case.** A new boot case (working name `steward-launch-overlap`): lease A spinning with
STEWARD9's per-width derived deadline; B's `start_agent` at A's deadline − 1 s on the guest
clock so B's launch (≥ 1.7 s on every supported row) spans A's deadline; judged only when the
trace shows the overlap, else a loud failing line; run on main first to record the miss. Tier A,
M, steward-red-5. Cost: one thread, one lend page, `in_flight` +1 at erofsd if counted.

### SHELLN1 measurement (2026-10-20): the burst is NOT the shell's print — my ruling's cause was wrong

The listener split (receive, decode, queue to a printer; host-tested) moved nothing: first/last
notice before → after, D=6 rv64/1 595k/1424k → 526k/1556k µs, rv64/2 489k/1451k → 471k/1228k,
rv32/1 640k/1543k → 643k/1719k; D=16 rv64/1 673k/1842k → 872k/2175k, rv64/2 972k/1925k →
890k/2602k; gaps between send returns still 60–380 ms (D=16 one hart alternating 60–70 and
110–180 ms); one of sixteen dropped at NOTICE_US on rv64/1. By beamlet's code the serve thread
forwards each send to the VM's wake endpoint (FOREVER rendezvous) before receiving the steward's
next, and the VM takes wake-ups with a zero-wait receive from idle and from console_read on every
scheduler next(), so the take was never behind the print. Table: `.worktrees/SHELLN1/.tmp/SHELLN1/
burst-finding.md`. **My SHELLN1 ruling attributed the burst to the shell's print rate; the
measurement corrects it.** Candidates left: the VM not reaching next()/idle for 60+ ms (a long
native or slice at icount), the kernel not running the VM's thread after the serve thread's send,
the send's own cost.

Ruled: (1) yes, one diagnostic boot (rv64/1) with test-only guest-clock stamps at the listener's
receive and the printer's print beside the probe's send returns, to split "VM not taking" from
"steward/kernel side"; a measurement before any further ruling. (2) SHELLN1 is held until (1) is
in. The split lands only if it moves a measured quantity or a rule requires it; "correct in
itself" (a notice no longer waits on a re-bind or the relay) is a claim to measure in the same
boot, not a reason — tenet 1, deleting beats adding. If the take is within a turn whatever the
listener does, the split is reverted and SHELLN1 closes as a finding (the node's premise wrong,
the cause named with its node); if the VM's take is the lag, the fix is beamlet's wake/take path,
a beamlet node, not the listener; if the lag is before the VM's receive, it is the steward's send
or the kernel's scheduling of the receiver, a kernel/steward node. The burst cases' residual is
restated with the measured cause either way; the dropped notice (a sponsor never told) stays a
finding and lands with the cause's node.

### SHELLN1 lands + WAKE1 (2026-10-20): the burst is the VM's idle not ended by the forward

Diagnostic (rv64 both hart counts, guest-clock stamps; `.worktrees/SHELLN1/.tmp/SHELLN1/
burst-finding.md` "The diagnostic boot", `diag/timeline-smp{1,2}.txt`): per notice, send return
→ listener receive 43–193 ms; send N+1 returns about when recv N happens (the serve thread sits
in its FOREVER forward until the VM takes it, then receives the next); the print backlogs (150–
966 ms) and gates nothing; the hart is idle through the gaps (no runner 62 and 28 times in the
two longest); no wake record for the VM's budget inside the lag, only the periodic budgets'
timers. So the gate is the VM's take of the forward: the VM's idle is not ended by the forwarded
notice until a timer or console wake does. Not the listener, not the print, not the steward's
send. The re-bind benefit could not be provoked: unmeasured.

**Ruled. SHELLN1 lands as revert + finding:** the split is reverted (it never ran in a gate and
moved nothing; code that moves no measured quantity does not land), the test-only stamps are
gone (they are), the burst cases stay as STEWARD12 left them, and the page sentences SHELLN1 and
STEWARD12 wrote — "the sponsor's rate until the shell takes a notice before it prints the one
before" (steward.md "Failure and restart"/"Residual risks", scheduling.md) — are corrected: the
cost is between the serve thread's forward and the VM's take while the hart is idle, the VM's
idle not ended by the forward, named to WAKE1. That correction is SHELLN1's deliverable (docs +
the burst-finding file as evidence); the orchestrator closes the node on it. My SHELLN1 ruling's
cause was wrong and is recorded as such above.

**WAKE1 (the orchestrator ids it), investigation-first, Tier A** (beamlet's platform idle/wake
path; kernel if that arm bites; beamlet's strict reviewer). First make the session VM's budget
id readable in the trace (SHELLN1 could only infer it), then exclude in order: (i) beamlet — the
idle receive waits on an endpoint other than the forward's target, or is a timer wait that
`take_completed`'s zero-wait receive polls only when the VM runs; (ii) kernel — a receiver
waiting with a deadline not woken at once by a send (cite or add the ipc case that shows it is,
before touching beamlet); (iii) the serve thread not runnable between receive and forward. The
wider claim to measure: every completion to an idle VM (a call's reply, console input) would
carry the same wait — one before/after of a reply to an idle shell. Acceptance: recv − send at
the kernel's delivery, the last of N within 2× worst rounded up, no drop at NOTICE_US (the loss
was the take), the VM's wake inside the send in the trace. Plan: `plan-2026-10-20-wake1.toml`.

### READ1 gate + KILL1 (2026-10-20): a lease's clean end kills a neighbouring lease

READ1's full set is green (88 cases both widths, 162 PASS; lease-bad-start both widths). Its gate
`lease-code-load`: in every completed boot sixteen agents launch (2.3–3.3 s), load all 27 modules,
fifteen exit 0 in load order, and the sixteenth is `was killed 0` the instant a neighbour's lease
ended (rv64/2: agent-14 exits 0 → LeaseEnded → `agent-11 was killed 0`; rv32/1 agent-14's end
kills agent-15; rv32/2 agent-15's kills agent-14). `killed` is R10's word (budget.rs:1383): a
budget destroyed with its process in it. So one lease's DestroyBudget reached another live lease's
budget with the deadline still ahead — the silent wrong-destruction STEWARD10 guarded only for
deadline-killed leases (t ≥ d), here on the process-exit end path where the witness passes (t < d,
Ok on an index that no longer names the record's budget). rv64/1 failed differently (the guest
closed the SSH connection after the sixteenth launch, the shell idle, no process died): rerun
alone; a reproduction is a second finding, named. Evidence `.wash/local/READ1-gate-finding.md`,
`.tmp/READ1/evidence/gate10/`.

**Ruled (b): READ1 lands now.** Its claim is verified — sixteen launched, none refused, every
module loaded at the derived caps — and the kill is another rule's failure (R39: a lease ends at
ITS deadline or ITS sponsor's word; closure: a destroy reaching an object the step did not name).
`lease-code-load` lands unchanged (the forbid on any non-zero end is its claim; no "fifteen of
sixteen" acceptance), excluded from the whole run by the bench's existing means (the implementer
states which) until KILL1 lands, when it is KILL1's acceptance too; the pages' rows state fifteen
of sixteen and the finding by name. Not (a): the bug is on main regardless; holding the Files-cap
fix buys no safety.

**KILL1 (the orchestrator ids it), top priority, Tier A, steward-red-5 and kernel-red-3 (one arm
each), investigation-first.** Reproduce on the fake kernel first (sixteen leases ending by process
exit close together, the fake kernel asserting each destroy hits the budget created for that
step's lease — the kernel's verdict). Arms in order: (i) a lease's budget handle index closed or
revoked before its end (a wrong close in `give_back`, the held copies, the exit endpoint, the
notices right, `forget`; a scope destroy revoking it) and the index reused by a later lease's
budget, so the earlier record names the later budget — consistent with agent-14's end killing
agent-11 if agent-11 was created after agent-14's handle was lost; (ii) the exit watcher's
index→owner map (`pids`, keyed by exit-endpoint index) stale across a reused index; (iii) the
kernel placing or charging a process to another lease's budget. STEWARD10's "display artifact"
(`agent-N was killed 0` twice for two ids) is likely this bug's first sighting and is
re-examined, not cosmetic. Rule to hold by construction on every end path: no destroy by a kept
handle reaches an object other than the one the record was made for — the witness extended to
identity or the record re-validated, as the cause decides; a kernel arm comes to me. Plan:
`plan-2026-10-20-kill1.toml`. **STEWARD13 shares drive.rs's destroy path:** the orchestrator orders
them (KILL1's reproduction on main first; STEWARD13's implementer told the finding, since the
install-lock argument assumes a record's handle names its budget until the deadline — the very
assumption this breaks).

### KILL1 phase 1 (2026-10-20): the steward's arms excluded; the kernel arm stands

`.wash/local/KILL1-identification.md`, WIP 04235700e. (a) A Recorder `recycles` mode models the
table whole; sixteen leases ending by exit in the gate's three orders, the fake kernel asserting
each destroy hits its own lease's budget: PASSES on main; a mutation closing the budget's own copy
at launch fails at once (teeth). (b) Arm (i) excluded (every `close(` closes from its own record;
the victim was created BEFORE the killer in two of three boots, so no record of the killer's can
name a later budget); arm (ii) excluded (pid = exit-endpoint index, closed only in the EXITS
handler right before `exited`, the lines right at every report). Arm (iii) stands by elimination
and by the log's order: `killed` comes only from `destroy_subtree`'s kill walk; in every normal exit
the kernel's `Terminating` precedes the exited line; for the kill, `Terminating PID 60` comes
AFTER the killer's audit, holds (one lease's carve exactly) and its `[lease ended]` — after the
killer's whole event, destroy included. The victim is always the lowest lease in start order still
running; deadlines normal. STEWARD13's premise: nothing false. STEWARD10's display artifact: no
surviving log.

**Ruled:** the kernel arm, Tier A kernel, kernel-red-3 reviews; steward-red-5's arm is closed.
Evidence next, in this order: (1) the kernel model first (docs/kernel/model.md): sibling budgets
with a process each under one parent, destroyed one at a time in the gate's orders, the oracle
asserting no process outside the destroyed subtree is killed — if the model reaches it, that is
the kernel's own verdict and the fix's test; (2) if the model cannot reach it, the test-only kernel
probe as proposed (feature-gated as `latency-probe` is): each destroy's top budget id, each killed
PID's counted budget (frame and generation), and the killer's and victim's positions in the
parent's children list (the victim being the lowest still-running sibling suggests the kill walk
leaving the destroyed subtree — a sibling link read after unlink, or a process matched to a
budget by a reused slot/frame rather than its generation). The probe is removed or kept behind
the feature per the kernel's convention; the lease-code-load boot with it is the measurement. The
severity stands (a destroy reaching a sibling's process is R10/closure); a fix lands with the
model case that would have caught it.
(Correction: kernel-red-3 rotated at 70 %; **kernel-red-4** reviews KILL1's kernel arm.
kill1-implementer-2 launched on the evidence order; KILL1 rebases onto train 80 (READ1 + SHELLN1),
which brings lease-code-load into its tree.)

### STEWARD13 checkpoint (2026-10-20): the design holds; three notes

Host tests green. `Kernel::launch` → `Launched::{Started, Pending(job)}`; the drive stops at a
pending Launch (`Pending {batch, step, produced, job}`, FIFO); `launched(job)` finishes the step,
runs the remaining steps (none in any table's batch), pushes `Done`, replays the object's held
events in order. Held set: `Exited{object}`, `EndLease{lease==id}`, `ChannelClosed{session==id}`,
`SshdGone` while a session launch pends (its Detach would hit a Starting session). The core serves
LockedOut, Idle, Attach (refuse_in_use) and a session's EndLease (refuse) between a batch and its
Done; nothing in Starting emits a second batch. Each call gets its own reply token; a call with no
reply yet is parked and answered from the Done's/replay's `Output::Reply` into its own lend; served
start_agent said at the late answer. Lock: `Kernel::without_installs` wraps the now()+destroy pair;
the thread holds the Mutex across start() and drops it inside the read closure; no libs/client
change. Measured: lock wait max 33 µs rv64/1, 50 µs rv32, 5.2 ms rv64/2; thread stack 4,840 /
3,808 B → 3 pages; loop peak 18,536 B → 10 pages; entry 1,027 → 1,051. Overlap case: before
0.36–0.58 s net on all three boots; after p99 20/36/19 ms net, every deadline inside a launch
window. Launches overlapping an exit grew 0.3–0.5 s.

Notes (not objections): (1) the loop's stack peak GREW with the launch moved off it — explain
in the report what holds it now (the pending state, the replay) — a growth where a shrink was
expected is stated, not just declared. (2) The launch bound is derived (2× worst rounded up); with
overlapping exits adding 0.3–0.5 s, re-take the worst launch over both widths and show whether
the arithmetic still gives 8 s — the number follows the measurement, a target does not. (3) The
5.2 ms two-hart lock wait goes on the page as the measured cost beside the 50 ms timer-wake p99.

### KILL1 cause (2026-10-20): not a wrong destruction — the virtual clock reached the lease's deadline

`.wash/local/KILL1-cause.md`. (1) The model's sibling contract (16 children ending by exit in the
gate's three orders and two more) passes; the model cannot reach a walk leaving a subtree. (2) A
kernel probe (feature `kill-probe`) on lease-code-load rv64/2, three boots: the destroyed top IS
the victim's own budget, its chain exactly the victim's process, the parent's list intact,
`deadline true`. R10 and R39 hold: the lease ended at ITS deadline. (3) The deadline is start +
1.8e9 µs as set; the guest clock jumped to it (audit before `at 575133051`, expiry `now
1912083242` for `deadline 1912083218`; 295 s wall) with every hart idle and nothing nearer armed
(hint = this deadline). QEMU icount skips an idle guest to its next timer deadline; testbench.md
says `sleep on` keeps idle time the host's, and the jump happened with sleep on. The victim is the
oldest lease because its deadline is earliest; the kill follows a neighbour's end because that is
when the guest first idles with only lease deadlines armed. (4) The same case on the host clock:
PASS in 78 s, sixteen exited 0.

**My KILL1 ruling framed a wrong destruction; the measurement says the kernel and the steward did
exactly what R10/R39 say. Recorded as corrected.** The finding is the bench's: under the virtual
clock a fully idle guest with only lease deadlines pending reaches them, and the page's `sleep on`
claim is false as measured.

**Ruled.** (a) lease-code-load runs on the host clock (a `forward` case by testbench.md's own rule
for what such a case keeps), the reason in its comment: it judges counts (sixteen launched, loaded,
exited 0), not timings, and under the virtual clock an idle guest reaches a live lease's deadline;
`whole_run` back on. Not another shape: arming a timer to defeat the skip would be designing to
pass. (b) testbench.md's `sleep on` sentence is withdrawn and replaced by the measured behaviour
(an idle guest's clock skips to its next armed deadline, sleep on or off, on the bench's QEMU),
plus the rule it implies for case authors: a case under the virtual clock whose guest can idle
with a live lease either runs on the host clock or expects the lease's end. (c) The probe is
removed (tenet 1: it served its identification; KILL1-cause.md keeps how); the model's sibling
contract stays as a kept test with teeth. (d) The rows READ1 wrote ("fifteen of sixteen, R39 by
mechanism" on erofsd.md, scheduling.md, steward.md Residual risks) are corrected to sixteen of
sixteen on the host clock with the clock finding named to testbench.md; SECURITY.md unchanged.
KILL1 closes on that (Tier A for the kernel test file; kernel-red-4). STEWARD10's "display
artifact" stays unexplained, not attributed to this.

### WAKE1 cause and fix (2026-10-20): the serve thread's blocking forward, not the VM's idle

`.wash/local/WAKE1-identification.md` (rv64/1 burst boot under block-trace + shell stamps). The
session VM's budget is read (a `#` block-trace record at budget_create: id, parent, pid). My arms:
(i) excluded (idle receive and forward share one endpoint, frame 31395); (ii) excluded (a waiting
receiver is woken in the sender's entry; idle wake → pick 2.9 ms median, 10.6 worst); (iii) not as
stated (the serve thread is runnable, 1–38 ms to its pick). **What holds: the VM is not idle in
the burst** — runnable throughout, the hart never idles (SHELLN1's "idle" was the return to kmain
before a pick); 61 % of the window is the checked kernel's audits, the VM 26–28 % of the rest; a
busy VM takes a forward only at its next scheduler turn (a zero-wait poll per 2,000-reduction
slice): blocked forwards waited 107 ms median, 126 p90 — SHELLN1's 43–193 ms. The serve thread
forwards each notice with a FOREVER rendezvous and only then receives the steward's next, so the
steward's send waits on the VM's poll and drops at NOTICE_US (the lost notice). On main's unsplit
listener recv − send is 76–866 ms, +~300 ms a notice where the inline print is attributed; with
the split the printer still spends the same VM reductions, so totals did not move (SHELLN1's
table) — the revert stands. (c) An idle VM's reply carries no wait (File.read: 2.8 ms of ~1 s);
busy, forwards block 3.4 ms median (worst 102/298 ms).

**Ruled: the fix is the hub's forward, not the VM's cadence.** The serve thread never blocks on
the VM: completions and notices go into a bounded queue of the hub's, the VM woken once (a
non-blocking wake on the shared endpoint — R13's reply shape, or the existing poll finding the
queue), and the serve thread goes straight back to its receive. Bound stated: the hub's call pool
(one completion per outstanding call) plus the notices the steward can send a session (its live
leases by the carve term, `server_restarted`, approval), the arithmetic on beamlet.md; a full
queue blocks the forward as today, stated as the residual. Consequences: the steward's send is
taken at once whatever the VM does (no drop at NOTICE_US unless the serve thread itself is gone);
a burst's N notices are all queued before the VM's next poll, so the last of N costs one slice,
not N (the burst cases then assert a derived bound, 2× worst rounded up, and the
deadline_notice-under-burst residual is replaced by the measurement); an idle VM is unchanged
(2.8 ms). Not: the VM polling more often (its own CPU, and under the checked kernel's audits the
slice is the cost — a bench condition, stated), not a target move. The 2,000-reduction slice at
27 % share under audits is the bench's shape; release has no audits — state the row's condition.

Instrumentation: the `#` budget_create trace record STAYS (behind the existing block-trace
feature, one record per create, the identification depended on it; documented on the trace's
page); the shell stamps go; analyze.py/chain.py stay in the worktree's .tmp, not the tree.
Tier: beamlet's hub (strict beamlet review), size S–M; pages beamlet.md (the completion queue,
its bound), steward.md "Leases"/"Residual risks" (the NOTICE_US loss sentence), scheduling.md
(the burst row). Confirm with the fix on main's unsplit listener that the print no longer gates a
take (all N taken at one poll, prints after), so SHELLN1 stays closed.

### WRITE1 identification (2026-10-20): the hang did not exist — the premise withdrawn

`.wash/local/WRITE1-identification.md`, `.tmp/WRITE1/repro1..7`. Seven boots, 68 agent VMs
writing to /project beside loads (N = 4, 12, 16; rv64/1), every agent under a watchdog naming the
step and the hub's state after 120 s without progress: no watchdog fired; every File.write
returned (:ok or {:error, :etimedout}); walfsd says every write it serves; on the host clock
(N = 12) all 12 exited 0, 224 of 348 writes served, the rest etimedout at the hub's give-up. None
of my three outcomes: the hub answers its caller at the bound. diag4/4b (virtual clock, before
READ1's fix) were misread — the file ends at the line BEFORE the stalled step, so that write's
bytes never reached the volume — and train 81's rule (an idle guest's clock skips to the next
armed deadline) covers them, showing here as 11 leases killed at their deadline and two sessions'
sockets closed. **My WRITE1 node was framed on READ1's inference ("the file holds the write's own
lines"); the inference was wrong and so was the node's premise. Recorded as corrected.**

**Ruled.** WRITE1 lands as the rule, not a fix of a hang: (i) **in** — a File.write is 5–6 hub
requests, each bounded by FILE_WAIT_US, so a write's worst is their sum (47 s seen before
etimedout); files.md states the one-answer rule PER REQUEST (every hub request ends in exactly
one answer to its caller within FILE_WAIT_US plus the reply margin) and that a File operation's
bound is the sum over its requests; held by a host test. (ii) **out as code** — the VM's asker
waiting FOREVER on the hub is right: the hub answers by construction (the rule above and its
test); a VM-side timeout would be a second mechanism for one rule and would hide a hub bug as a
VM timeout (one obvious way; size for the need). State it as the reason the asker has no bound.
(iii) **in, by construction** — a give-up on a walked fid hands the result to the caller FIRST
and clunks the fid after, off the caller's path (the clunk is cleanup: the same shape as R41's
"the destroy is the cutoff, cleanup only frees records" and STEWARD5's thread); today's
clunk-before-handover makes the caller's bound 2× FILE_WAIT_US, a violation of (i). Host test:
a give-up answers within FILE_WAIT_US + margin while the clunk's server never answers. Pages:
walfsd.md's hang sentence (READ1's stated finding) replaced by the correction (the stall was the
virtual clock reaching a lease's deadline; no write's completion was lost); the throughput
sentence KEPT (224/348 served under 12 writers is decision 3's real finding, a node only if a
write SLA is needed); files.md as above. Case: the boot case on the host clock, every write
answered (:ok or etimedout) and the file's content exactly the :ok lines — small; the watchdog
infrastructure goes. Not the diag4 rebuild on the pre-fix tree: the file evidence and train 81's
rule suffice. Tier A (the hub, strict beamlet review), size S; the node's title and body are
corrected by the orchestrator to the rule.

### WAKE1 landed; its four findings (2026-10-20)

WAKE1 (5ca6ccd96, kernel-red-4; `.wash/local/WAKE1-report.md`): the hub's bounded forwards queue
(128 = 2 × (32 parked + 32 sends)) and one wake; sends return 37–131 ms apart (the steward's
per-exit work), last of six 1.11–1.28 s (was 1.53–1.79), of sixteen 1.83–2.51 (was 2.11–2.76),
none lost; burst bounds asserted 3 s / 6 s.

1. **The shell's inline print (250–370 ms a `[lease ended]` line) is now the visible term.** Not
   a node, not SHELLN1 revived: the split would move the listener's receive stamps (not a judged
   quantity) and nothing the sponsor sees — the lines still print serially at the same cost; the
   steward's send and the loss are already fixed by the queue. The finding is the per-line cost
   itself (40× consoled's 6.5 ms per reply): a follow-up whose entry condition is one measurement,
   the cost of a shell print at idle on the release build and where it goes (VM reductions vs the
   relay vs consoled); a shell-track node if it is the print path's own. The listener's take rate
   matters again for the M5 harness (session code reacting to lease ends): owner Q5's wave.
2. **The steward's per-exit work (37–131 ms a send) is the remaining serial cost → a node.**
   STEWARD12's notices-first was built, measured at 16 ms of 1.4 s and not kept because the
   forward was the gate; WAKE1 removed that gate, so the same change would now move a burst's
   last notice from N × (37–131 ms) toward N × a send's return. Investigation-first, Tier A
   steward, S, steward-red-5: re-apply 57bff2058's shape on WAKE1's base, measure the burst
   cases; land if the derived burst bound shrinks materially, else record. The kernel's N
   destructions before the first notice (~590 ms) stay a kernel residual.
3. **`./test-shell`'s console tests hang 1 in 10 on main → a node** (tenet 6: a flaky test is a
   bug, fixed, not retried). Shell track, Tier B, S: reproduce at 1 in 10, fix the test or the
   shell, state which.
4. **`echo.ex` unformatted on main, the fmt gate not covering it → a node.** The gap is the
   gate's: `set CASES=formatting` covers every .ex/.exs in the tree (the agents' modules
   included); echo.ex formatted in the same change. Bench, S.
(Ids: (2) STEWARD14, needs WAKE1, steward-red-5; (3) B90, shell track, steward-red-5; (4) BENCH2,
kernel-red-4. (1) a recorded follow-up beside owner Q5.)

### B90 cause (2026-10-20): the fake kernel's held clock, not the shell

`Fake::tick` counts a thread as in a wait until it reacquires the lock after its condvar returns,
so a thread the change already notified still counts; on a loaded host the VM thread takes all
ten ticks before the waiter is scheduled, and the test stalls in Drop on a console write whose
busy answer was handed over after the last tick. B38's rule ("a tick ends the waits the test
wrote it to end") was stated, not built. Fix in `libs/rt/fake` only: every change bumps a
generation and zeroes the waiting count; a waiter re-counts itself only when it waits again; a
tick waits until every notified thread has resumed. A fake test failing on main, the consumer
sweep, B38's rule restated on testbench.md and the fake's doc comment. Re-tiered A (shared test
infrastructure), kernel-red-4. No objection. One check: the tick's wait on "every notified thread
has resumed" must be bounded by construction — a notified thread either reacquires or exits, and
an exit must account as a resume, so no tick can wait for a thread that will never come back;
the fake's doc comment states it and a test shows a notified thread exiting does not hold a tick.

### STEWARD13 rv32 one-hart N = 1 decision_wake (2026-10-20) → SCHED5's second row

`.wash/local/STEWARD13-finding-rv32-decision-wake.md`. Every gate passes both widths but one
cell: steward-latency rv32/1 N = 1 decision_wake net p50 30.4 ms (p99 33.4) against 25 (page
before: 24.2/28.5); N = 2/3 1.2/1.0 ms, rv64 both hart counts met. Bisected: the same tree with
the launch inline on the loop gives 23.3 ms, so the shift is the launch off the loop, at one
spinner only. In BOTH forms the wake is pathological: a 30 ms receive timeout sleeps 53–62 ms
with one lease VM running, 1–3 ms beside a second VM; the steward does nothing in the window.
The thread moves the loop's receive ~10 ms earlier relative to the VM's first run.

**Ruled (a), the node being SCHED5, not a new one.** The signature is SCHED5's (rv32, a timeout
wake with exactly one runnable VM: its N = 2 two-hart timer_wake is one spinner on the other
hart): a sleeper's timeout not ending the running VM's slice — the timer apparently armed for the
slice's end, not the nearest timeout — surfacing only when nothing else switches. SCHED5's scope
gains the row and the sharper hypothesis; its measurement is the timer programming against the
slice with one runnable VM. The rv32/1 N = 1 decision_wake cell is recorded, not judged, the toml
and scheduling.md stating the target, before/after, the bisect and the owning node (STEWARD9's
last-resort form, a cell never the table; STEWARD4 Q9's precedent). **Not (b):** moving
process_start back onto the loop to restore the phase would be designing to pass an anomalous
wake — the inline 23.3 ms was 53–62 ms of sleep landing a sample under the target by phase; and
bisects of the steward's phase cannot change the owner. The 20 ms timer wake at 2.2–3 ms
threaded (0.8 inline) is within its 15 ms p50, stated. Notes closed: the loop's stack growth is
the late-answer path (boxing the pending batch, 18,472 B); the worst served start_agent 4.82 s
under the 6.64 s the bound derives from — the bound stands.

### Two bench facts (2026-10-20): smp-evict-mttcg's flake → a B node; walfsd timed steps under load → the condition, not a node

1. **smp-evict-mttcg fails ~1 in 8 alone** ("guest exited while waiting for the shootdown line,
   QEMU exited with status 0" at 1.1–1.2 s). A flaky test is a bug (tenet 6): **a B node**,
   investigation-first, shell-free. Three arms, excluded by the failing boot's full console: (i)
   the line is in the captured output but after the bench stopped reading — the bench's console
   reader does not drain to EOF after the guest exits before judging (bench, Tier B); (ii) the
   line is absent and the guest exited 0 — the kernel case ends on the primary hart without
   joining the hart that prints the shootdown line (a kernel-case ordering bug, Tier A,
   kernel-red); (iii) the line was written but the UART was not drained before the shutdown call
   (the kernel's exit racing its console flush — fail loud means the last line is flushed; Tier A,
   kernel-red). Whichever: the fix by construction (drain to EOF; join before exit; flush before
   shutdown) with the case rerun at the 1-in-8 rate shown green over enough boots to say so.
2. **Train 82's three walfsd latency steps failing under the whole bench beside three members'
   boots, passing alone twice both widths: not a node.** The timed cases' bounds are the box's
   under the stated condition — one whole bench at a time, QEMU exclusive during it (the owner's
   rule of 2026-10-05; members' host-only work pinned to the other cores) — and the boots beside
   the train broke the condition. WAKE1 publishes on the rerun evidence, rightly. Two
   consequences: the condition goes on testbench.md beside the timed cases' bounds if it is not
   there (a sentence; BENCH2 is live and may carry it), so ATK11's bounds are read as "under one
   bench alone"; and if `q` cannot express "a whole bench holds QEMU" today, a Tier B tooling node
   (S) to make it so is justified by this being the recurring failure mode — the orchestrator's
   call, knowing q. The walfsd timed cases flaking first under sharing is expected: they are the
   bench's tightest timed bounds, which is what makes them the gate.
(Ids: (1) B91, investigation-first, arm to me before the fix. (2) B92, Tier B tooling S: the
whole-bench targets hold a machine-wide qemu lock via `q run --lock`, single boots wait with a
line naming the train, host-only cases exempt; the condition sentence rides with B91 unless
BENCH2 carries it.)

### B91 arm (2026-10-20): the kernel case's ordering; spin on the writer's progress, not the clock

`.tmp/B91/phase1-arm.md`. Not the reader, not the flush: the console runs to TEST PASSED; the
shootdown line is absent because the kernel never printed it — ptable's evict prints only when
`hart::shootdown` asked a hart, and at the destroy no other hart ran the writer. The judge's
`wait_ms(5)` is a receive timeout whose deadline re-arm lands on the first hart to re-arm, often
the writer's at its 1 ms slice end; kmain's expiry wakes the judge there and the pick gives it
that hart; the writer is Ready on no hart while wake_idle IPIs the idle hart; the judge destroys
at once, racing that pick. Identical under icount, so only MTTCG shows it.

**Ruled: the mechanism is right, the condition is not yet.** Spin, not sleep — the judge holds
its hart by construction, no deadline another hart can serve — is the right mechanism. But a 5 ms
clock alone asserts what the case must observe: that the writer is running on another hart at
the destroy. Order on the event: the writer publishes its progress (a counter it bumps each pass
over the mapping, with its hart id), and the judge spins (`sched::spin_until`'s shape, no sleep)
until it has seen the counter advance — two increments, so the writer is actively running
elsewhere now, not merely fetched — then destroys; the clock is only the failure bound (the
case's own timeout, loud). With one runnable thread on the other hart and the judge spinning on
its own, nothing displaces the writer between the observation and the destroy, so the shootdown
is asked by construction and the line is deterministic under MTTCG too. smp-evict.rs only, Tier
A (a kernel case), kernel-red-4, no kernel line moves. Acceptance: the 40-boot baseline's failure
rate, then 40 boots green after, both widths at two harts; the case's comment states why it
spins and what it waits for (the memory rule: order on events, not the host clock).

### WRITE1's gate finding (2026-10-20): a give-up leaks its fid; the clunk follows the request

lease-writes (sixteen writers, host clock) lost 1–3 agents in 6 of 10 boots; with the loud exit
the status names it: exit 104, the input read refused `emfile` six times. Mechanism (files.rs
`give_up`): a request given up on IN FLIGHT has its fid "retired as a lost reply's is" and is
never clunked, so every etimedout write under the data volume's throughput (decision 3's dozens
a boot) leaves one fid held at walfsd and one burnt in the client's pool; sixteen agents'
timeouts drain the account's working pool (71 less twenty roots) until walfsd answers `too many
open files` to whoever walks next — the late starters' first read of /project/in.

**Ruled: in WRITE1, Tier A as the rest — the same rule's second half.** Every hub request ends in
one answer to its caller AND every fid it walked is let go: a give-up on an in-flight request
queues a Clunk for its fid as a closed file's (Owner::Close), after the request in the
connection's order; the server serves in order, so the clunk lands after the late answer (a failed
Rwalk leaves no fid and the clunk's `unknown fid` frees the client's); the pool recovers. files.md's
"retired" sentence becomes "clunked after it". Host test: a request unanswered past the bound,
then let go — the server's clunk count rises and the fid is reused. Not a lower N (designing to
pass), not an agent tolerating `emfile` (hiding a leak). **One check:** the claim "the server
serves in order so the clunk lands after the late answer" must be a rule the serving library
states (serving.md, per-connection order) — cite it; if a server may answer out of order (parked
completion calls at walfsd/littlefsd), the clunk is instead issued when the hub takes and drops
the late answer (the hub knows that moment), so it never precedes the request it closes. Either
way the sentence on files.md says which.
(Landed in the second form: the mux serves in order but goes on past a request the file server
holds (ninep_mux.rs fill), so the hub keeps a given-up request as Owner::GivenUp and clunks at the
moment it takes and drops the late answer — a walked fid as a closed file's; a walk in flight
clunked only if its Rwalk walked every name, else its number returned; an open handle's fid never
the operation's. files.md says so; host test a_fid_given_up_on_is_clunked_when_its_late_answer_comes
fails on the code before. Head 88ce8464e, train 84.)

### State (2026-10-20): trains 84 (B91) and 85 (STEWARD13 + B92) published, main dc75a30eb

q holds the qemu lock for a train now. STEWARD13's reviewer-noted "cosmetic" usage-line
deferral proved a regression in train 85 (a sponsored session carves from its sponsor's
sub-budget, so the sponsor's line printed before the launch's report); fixed (the deferral keys
on the carve's domain). Lesson for my notes: an ordering change in the steward's launch path is
never cosmetic — a line's place is a claim the bench reads. SCHED5 phase 1 launched: both rows on
main with the kept trace, the arm from the timer's arming against the slice, to me before any
kernel change; its brief follows that measurement.

## Owner's answers 2026-10-20 → WEIGHT1, IPD2 (architect-oct20)

- **Q1 → WEIGHT1.** The image's steward at 1,000 under `system`, and every manifest copying the
  image's steward entry. init.md "Weights" already says 1000: the image was the outlier. Brief
  `.wash/local/WEIGHT1-implementer.md`. Tier A (system weight, R12), S, kernel-red-4.
- **Q2 → a line in WEIGHT1.** "No launch weight" becomes a rule sentence with its reason on
  steward.md "Residual risks" (the launch bullet): the pages state the launch's measured share but
  not the decision.
- **Q4 → IPD2.** The recommendation said the mechanism existed (`limits=` per scope badge); it does
  not reach a session: `limits=` overrides account-0 root badges only, and a session's `/net`,
  minted by the steward's grant, is admitted in the session's account bucket from its first
  request (serving.md). Ruling: an account override, `account=ACCOUNT:INFLIGHT:STATE:SOCKETS` in
  `ipd`'s args, applied to the account's unlabelled bucket (ipd refuses labelled callers, R60);
  the serving library's `Override` gains the account key, the worst-case rule unchanged; the share
  rule within the bucket unchanged; the number lives in ipd's args alone (the manifest is the one
  source, as for sshd and the steward), no principal key, no init/steward change. The image:
  `account=1001:5:4:32`. Fids bind live sockets at 64/2 less dirs; TIME-WAIT holds no fid. The
  heap cap's claim to hold every socket's buffers is checked against max_sockets × 17,408; a gap
  today is a finding first. Size M, not S. Brief `.wash/local/IPD2-implementer.md`.
- **Q5.** No line needed: agents.md (harness, loop, escape room "planned · M5") and
  m5-self-hosted.md (steps 2, 6, 7) already place them. Noted for the editor, not a node: agents.md's
  "Delegation only narrows", "Labels and vaults", "No credentials in agent memory" still read
  "planned · M3" though much is built (sub-agents, labelled-agent-no-sink); a status-line pass.

## IPD2-sizing: the heap cap does not hold the sockets (architect-oct20)

Finding (.wash/local/IPD2-sizing.md): ipd.md says the heap cap holds every socket's buffers;
it holds about 8 above the idle peak, and past that `new_socket` panics: a user can crash a
shared server, and the image's own defaults (alice 3+3, bob 5) reach it. Ruling: options 1 + 3,
inside IPD2 (not a separate B node: IPD2 is the package that rewrites this arithmetic, and its new
case is the attack that crashes ipd on main).
1. **Count each socket once, where it lives.** ipd refuses to start unless heap_pages x page >=
   the base (the measured working set at twice its peak, 74 pages today) + max_sockets x
   SOCKET_BYTES. In the same change the budget term costs a `State` unit as a connection
   (CONNECTION_BYTES), not as a socket: the buffers are the heap term's. If that Cost change
   reaches past ipd's sizing, keep the double count and name it a residual on ipd.md instead.
   Every manifest with an ipd entry gets heap_pages derived the same way; the bench's
   per-entry manifest check counts the heap term. Owned paths widen to those manifests and that
   check.
2. **A backstop, not a channel.** `new_socket` reserves fallibly: a failure answers `too_many` and
   prints one ipd line saying an invariant broke. With (1) that line is unreachable, so the boot
   cases forbid it.
3. **Pages.** The rule and its arithmetic go on ipd.md "Sizing", with "Bad arguments, or limits
   that do not fit its budget" extended to the heap; testbench.md's memory table gets ipd's row;
   SECURITY.md gets an R26 row if a row names this crash. Size L.

## SCHED5-cause: a user call's kernel section is at most about a slice (architect-oct20)

Finding (.wash/local/SCHED5-identification.md): both rows are one kernel section, the stub's
`map_fixed` of beamlet's 835-page text (39.8 ms on rv32, checked), held with interrupts off; every
range call is linear in its pages at ~48 µs a page on rv32, so any VM's large map, unmap or load
holds every wake (and every other hart, R78) for its length. R12 permits the linear term today.

**Ruling: option (a), by construction; (b) is not in SCHED5; (c) only for R10, which already has
its own target.** The principle (TENETS: responsiveness is a measured target under a named
workload, but what a hostile caller can hold the kernel for must be bounded by construction, as
`MAX_LEND_PAGES` 16 bounds a lend for the same reason) is: **no call a process can make holds the
kernel longer than about a slice.** So:

1. **Every call that takes a range** (`map_anon`, `map_fixed`, `unmap`, `set_flags`,
   `process_map`; lend and transfer are capped already) refuses a range over `MAX_RANGE_PAGES`
   pages by arithmetic before any walk or charge, with the error a lend past `MAX_LEND_PAGES`
   gets (a new kind only if the caller cannot tell it apart where it matters, and then the
   contract names it). An ABI constant beside `MAX_LEND_PAGES`; if the derivation gives 16, the
   simplifier says whether one constant serves both, with both reasons on the page.
2. **The derivation, not the number:** the largest power of two whose worst section (a
   `map_fixed` whose every page table is absent, so the tables are allocated in it) measures
   within one `SLICE_US` (1 ms) on the slower width in the checked build. Today's per-page cost
   says 16 (0.76 ms; 32 is 1.5 ms); the implementer measures and states it on memory.md R22 with
   the per-page figures. A wake then waits at most a slice and a section of a slice's order for
   anything a user can do. R12's sentence changes: the linear term becomes "at most
   `MAX_RANGE_PAGES` pages, a constant"; R10 stays the one named exception, bounded by its target.
3. **Callers chunk.** The stub maps each segment in chunks (one `map_fixed` per chunk; a chunk's
   `OutOfMemory` fails the child as today). The rt heap keeps a large block one contiguous range:
   it may `map_anon` the first chunk and extend with `map_fixed` at the following addresses
   (memory.md allows `map_fixed` in `map_anon`'s area), undoing the chunks already mapped when one
   fails; `unmap`/`set_flags` loop. Grep every caller of the five calls (libs/rt, stub, beamlet's
   platform, tests/programs); the fake kernel enforces the same cap so a host test meets it.
4. **The model** gets the rule in its contracts (R22's), a mutation that ignores the cap, and no
   extra RNG draw in gen.rs (the memory rule: a lens drawn `any()` is now refused when over the
   cap, which is the contract). Mutation deadlines re-checked.
5. **Cases:** the ABI refusal on both widths; an attack case: a hostile process issuing range
   calls at the cap in a tight loop while the sched-latency stand-in's wakes are measured, the
   wake targets met (the verdict the stand-in's); the kernel's memory set re-run
   (`map-fixed-attack`, `map-anon-search-bound`, `map-fixed-tables*`, `scan-bounds`,
   `process-map-*`); the launch's cost re-measured (835/16 calls at ~12,000 instructions an
   entry is ~5 ms on a ~4 s launch, stated); the two rv32 cells re-measured and judged, no target
   moves; the model's host tests and mutation sweep; the memory cases' heap peaks (the chunked
   heap must not raise a cap).
6. **(b)**, the three walks a page: recorded on memory.md's residuals with the per-page figures as
   a cost, not a bound; a node of its own only if the launch time is wanted lower. Not here.

Tier A (kernel, libs/rt, stub, model), size L, reviewers kernel-red-4 + simplifier + editor.
Pages: memory.md R22 and the five calls' table, scheduling.md R12 and "With the real steward"
(the cells), budgets.md's R10 sentence if it cites R12's term, the ABI constants where
`MAX_LEND_PAGES` is documented, SECURITY.md's R12/R22 rows.

## B95-cause: the mux's session bound runs from the answer; a dead instance's state at a restart (architect-oct20)

Finding (.wash/local/B95-identification.md): `pump` sets a session's deadline from a `now` read
before `fill`, so a fill of the bound or longer (walfsd's is COLLECT_WAIT, 10 s) ends the session
at its answer; the hub reads the next OPENED as the connection's end and the VM's volume is eio for
its life; the stranded fresh session outlives the VM, and init's restart of beamlet, handed the
same badge number, lands its connect on it (empty answer, not OPENED: Unexpected, eio).

Rulings:
1. **(1) now, in B95.** The deadline, and a pending request's timeout, are judged at the time
   the answer is sent: the stated rule (serving.md "The end": "since the last returned"; the host
   test "its whole bound from that answer"). A host test on the fake kernel's held clock: a fill
   longer than the bound leaves the session its whole bound from the answer. Every bin linking
   redoubt_rt is built and its host tests run before any boot (the runtime-change rule).
2. **The natural load run is required** (the orchestrator gives the go: the beamlet set as B93's
   passes, diagnostic lines on, logs kept), because the design answer differs: (a) a fill ≥ bound
   is closed by (1); (b) a live waiter not re-calling within the bound under load means the
   session bound misfires on a live client, and the answer is then (3): the hub keeps the
   connection and fails only what was outstanding (`Ended`), plus why the waiter starved. (b) is
   a follow-up node, not B95's; (2) is out: the dead instance's session is its launcher's to
   release, not the hub's to patch.
3. **A new node (the Architect drafts it): a restarted entry does not meet its predecessor's
   state.** init re-mints the same badge number for the next instance (init.rs `start`), and a
   shared server keys fids, admission and the mux session by that badge: the dead instance's
   session lasts to its bound and its fids and admission for ever, and the next instance inherits
   both. The rule exists (serving.md "A client dies": connections stay until the launcher
   disconnects them; wire.md "A launcher releases its child's grants"); init honours it for the
   steward's fresh connections only. Design: init releases a dead instance's state at every
   server it was handed a root badge for, before the relaunch (a typed release through the
   re-minted badge that frees the root's own fids, session and admission, as `disconnect_all`
   frees what it minted), or the restart is handed fresh connections as the steward is. Tier A
   (init, libs/rt), S-M. B95 states it as a residual on serving.md with the window and the fid
   arithmetic (walfsd's per-bucket fids against what one beamlet instance holds, times init's
   restart count).
4. **Q2: (a).** The replay inside the grace is the case's stated condition (its comment) and the
   same-badge restart it exercises found B26 and B95: keep it. (b) removes the coverage; (c) is
   not warranted: `poweroff` and the grace exist for this. testbench.md unchanged. After (1) the
   replay must be clean on both widths; a forbidden line in the grace stays the verdict.

### B95-cause, corrected premise (architect-oct20)

The implementer's check: fids do not outlive a dead instance past its successor's attach: the
plain path's `Tversion` drops every fid of the key (ninep.rs `answer`, intro(5) version), and
beamlet attaches with one before `Hub::connect` (the hub's contract: an attached connection).
What outlives it is the **mux session** alone: InFlight and its pending requests' Requests and
Pages, to the session bound, and the bound is extended by every hold-0 completion call the
successor's connect makes (parked, answered empty, `pump` resetting the deadline), so a successor
retrying connect faster than the bound never gets a session. INIT6 is re-scoped to the one
obvious fix: **a `Tversion` on either path ends the key's mux session as it drops its fids** (the
mux path already ends it as a protocol error; the plain path says "a new session on this
connection: every fid on it goes" and must mean everything). No change to init; init.md's Restart
bullet is reworded to the truth (a root-badge instance's state at a server goes at its
successor's `Tversion`; the console and the steward's fresh connections at init's disconnect).
Size S, libs/rt + a case + the pages. B95's residual sentence names the window as the implementer
wrote it, closed by the rule above when it lands (no id on the page).

## B94-cause: an idle socket's liveness tolerates the losses the design declares (architect-oct20)

Finding (.wash/local/B94-identification.md): ipd's established sockets carry smoltcp's 60 s
timeout (from the last segment in) and a 30 s keep-alive (from the last segment out), checked
timeout-first, so an idle session lives on one probe and one answer per 30 s: one lost answer, or
one ipd turn missed between T+30 and T+60, resets a live session. netd drops a frame ipd has not
taken within 50 ms "as on any wire" (netd.md): the design declares the loss path on which the
liveness check tolerates nothing. Not reproduced under a side load; 1 in 1 whole bench.

Ruling:
1. **A fix is ruled without a reproduction.** The page claims "an established socket without
   acknowledged progress for 60 s is ended"; the mechanism ends one after a single lost answer,
   which is not 60 s without progress. That is the code departing from the page on a loss path
   the book itself states, and a shared server resetting a live session is a defect whatever the
   rate. Option **(a)**: the keep-alive interval derived from the timeout over a stated probe
   count, `KEEP_ALIVE = IDLE_TIMEOUT / 6` (10 s): five probes before the timeout; the socket
   survives four consecutive lost answers, or an ipd stall of 50 s; the timeout stays 60 s (no
   bound moves). Stated on ipd.md with the arithmetic and the loss model (netd's 50 ms drop, an
   ipd turn missed). (b) is out: per-socket probe counting in ipd for what smoltcp's two timers
   express. (c) is out: a residual would record a known defect against a declared loss path.
2. **Cost measured, not assumed:** an idle socket now wakes ipd six times a minute; `launch-idle`
   and the idle ceilings (IDLE1/B56's cases) unchanged, the probe's cost per socket stated beside
   the rule; a host test of the stack on the fake clock: four lost answers survive, five end it,
   a 50 s stall survives, a 60 s one ends it.
3. **The exclusive run stays**, as the measurement that names the loss source, and its reading is
   as the implementer wrote: a netd drop (the declared path), an ipd wake 30 s late (a scheduling
   finding: a shared server starved on one hart under sixteen VMs, the owner's Q1 data point again,
   brought to the Architect with the numbers, not fixed in B94), the ACK missing from the capture
   (host/slirp: a bench residual), a FIN from sshd (another bug, its own node). On a pass: 0 in 1,
   the worst ipd lateness and netd drops as the measured margin, on the page beside the tolerance.
   The case's bound, deadline and N do not move.

Tier A (servers/ipd), size S, reviewer kernel-red-4 as assigned. Pages: ipd.md "A socket lives"
and "Sizing"/residuals as touched; netd.md's drop sentence cross-referenced; SECURITY.md rows if
tests change.

## B95's side findings → B98, B99 (architect-oct20)

1. **walfsd's mount fails under load, with no reason, and the box reboots → B98 (new, Tier A).**
   Not B51 (a completion call's clock), not B26 (the replay), not INIT6 (the mux session). The
   console (.tmp/B95/load/rv32-02.kept.smp1.log:48) has walfsd's "does not mount, and is served as
   corrupt" and no blkd line: blkd prints nothing when it marks its device broken (disk.rs "Broken
   stays broken"), and walfsd prints no reason for a failed mount (the read's error or the check
   that failed). The likely path is blkd's REQUEST_TIMEOUT_US (10 s) missed under host load (a
   disk case's guest clock follows the host's while the guest idles on a virtio completion), the
   device marked broken for ever by a deliberate rule whose reason is a lie, not a slow host;
   IPD2's "volume poisoned: create: blkd answered Failed" is the same signature. The cascade after
   it (beamlet exits 1 five times, init reboots) is init's rule as designed, not a finding.
   Investigation-first: (i) blkd says when and why it marks a device broken (timeout or which
   lie), walfsd says why a mount failed; (ii) reproduce under the same load with the lines on;
   (iii) if it is the timeout under host load: the design question is the bound's clock, the same
   shape as B94's, brought to the Architect with the numbers (no bound moves in the node); if the
   reads returned wrong bytes, a kernel/driver finding, top priority. Reviewer kernel-red-4.
2. **bootfsd's heap cap fails its check under load → B99 (new, Tier B, editor).** userland-boot
   rv32, six of six under the load set: "heap needs 2308 pages for twice its 1154-page peak,
   capped at 2304"; alone it passes. The cap is twice an idle-measured peak (B35/B42 set it at the
   rule's value, 2 × 1152); the peak carries a load-dependent term the derivation does not: the
   pages of requests outstanding at once (bootfsd admits pages 2 and in_flight 2 a bucket; more
   clients overlap under load) and any Vec growth they trigger. The guest reports the failure in
   guest time, so under the shared-host rule it is a verdict, not a flake. Fix: derive the cap as
   the entries' size plus the admission's worst-case per-request term, stated on bootfsd's page
   and in the manifest's comment, with the memory cases' measured peaks under load beside it; or,
   if the term cannot be derived, twice the worst measured under the load set with that stated.
   Re-tier to A if bootfsd's code changes.

### SCHED5: map_anon's placement search, the hammer, the heap's termination (architect-oct20)

1. **The search is the second named exception, and gets a node of its own (K34).** After the cap,
   every range call's pages are bounded by `MAX_RANGE_PAGES`; `map_anon`'s (and `map_device`'s,
   `dma_alloc`'s, a received message's) placement search is bounded by the area's constant
   (65,536 pages, 9.6 ms in the checked build when every table is present), which a process can
   make every one of its own calls pay by taking a page in each 64 KiB window of its own area:
   ten slices per call, at will. SCHED5 states it beside R10 as the second exception, with its
   measured number (option (a)); K34 bounds it by construction: a per-process free-run summary over
   the placement area kept in step with the page tables (a page is free only if its entry is
   empty: reservations and both sides of a loan are taken), read a word a level as the frame
   bitmap is, so a placement costs a constant small against a slice whatever the area holds; or a
   search that resumes where it left off across the process's entries, if the summary's memory
   per process is not worth it. Tier A (kernel), size M, kernel-red-4. Not in SCHED5.
2. **The hammer.** The contention cases keep the whole-area worst case with 16-page requests by
   taking a page in every 64 KiB window of the hammer's area (4,096 pages a hammer budget in place
   of 512, the budget grown to hold them): the same search length as before, so the recorded
   figures compare; the toml's comment states the derivation and the page's sentence on the
   hammer's shape follows. A figure that moves is re-recorded, no target moves.
3. **The heap's termination is the one-pass rule, not a count.** "At most 16 gaps" would fail a
   large block that fits past the sixteenth gap, the regression option (ii) was ruled out for. The
   loop ends when the first-chunk address, having wrapped, reaches the first attempt's: the
   kernel's own answer. The retries' cost in a fragmented area is the process's own, each call
   bounded, and K34 makes each cheap.

### B95 review (servers-red): fresh requests are served, never timed out (architect-oct20)

P2: a request's deadline bounds a request the file server asked to hold; a request not yet tried
is tried once, in order, whatever the clock says, and a tried, waiting request is judged at the
time of the judgment. A fill slow on one request may not turn its untried siblings into timeouts
(the client has no clock on the server's time). Folded into B95 with a mux_held assertion.
P3-2: INIT6 also covers the successor collecting the predecessor's pending answers and the
stranded admission carrying over (added to its brief and body). P3-1 (expire's stale now after a
long pump): not a node; judging late by at most a fill, never early, once every deadline is set
from a fresh read; the one-line re-read folded into B95 if it is one, else a sentence.

### B94: the keep-alive's idle cost; the per-packet cross-hart cost → K35 (architect-oct20)

Measured (.wash/local/B94-idle-cost.md): KEEP_ALIVE 10 s costs launch-idle's busiest hart about
+1.1 interrupts/s (four more probes a minute on one socket), over its 3.8/s ceiling in 2 of 6
rounds and in the one quiet round; the rise is m_software on all four harts: about 66 interrupts
and 84 SBI calls per probe across the machine, ipd itself waking once. Ruling:
1. **Option (2): KEEP_ALIVE = IDLE_TIMEOUT / 4 = 15 s**: three probes before the timeout, two
   consecutive lost answers or an ipd stall of 45 s tolerated; the declared loss model is one
   dropped frame per ipd stall past 50 ms, and two such in a minute under load is the worst the
   bench has shown plausible; the cost about +0.5/s on the busiest hart, under the ceiling, which
   does not move (it is a target set from measurement). Re-measure as the table was made,
   alternating against base, the quiet rounds the ones that count; if it still crosses the
   ceiling in a quiet round, stop and come back. State the tolerance, the cost per probe and the
   per-probe path on ipd.md beside the rule.
2. **(3) is out**: lengthening the timeout to fit probes moves a stated bound (a dead peer's
   socket held twice as long, a capacity cost to every account) for the idle budget's sake.
3. **(4) is a node, K35**: 66 cross-hart interrupts and 84 SBI calls to send one small frame and
   take its answer is the box's per-packet cost at rest, paid by every packet of every idle
   session, not the probe's; the chain (ipd's timer wake, transmit to netd, the card's interrupt,
   netd's receive thread's send, ipd's wake) should cost a handful of wakes. Tier A (kernel's
   wake/IPI path, netd, ipd), M, kernel-red-4; after B94. When it lands, the probe interval is
   not re-derived unless the loss model changes.

### SCHED5: the search's true worst (architect-oct20)

Finding (.wash/local/SCHED5-search-finding.md, base kernel): a refused `map_anon` costs one
page-table walk per candidate start, so its cost follows the starts, 65,536 / n for a page pinned
per n-page window: 22 / 35 / 61 / 114 ms on rv64 (16 / 24 / 38 / 68 on rv32) for n = 16 / 8 / 4
/ 2. "9.6 ms worst" was one shape. A lease's budget buys about 25-30 ms a call; 128 MiB about
114 ms. Rulings:
1. **Pages:** SCHED5 states the table and the per-start cost on memory.md R22 (and the figure
   wherever it is repeated: scheduling.md R12/R78, SECURITY.md), the worst measured 114 ms, and
   what a budget of a given size buys; the exception is named with that number, not 9.6. K34
   stays its own node (a different structure; SCHED5 is L already) and **moves up**: launched
   as soon as SCHED5 lands, ahead of anything else on the kernel's memory path. Its brief gains
   the per-start cause: one walk per start is the constant factor, a search that walks once and
   iterates entries (the resumed form) or the summary removes it.
2. **map-anon-search-bound:** (a), stated. The old bound was derived from a shape the cap makes
   unbuildable, so the case changes shape of necessity, not to pass: it builds the densest refused
   shape its own budget affords (state n and the pages pinned), asserts twice the measured worst
   rounded up (n = 16: 22.4 ms → 45 ms; the wake bound likewise from the measured lateness), and
   the page says the bound is that shape's, with the table beside it for what a larger budget
   buys. K34 re-derives both down. (b) would leave the search unguarded until K34.
3. **Contention cases:** hammers pin before the window; `search_alone` moves into a hammer-sized
   child so its refusals are the search's, not its budget's (a measurement measures what it
   claims); the figures re-recorded, no target moves; `irq-boot-hart-only` is a negative control:
   a negative that passes proves nothing, so it must fail again, for the stated reason, before
   SCHED5 is reviewed, and the reason it stopped failing is in the report.

### SCHED5: the contention family's hammer is chosen by its self-check (architect-oct20)

irq-boot-hart-only is sched-lock-contention's self-check (same load, interrupts on the boot hart
alone, must_fail on the driver wake's target). At n = 16 it passes on rv64: the gate at that load
no longer catches IRQ1's removal there. Rule: the family's hammer is the densest pin shape at
which, on both widths, the positive meets its targets and the negative misses them; one hammer
definition, n stated with the derivation, figures re-recorded, no target moves. The earlier
"n = 16 so the figures compare" yields. If no n separates them on a width, that is a finding
stated with the numbers, re-examined when K34 lands.

### B94 review (servers-red P2-1, P2-2): the idle ceiling's headroom; the probes' scaling (architect-oct20)

P2-1. testbench.md says launch-idle's ceilings are twice the measured worst (1.6-1.9 when set,
2026-10-14) and so catch a cost that doubles. Main before B94 already reads 2.4-2.8, B94 2.9-3.5
against 3.8. Ruling: **keep 3.8**; the page restates the truth: set at twice 1.6-1.9; main now
measures 2.4-2.8 and with the keep-alive at 15 s 2.9-3.5, so the ceiling now catches a rise of
about a tenth, not a doubling; the rise since it was set is a finding of its own (B101), and the
per-packet cost (K35) is where headroom comes back. Re-deriving to 7 would hide the doubling of
base the ceiling exists to catch; holding a live-session correctness fix for an efficiency node is
backwards. launch-idle's verdict rule is the host-clock one already on the page: a miss beside
other work is no verdict until it misses alone; B94's twelve alone rounds are the verdict.
**B101 (new, Tier A investigation, kernel-red-4):** the idle box's busiest hart rose from 1.6-1.9
to 2.4-2.8 interrupts/s between the ceiling's setting (87553025a, 2026-10-14) and 3e6f29443, with
the 30 s keep-alive on both sides: bisect by train with launch-idle alone, name the merge and the
mechanism (m_software hand-offs, timer re-arms, a new periodic wake), fix or state; no ceiling
moves. Independent of K35 (the per-packet chain) and B94.

P2-2. The scaling (about 66 IPIs a probe, four a minute per silent socket, up to an account's
socket cap: alice's 32 about 140 IPIs/s machine-wide, paid by ipd, netd and the kernel) is a
residual, not a node: it is bounded by the manifest (the owner's socket cap times the derived
probe rate times the per-probe constant), the servers that pay it are weighted (R12 bounds a
server's work; the kernel's interrupt work is billed to the device's owner), and K35 shrinks the
constant. State the arithmetic on ipd.md beside the cost. Charging a server's work to its client
is R12's settled design and is not reopened here.

### SCHED5: the self-check at n = 8 (architect-oct20)

No n separates the pair on rv64: at n = 16 the negative meets (p99 44.6), at n = 8 the positive
misses p50 (34.0 ms, a whole 35.6 ms search). Ruling: trace n = 8 on rv64 first, one boot: a wake
landing inside a section and served on the hart IRQ1 lets take the interrupt should wait about
half a section at p50; a whole section says the woken driver waits for the busy hart's section
end while another hart idles, or the interrupt is taken only at a boundary: a scheduler finding
the gate found, not a gap in the gate. (a) If so, a new node (K36) with the numbers, and SCHED5
takes the implementer's proposal (family at n = 16; rv32's self-check judged; rv64's stated as a
finding with the reason; re-examined with K34 and K36). (b) If the full-section wait is
inherent, the proposal stands with the reason on the page. No target moves. The launcher's batch
stays 64 pages, split into range calls by the runtime: a launch costs nothing measurable (2.29 s
rv32 median, the base's; a 16-page batch read 3.65 s), stated beside the cap.

## B98-cause: a slow host is not a lying device; the bench's disks do not fsync the host (architect-oct20)

Reproduced (.tmp/B98/cause-numbers.md): under injected host I/O pressure (dd + fsync on the ext4
holding the case images; PSI io 53-78 %) beamlet-files failed 3/10 a width; blkd marked its
device broken on a flush with no completion in 10.0 s; walfsd's mount said "formatting the blank
range: Io: blkd answered Failed". No byte wrong. The guest's flush is QEMU's fdatasync
(cache=writeback) queued behind the host's writeback; no icount, so 10 s of host time. Natural
load (PSI 2.8 %) did not reproduce it in 20 runs. FILE_WAIT_US (10 s) gave one etimedout the same
way (a walfs transaction is several writes and four flushes).

Rulings:
1. **The box's rule stands.** blkd's "a lie is permanent" has a stated reason that is sound for
   DMA (a completion for a request no longer tracked), and a 10 s flush on the box's own SSD is a
   broken device. The refinement (1) (fail the request, keep it tracked until the device answers,
   permanence only for protocol violations) is not built now: it would not rescue the volume
   either (walfsd poisons on a Failed write, correctly), and it answers a condition the box's
   hardware does not present. It becomes a node only if real hardware shows slow-but-honest
   devices. No bound moves.
2. **The bench owns it: (2).** A case's disk is fresh per boot and the guest's flush ordering is
   honoured in the host's page cache either way (a QEMU exit loses nothing; the power-loss case
   simulates loss by QEMU's exit, not the host's), so the bench's case disks run `cache=unsafe`:
   a guest flush costs no host fsync and the host's writeback pressure no longer reaches blkd's
   bound. launch-keep-disk's second boot reads the same file from the same page cache. `./launch`
   (the owner's hand runs) keeps writeback: its disks are the user's and its host is the user's;
   stated, not decided here. testbench.md's disk section states the setting and the reason with
   the numbers (3/10 at PSI 53-78 %, 0/20 at 2.8 %); blkd.md says that on a virtualised disk the
   bound measures the host too, which is why the bench does not fsync it.
3. **FILE_WAIT_US: the same answer.** It does not move for a host artefact; a walfs transaction
   that exceeds it on the box's own disk would be a finding of its own.
4. **The reason lines stay** (blkd's broken line with its cause and numbers; walfsd's mount
   reason): they are what made this identification possible, and they are B98's deliverable
   beside the bench change. The console lines are contracts: the cases that forbid or expect them
   are named in the report.

Tier A as assigned (blkd/walfsd lines), the bench change Tier B inside it; reviewer unchanged.

### SCHED5: the n = 8 trace; the hammer must cover the window (architect-oct20)

Trace (.wash/local/SCHED5-trace-n8.md): IRQ1 works (the idle hart takes the alarm and runs the
driver); the whole-section wait is the driver's own `receive` queued in ticket order behind the
hammer's next search, phase-locked: R78's one-section bound, inherent under one lock. But the
hammer covered only 105 of 200 samples, so the gate's p50 mixed loaded and unloaded wakes, and
n = 16's 1.2 ms p50 is untraced: a hammer that does not load the samples passes the positive
for the wrong reason. Ruling: (1) the hammer covers the whole window (searches until the window
ends, pins kept), at every n; (2) re-measure n = 16 and n = 8 at two harts, both widths, with the
trace saying what fraction of wakes fell inside a section; (3) then the self-check rule applies on
the corrected hammer. Expected: under the cap the shortest refused search is n = 16's (22 ms rv64,
16 rv32), above the 15 ms p50 target, so a phase-locked continuous hammer makes the positive miss
at every buildable n by R78's own bound, whatever the interrupt routing. If so, the page says it:
the two-hart contention gate's wake targets are unmeetable under the cap until the search is
bounded (K34); the contention cases and the self-check record, not judge, until K34, which
re-judges them as part of its acceptance; SCHED5's own cells (the rv32 steward-latency rows) are
judged as before. If the corrected hammer leaves the positive meeting on a width, judge there.
No target moves.

### B97-shape: an empty listener's entry is charged to its holder's bucket (architect-oct20)

Approved as proposed: `room()` and `listen()` check the key's own entries (lingering included)
against 2 x cap(key) in place of the whole table against max_entries; the global check stays as
the backstop for the window between a disconnect and the next poll's sweep. Arithmetic on
ipd.md "Sizing": a bucket's sockets <= cap and a listener with a backlog holds a charged backlog
socket, so legitimate use never passes 2 x cap; only empty listeners past it are refused, on the
holder's own bucket; live keys are at most the buckets, each counted in max_sockets at
max(cap, default), so the sum over live keys <= 2 x max_sockets and the table's size stands.
A State unit per empty listener rejected (the accept that empties it has no reservation to pay
from; refusing it would break sshd at its cap). Host tests as proposed (A fills with empty
listeners, B still gets its socket: red on d66839252); no attack case (only a listen scope, the
image's sshd, reaches it: stated on the page). Per-key counting is a scan of at most 216 entries
or a kept count: the simplifier's call. Tier A, small, red review as the node says.

### SCHED5: the corrected hammer confirms it; the contention family records until K34 (architect-oct20)

With the hammer covering the window (.wash/local/SCHED5-cover.md) every loaded wake waits one
whole search, phase-locked: n = 16 reads 21.4/21.9 ms rv64 and 16.0/16.5 rv32 (search alone 22.6
and 17.0), above the 15 ms p50 target; n = 8 worse. The shortest refused search under the cap is
above the target on both widths, so the two-hart contention gate's wake targets are unmeetable
under the cap by R78's one-section bound until the search is bounded (K34). Approved: the page
says so with the trace's reason and the numbers (re-measured alone once, since the page's
figures come from the stated condition: the runs beside train 89 are guest time, but the page
states figures under one condition); the family (sched-lock-contention, -4, -4-mttcg,
irq-boot-hart-only) records its wake figures in the `record=` form and keeps ticket order and the
other oracle checks judged; the self-check's must_fail is recorded with them (a negative cannot be
judged while its positive is not); the hammer at n = 16; SCHED5's own rv32 cells judged. K34's
acceptance re-judges the family and the self-check at the targets. The earlier 1.2 ms pass: one
sentence in the report (the hammer ended before the window; the coverage check forbids it).

### B98-cause, launch cases (servers-red P1) (architect-oct20)

My ruling assumed launch-kind cases took cache=unsafe; they do not (launch::command passes
Flush::Host). Ruling (a): thread a Flush through launch::command, the bench passing Free, ./launch
keeping Host. Same reasoning: a launch case's disks are the bench's, fresh per boot, and
launch-keep-disk's second boot reads the same page cache. What a launch case proves is ./launch's
command line, not the host's fsync policy. The one difference is stated on the page and in each
case's comment, and a host test asserts launch::command differs from ./launch's line only in that
flag. Gate: launch-keep-disk, launch-system, launch-idle both widths under B98's injected writer,
10 runs each, 0 failures; launch-idle also alone (its host-cores ceiling must not move). P3s
folded: the flood cases' 4,000 ms bounds get a comment now and are re-measured next train (no
bound moves until then); Broke.waited's span fixed.

### SCHED5: rv32 two harts gated; the deadline-notice cell → B103 (architect-oct21)

Both of SCHED5's cells fixed (row 2: 528 µs net from 32.5 ms; row 1: 4.0 and 4.3 ms from the
base's 38.0). The rv32 two-hart deadline-notice p99 misses 40 ms on head and base alike (43.4 /
45.3 / 56.5 and 41.9 / 41.9 / 56.1 at N = 1-3): never judged there before (smp rv32 = [1]; the
page's 13-14 ms were at the dequeue). Ruling (a): gate rv32 at [1, 2], that cell
record=rv32:2:N=k:deadline_notice:p99 for k = 1-3, both sets of numbers and the fixed rows on
scheduling.md, no target moves. B103 (todo, needs SCHED5) traces and fixes or states it; its
acceptance removes the record= and judges the cell.

### B100-shape: a send's messages are taken in place, never listed (architect-oct21)

Approved B (two passes over the transfer, no list): A's cap would be a new protocol rule (Tflush
frees its oldtag's bit as taken, so a send may legally carry more than 256 messages). Conditions:
pass 1's cost is the transfer's own bytes (stated in the code); pass 2 stops cleanly where the
session ends mid-transfer (messages before k taken, k on dropped with their pages' admission
released; a host test); the counting-allocator heap test (full MAX_LEND_PAGES of 7-byte
messages, tag reuse through Tflush, peak under one page; ~640 KiB on 820d5e5ff) plus the
long-message path unchanged; the attack from a native program in a session's budget to bootfsd,
verdict bootfsd's heap record peak, or the host test as the attack with the reason on the page.
bootfsd.md's residual goes; serving.md 'A request' gains "taken in place, never listed".
Tier A, small, red review.

### B102-shape (architect-oct21)

steward-context-login's one-way :redoubt.send(/dev/cons, ended) meets send/2's 1 ms receiver
wait (one slice) while consrelay's serving thread is between turns; the only caller of one-way
send in the tree is this case. Rulings: (A) the case's step retries until the relay takes it,
bounded by time (2 s of tries at 10 ms, stated as 2,000 slices), printing the try count when it
lands; send/2's contract and SEND_TIMEOUT_US unchanged. (B) fail fast: forbid {:ended, {:error
per line, `within = 60` before the expect. (C) in B102: a mark-wait timeout does not abort the
other sessions, and a failure ranks before a timeout so the report names the stuck session; host
test; testbench.md's multi-session report sentence updated. Gate: 5 alone runs each width, docs,
formatting, and a load run (10 each width beside the 8-case side set, orchestrator's go) with the
try counts reported; a run needing more than half the bound's tries is a finding before merge.

### SCHED5 review (sched5-red P2-1): device owners' dma_alloc and map_device (architect-oct21)

(a): SCHED5 states a third bounded-but-not-sliced cost on R22, R12 and SECURITY.md: a device
owner's dma_alloc and map_device, linear in the pages asked for, bounded by the DMA pool (1,024
pages, ~48 ms rv32 checked at its worst) and the device's register region; only a driver init's
manifest names holds the DMA-flagged handle, and the image's drivers call each once in open()
(blkd/kernel.rs:66, netd/kernel.rs:146) for their measured sizes, so a running box pays it only
at a driver's restart. Not (b): no hostile caller reaches them, and chunking would break
dma_alloc's contiguity contract. A node only if a driver calls either while serving or with a
size not a constant; a test or assertion holding the call to open() is the simplifier's call.
The rt place() rover P2: fixed in SCHED5.

### B100 attack case: a session reaches the raw send; bootfsd's stack (architect-oct21)

A session sends the transfer itself (beamlet's send native, up to MAX_BUFFER), so serving.md says
any session can, and the bound is the server's. The case: pure Elixir on image/boot.toml; the
verdict bootfsd's heap record (rv32 fails on the base: 2,670 needed against 2,510; rv64 shows the
gap but cannot fail, the cap sized from rv32's boot peak; stated). bootfsd's stack: (a) 4 → 5
pages, twice rv64's 8,952-byte peak, in the image and every manifest whose memory case reaches
the path (listed; others keep 4 with the reason); the memory table's row updated. Memory cases
booting bootfsd re-run both widths.

### B101-cause: the lock's release wakes the next ticket only; wake_idle after release (architect-oct21)

Drift = IPD1's 8 s park (sshd's two reads re-asked: user ecalls 6.0 → 8.3/s; stated, IPD1's
ruled margin) + the lock's hand-off: release wakes every marked hart though a ticket lock serves
one (36-41 % of hand-off halts follow another), and wake_idle's interrupt is sent while the waker
holds the lock (15-22 %); under host load sections stretch and the hand-off halts multiply.
The ceiling's 1.6-1.9 does not reproduce at its own commit (2.1-2.5). Ruling: (i) the release
wakes only the next ticket's hart: waiter records ticket, marks SeqCst, re-reads now_serving
before halting; releaser advances now_serving SeqCst, reads marks, wakes the match; the argument
on R78; an exhaustive interleaving host test and a mutation it catches; smp-lock-wait at 4 harts
MTTCG. (ii) wake_idle's interrupts collected under the lock and sent at release. FIFO, the halt
and the icount argument unchanged. Re-measure launch-idle twelve alternating alone rounds;
testbench.md states the measured range and the ceiling's origin honestly; the ceiling does not
move. Report B94's per-probe cost after (K35 starts from it). Tier A, kernel-red-4 + simplifier +
editor.

### SCHED5 fold (red P3-3): process_map from untouched reservations (architect-oct21)

(a): R22 states the cap's one overrun: process_map from untouched reservation pages backs and
zeroes them inside the call, 16 pages 1,765 µs net rv32 / 1,445 rv64 (~1.8 slices); only the
loader's first process holds reservations (memory.md: process_start gives none), so no user
process can build it. SECURITY.md's row says so. The derivation stays map_fixed's ((c) out:
halving every cap and costing launches 60 % for a trusted start). (b) only if reservations ever
reach another process; the page names that condition.

### B101-cause, correction: (ii) withdrawn (architect-oct21)

Deferring wake_idle's interrupt to the release failed timer-flood-at-rest on both widths (668 /
666 sections that find nothing, bound 20): the waker's hart retakes the free lock and picks the
woken budget itself, and the woken hart wakes to nothing. An idle wake exists to queue the woken
hart before its waker retakes the lock. (ii) withdrawn; (i) stands. R78 keeps "a wake sends the
interrupt at once, and the woken hart may halt on the lock", with one sentence: the 15-22 % of
hand-off halts after an idle wake are that price, and deferring was measured to turn each into a
wake for nothing (the case's numbers).

### B106-shape: a served child's endpoint is owned by the child's budget (architect-oct21)

jobs.rs served() makes the child's endpoint in the VM, so the session's budget owns it; piped's
death fails neither a later call nor a queued untaken one, and the waiter blocks for ever.
Ruling (1): the served child's endpoint is owned by the child's budget, so its destruction fails
queued calls (endpoint_dying) and makes later calls Dead; a stamp-only fix would leave the queued
window. Mechanism, implementer's choice with reason, Architect prefers (a): endpoint_create takes
an optional budget handle at or below the caller's (owned by and charged to it; mutation and model
contract), over (b) the child creating its own and returning the send right (a launch round trip).
Pages: objects.md, ipc.md R4b (served children own their endpoint; init's root-owned endpoints the
deliberate exception), native.md/beamlet.md served children, io.rs Io::forget's doc. R13 needs no
line. Evidence: two fake-kernel host tests (death between answer and next call; a call queued
untaken), red on main; pipe-sixty-four rv32 x10 alone on SCHED5's head; short gate. Tier A, red.

### B106 review (sched5-red P1-1): a swept handle's index is not reused until its holder closes it → K36 (architect-oct21)

B106's (a) makes a call after the death get BadHandle: R10's sweep frees the index and the next
handle the VM installs (likely the next launch's endpoint_create) takes it, so a hub waiter
mid hand-over can reach the next piped or an unrelated server. Before B106 the index stayed valid.
This is the class STEWARD10/11 and KILL1 met in the steward (a recycled index acted on as the
old object) and P3-1 here (close_served closing reused indices): the kernel frees a swept index
at once, and every holder must prove it never acts by an index that may have been swept. (iii)
cannot be proved in the VM: a userland check before a blocking call is a race, and the hub's
waiter cannot hold a lock across a FOREVER call that only the destruction ends.

Ruling (i), as a kernel rule, node K36: a handle that a destruction sweeps stays in its holder's
table as a dead entry; every call through it returns Dead, close frees it, and the index is not
reused until then. Revocation stays complete (the entry names nothing and grants nothing, I2);
I1 is untouched (a dead entry is no object, so no lookup reaches a freed frame). A copy in a
message not yet received still arrives as 0. Cost: a dead entry holds its slot against
MAX_HANDLES until closed. Survey first (K36's first deliverable): every holder that relies on a
swept index vanishing (init's closed-copy checks, the steward's stale-index guards, servers
holding client-stamped copies, the model's I2), each either closing what it is told is dead or
shown to; the survey says whether every sweep or only endpoint and process sweeps need it.
B106 needs K36: its "Dead, not BadHandle" promise is K36's. B106's P2-1 (split
R9EndpointInAnyBudget) folds now; P3-1 is closed by K36.

### K36-survey: a sweep never frees an index; every consumer closes in K36 (architect-oct21)

Survey: `.wash/local/K36-survey.md` (k36-implementer, main 7575a4267). Rulings on its five questions.

1. **Scope: every live-table sweep (S1 budget chains, S2 process object freed, S3 device
   destroyed).** One rule, "a sweep never frees an index": the hazard is the index, not the kind.
   S1 budget handles are the STEWARD10/11 and KILL1 class; S2 lets a stale `process_start` or
   `process_map` act on another unstarted child; S3 is init's quarantine check. `close_all_handles`
   (the ender's own table) is not a sweep: no holder is left. Delivery-time nulling of a queued
   copy (`is_live` → 0) is unchanged.
2. **`budget_destroy(h)`'s own h goes dead like any other; rt's `Budget::destroy(self)` closes it
   after the call.** No kernel exception. `budget_reap` keeps h (its budget lives). A deadline has
   no caller: its handles go dead and each holder closes (the steward's `DestroyBudget`).
3. **Agreed, as stated.** `handle_close` on a dead entry is `Ok` and frees the index (init's
   close/close-again check keeps reading `BadHandle` second). Every other use of a dead index is
   `Dead` and the call does nothing, a dead index as an argument included: a send's handle list
   (nothing sent, nothing arrives as 0), `process_start`'s list, `mint`'s budget, `process_create`'s
   exit endpoint. Order: `BadHandle` (0, past the table, empty slot), then `Dead`, then
   `WrongObject`. A dead entry stores only its mark: no frame, id, badge or stamp and no chain link,
   so I1 and I2 read it as nothing. It counts against `MAX_HANDLES` and keeps its table page charged
   until closed: the cost, stated on objects.md.
4. **Every consumer's close lands in K36**: init (own handles per restart, console connection,
   instance budget), the steward (`DestroyBudget`, `Held`/notices/launched at a scope's end, the
   `relay_vm` replacement), the VM's collection (a collected term closes its index; "gone, never
   closed" goes), rt's `Budget::destroy`, the model, the steward host tests, the twelve bench
   programs reading `BadHandle`. Reason: from the kernel change on each F is a leak into
   `MAX_HANDLES` in a long-lived holder; a kernel contract change is built against every consumer
   before any whole bench.
5. **Retire the steward's install lock and clock witness in K36.** The destroy guard and the
   recycled-index host tests change in K36 anyway, and a fence left standing needs a comment that
   says why the obvious way fails (tenet 1); after K36 no honest comment exists. Condition: the
   report states, every site read, that `without_installs` guards nothing but index reuse; if it
   guards anything else, that part stays and the report says what.

**The book.** A new invariant **I17 (a swept index stays dead until closed)** on
`docs/kernel/invariants.md` after I16: the statement, kept-in (`handle.rs`'s dead mark; the three
sweeps), the model contract and the mutation (the sweep frees the index: today's behaviour), the
attacks (`budget-destroy-attack` now reads `Dead` while indices are not reused); I12 gains the
pointer: a stale id cannot reach a later object, and a stale index cannot reach a later handle
(I17). `docs/SECURITY.md`: the I17 row, its Tested-by equal to the section's list (C7). objects.md
Handles: "lowest free index" becomes the lowest index neither live nor dead; "an index stays valid
until it is closed, and a swept one stays dead until then"; the `handle_close` row and paragraph;
the error order; the cost. budgets.md R10 step 6: "becomes a dead entry in every process's
table; the holder's close frees the index". ipc.md R4: a dead handle in a send's list fails the
send with `Dead`; the queued-copy rule unchanged. processes.md: `process_start`'s list. steward.md:
the destroy rule is `Dead`; the lock's paragraph goes. The VM's served-children text (B106's pages)
loses "gone, never closed". No owner choice: nothing here weakens a guarantee; derived from I1/I2,
R10 and tenet 1. K36's body (active) needs no edit: its survey clause is answered here.

### SCHED5: a long run is placed by the kernel's search and mapped a range at a time (architect-oct21)

Finding (.wash/local/SCHED5-heap-allocation-identification.md): under the cap the rt's `place()`
finds a too-short gap only by mapping it (allocate, zero, unmap), so beamlet-heap-allocation rv64
went from 15 s to 47-64 s: 190,637 `map_fixed` and 196,212 `unmap` where the kernel's search read
page-table entries. **Ruling: option (1).**

1. **Shape.** `map_anon(len, flags)` keeps its signature; `len` is the run's length and is the one
   range argument that may exceed `MAX_RANGE_PAGES`. The kernel searches for a run of `len` pages
   as it did before the cap (the first free run, from where it placed last, one pass) and **maps
   the first `min(len, MAX_RANGE_PAGES)` pages** at the start it found, returning that address.
   The caller extends with `map_fixed`, a range at a time. No reservation of the tail, no new
   state: the tail is placement, not a promise, and "nothing may depend on the address" stands.
2. **R13 (one outcome per call)** holds: the call maps exactly `min(len, 16)` pages at `addr`, or
   nothing (`OutOfMemory` when no run of `len` fits, as before). The run's later ranges are later
   calls, each with its own outcome.
3. **The per-call bound** holds without a new number. The search is the exception SCHED5 already
   states beside R22 (one walk per candidate start; worst measured 114 ms at n = 2); a longer run
   has no more candidate starts than a shorter one (at most 65,536 - len + 1), so a whole-run
   search adds no new worst, and K34 bounds it by construction. The mapping is at most a range.
   `map-anon-search-bound` keeps its shape and target.
4. **The rt.** The extension runs under `Placing`. A refused extension (a page taken meanwhile by
   a placement the lock does not cover: `map_device`, `dma_alloc`, a lend landing from another
   thread) unmaps what it mapped and searches again from the refusal; **the filler goes** (the
   kernel's search skips short gaps itself, which is what the filler bought); the one-pass
   termination (the first-range address wrapped to the first attempt's) stays, as ruled on
   "the heap's termination". handle.rs:75-95's doc is rewritten to this.
5. **Model and fake.** The model's `map_anon` lifts the cap for `len`, places over the whole run
   and maps `min(len, 16)`; one contract: at the call's completion the pages of the run past the
   mapped range are free; one mutation, the kernel searching for a range only (SCHED5's current
   behaviour), fails it. The fake the same. No new RNG draws (gen.rs picks from values drawn).
6. **Refused.** (2) keeps a second record of what the page tables already hold, not authoritative,
   and keeps the probing fallback it was meant to remove: more state and the same worst (tenet 1:
   deleting code beats adding). (3) lowers a cost it does not bound.

**Pages:** memory.md's call table (`map_anon`'s row: `len` may exceed the cap; the first
`MAX_RANGE_PAGES` pages of a run of `len` are mapped), the range rule at line 42 and `TooLarge` at
54 (the exception named), "Where `map_anon` puts pages", R22 (mapping at most a range; the
search's exception unchanged); model.md's row; `libs/sys`'s doc. **Witness:** the implementer's
own: beamlet-heap-allocation rv64 x5 alone near main's 15 s and its rv32; the rt placement host
tests (filler tests deleted, the refused-extension re-search tested); the sets already run,
re-run. In SCHED5, not a node: the cap without this is the regression.

### K35 stage 2: a thread readied where its waker blocks runs on that hart; no spin; stage 3 is later nodes (architect-oct21)

Measurement: `.wash/local/K35-report.md` (28.0 interrupts, 34.6 SBI calls, 14.9 user ecalls a
probe on rv64; every probe touches all four harts; 17.3 lock hand-offs, 7.6 of them chain threads
on different harts entering back to back). Stage 1 (exact timeout caches) has the orchestrator's go.

1. **(c), a bounded spin before the lock halt, is refused: settled.** R78 measured the spin (300
   turns took back most of the halt's cost on rv32, little on rv64, and cost 7.5 % under `icount`)
   and says "so there is none"; K31's Ruling A judges two-hart shares where the harts run at once
   precisely because where a hart gives up or spends its turn moves QEMU's turn boundary and swings
   `sched-capped`'s icount shares about 60/1000. A spin reopens both. No.
2. **(a) is the rule, written on scheduling.md.** A thread readied in a kernel entry whose runner
   blocks in that same entry is this hart's own pick: no idle wake is sent for it. Constraints:
   the pick is the queue's pick, unchanged (lowest pass, the same thread an idle hart would have
   picked; if a lower-pass budget is runnable it is picked and the readied thread still gets its
   idle wake as today); the rule changes who picks, never what. It deschedules nobody: the
   waker's slice ended for its own reason (a block), so `sched-wake-no-preempt(-harts)` and their
   floors stand. R78 is unchanged: hand-offs are the consequence it prices, not its mechanism;
   the immediate idle wake for every other readied thread stands (the (ii) lesson,
   `timer-flood-at-rest`), and **no wake is deferred past the entry that made it**: the placement
   text ("the woken thread first runs at a pick, an idle hart's or one after a runner left") is a
   latency bound, and a deferral to the runner's next entry would add up to a slice to every
   cross-budget wake, which `sched-latency` and `sched-irq-wake` judge. The residual "No rule
   keeps a woken thread on its waker's hart" is rewritten to state the rule and what it leaves: a
   waker that continues to user still hands off. `sched::leave`'s `unrun` term already does part of
   this; the report says what the rule adds (the `beside` term, or a readied thread of a budget
   another hart runs) and the per-hop count it removes.
3. **(b) is netd's ordering, accepted, with its reach stated honestly.** `send` to a waiting
   receiver and `reply` both leave the sender running, so their `leave` wakes an idle hart before
   netd's `wait_irq` or a server's `receive` blocks: (a) reaches a hop only where the ready and
   the block are one entry (`call`). The implementer names each of the 7.6 collision hand-offs by
   the entry that made it (call-shaped or send/reply-shaped) and reports what (a)+(b) remove. If
   the send-shaped hop dominates, the remedy (a combined call, or a `receive`/`wait_irq` that
   sends first) is an ABI question back to me with the count; none is built unasked in K35.
4. **Stage 3 is not K35.** The rx page as a kept lend changes how frames move between netd and
   ipd (netd's lib.rs rule that each received frame's page is transferred): its own node, its own
   review, after K35's figure is known. One-context IRQ routing was weighed on scheduling.md's
   residual with a measurement ("so it is not routed ... the remedy if it comes to matter");
   about 2 events a probe is not the evidence that reopens it. Not a node unless stages 1-2
   leave it the largest named cost.
5. **The bound.** The acceptance is the derivation, not the number: after stages 1-2 the report
   lists the probe's remaining named events (its own timer, ipd's call and netd's reply, the
   card's interrupt on the harts whose contexts take it, netd's rx entries, the hand-offs left)
   and their sum is the target stated on ipd.md beside the keep-alive rule. 12 interrupts and 15
   SBI calls is the implementer's estimate of that sum; if the derivation lands there it is the
   target, if elsewhere that number is, and nothing moves to pass a run. The case's bound follows
   the bench's practice: twice the measured worst, rounded up, alone and under load, with the
   probe count checked by the card's frame count as the report does. User ecalls are asserted at
   15 (±1) as the check that nothing changed at the sockets. Idle ceilings do not move.

### K35 after the measurement: lands as a named cost and a stale-timeout fix; the levers are K37 and K38 (architect-oct21)

Result (.wash/local/K35-report.md from "What was built"; head 2d199eff5): stage 1 takes
sched-timer-flood's stale-wait interrupts to 0 (main 73 rv64, 19-20 rv32); (b) moved netd's
send-shaped hops onto ipd's receive, net zero; (a) needed no code (sched::leave already discounts
this hart's own pick; ipd's call readies 1.0 a probe with 0 wakes). Per probe head against main:
27.0/33.1 against 26.6/33.5 (rv64), 31.3/37.3 against 28.8/35.3 (rv32): inside the window spread.
The 10-11 hand-offs name as ~5 send/reply/receive-shaped and ~5 interrupt-shaped, 0 call-shaped.

1. **K35 lands, restated.** Title: one keep-alive probe costs an idle 4-hart box about 27
   interrupts and 34 SBI calls, named by the entry that makes each; a wait answered early no longer
   leaves a stale timer interrupt. What lands: stage 1 with its oracle (`no_stale_waits_in`,
   requiring 0: the design's "at most one early interrupt" now true, a fix with the case that
   would have caught it); the (a) rule on scheduling.md stating the behaviour that exists; the
   launch-idle-probes case and ipd's test-only feature (plain ipd byte-identical to main, checked);
   ipd.md stating the per-probe cost measured, the naming (5 + 5 + the probe's own), and that it
   is K37's and K38's to remove. **(b) is dropped from K35**: a behaviour change (the last frame
   held until the queue is drained) with no measured gain and a purpose it does not reach is not
   carried; it goes to K37, where a send that blocks in one entry makes it pay (tenet 1: deleting
   beats carrying). The residual "No rule keeps a woken thread on its waker's hart" is rewritten
   honestly: a server's reply and its receive are two entries, so every server turn hands off
   (K37), and a device interrupt is taken on every hart's context (K38).
2. **The case's ceiling is a guard, said so.** launch-idle-probes asserts per-probe interrupts and
   SBI calls at twice the measured worst alone, rounded up, per width, and user ecalls at 15
   (±1). Its comment and testbench.md say it is a regression guard that passes on main, kept
   because it is K37's and K38's witness (each must turn it red on main for its own hops) and so
   the idle ceilings are not the only reading of the chain. A guard that passes on main is a
   ceiling case, like the machine at rest's; it is not offered as a bug-fix case.
3. **Nodes, both added (todo, need K35), neither blocks K35.** K37: a server's turn is one entry
   (reply-and-receive; a receive or wait_irq that sends first); R13 whole-result constraint; the
   rt serving loop the one consumer; acceptance: the ~5 send/reply-shaped hops gone on the probes
   case (red on main), ipc-client's two-hart round trips within 1 % of one hart (the residual
   rewritten), sched-latency/sched-irq-wake/wake-no-preempt counts kept. K38: a device interrupt
   taken on one hart by threshold (the residual's own named remedy, reopened on cost at rest, not
   latency: new evidence of a new kind); acceptance: claimed-nothing externals 0 a frame, the ~5
   interrupt-shaped hops gone, latency stated against the residual's numbers, irq-boot-hart-only
   stays the negative control. Order: K37 first (a general per-call gain, every server), K38
   after; both after K34 unless the orchestrator or owner ranks otherwise. No owner choice: an
   ABI addition and a PLIC routing rule change no guarantee or wall; each has its stated reason.
4. **rv32's 31.3 against 28.8 is not yet a finding.** The report's own slot notes put 3 SCHED5
   host jobs beside hprobe rv32 2 (77 s), the high outlier, and main's figure is one window. With
   (b) dropped, head's netd path is main's; what could differ is stage 1's hart for a timer.
   Witness: three alternating rounds a width, head and main, alone (starts.txt showing no
   company), probes case and launch-idle; a regression only if head's lowest round exceeds
   main's highest; otherwise the spread is recorded on the case and the guard derives from the
   worst across both. The relaunch does this before the restated acceptance is judged.

#### K35 guard, the orchestrator's option A (architect-oct21)

Item 2 above as built: one case cannot measure a probe (27/34/15 is (probes - launch-idle)/26), so
launch-idle-probes judges per answer frame, background included: interrupts and SBI calls at twice
the worst per-frame figure alone, per width, worst of head and main; user ecalls at the measured
worst + 1 (about 33-34, stated as 15 a probe plus about 17 of background); ipd.md states the
differential and its method as the named cost. Agreed, no overrule. Condition: the worst for the
+1 comes from item 4's alone rounds (three a width, head and main), and if they spread by more than
1 the bound is worst + spread, rounded up, said so on the case. B (subtracting the other case's
background) and C (span counting in the judge) rejected: a second measurement inside a verdict,
and a judge proving a negative.

### K34: irq-boot-hart-only after the search is bounded; the contention family judged again (architect-oct21)

Design approved by the orchestrator: .wash/local/K34-design.md option (a), the per-process free-run
summary (+2 kernel frames a process, charged at space creation; a placement reads group records
and at most 3 bitmap words, tens of µs). The question: what the negative must prove once no
section is longer than about a slice.

1. **What IRQ1 is worth, by construction.** With every hart taking device interrupts, an alarm is
   claimed by the idle hart, which then waits one section for the lock (R78); with the boot hart
   alone, the alarm waits out the boot hart's section and the woken driver's `receive` draws behind
   the next: two sections. IRQ1 saves exactly one kernel section a wake, under any load. R12 after
   K34 makes a section at most about a slice (the costliest bounded call, a 16-page `map_fixed`,
   0.76 ms rv32), so the pair separates by under a millisecond, and the 15/50 ms targets cannot
   see it. The one load that lengthens a section past 15 ms is R10's named exception (a full
   lease's destruction, 30 ms), and it fails the positive too: every wake waits the lock. **So no
   load separates the pair at the targets, by construction.** That is the "state why it can't"
   branch of the negative-control rule, and it is K34's gain, not a loss.
2. **The implementer's proposed verdict (the per-hart alarm count, the other hart taking 0) is
   refused.** It asserts that the debug feature routes interrupts to the boot hart alone: a test of
   test code, proving nothing about the system. A negative must fail for the reason its positive
   passes.
3. **Ruling, conditional on the measurement the implementer planned.** Measure the negative first
   on K34's kernel, both widths, alone. If it still misses 15/50 net of audits, it stays, judged,
   `record=` off, and the page says what it waits behind. If it meets them (expected): **retire
   `bench:irq-boot-hart-only` and the `irq-boot-hart-only` kernel feature** (debug-only code
   whose one case is gone is not carried; nothing unused is mentioned). IRQ1's test becomes what
   already checks its mechanism exactly: `sched-lock-contention` and `-4` require a hart other
   than the boot hart to take ten or more alarms, `-4-mttcg` that every hart does, and
   `sched-irq-wake`'s proof reads each device interrupt against the runner's slice; boot.md's
   IRQ1 status and Tested-by, scheduling.md R78's "self-check of the targets" paragraph and the
   negative-control sentences, testbench.md's case list and SECURITY.md (C7: Tested-by equals the
   section) follow. The page states item 1 as the reason there is no negative at the targets. A
   section-relative negative (the wake in units of the section, the mttcg case's form) could see
   one section, but it would measure what the alarm counts count exactly; not built.
4. **The contention family: yes, as the node says.** `record=` comes off `sched-lock-contention`,
   `-4` and `-4-mttcg`; all three are judged at 15/50 ms net of audits. `-4-mttcg` loses its
   searches-ratio clauses (`driver_wake_p50_searches=3`, `p99=8`): a search of tens of µs is no
   unit beside host noise; its 15/50 ms in host time stand. `-4` under icount is judged too: the
   turn-order inflation was about five searches (113 ms), now about five of tens of µs. The hammer
   changes of necessity: "refused after a search of the whole area" is no longer R12's costliest
   call. It becomes the costliest bounded call R22's rewritten table names (expected the 16-page
   `map_fixed` with every table present, or `unmap`), chosen as ruled on "the contention family's
   hammer is chosen by its self-check": the program times one section alone, sizes its window from
   it, checks every wake fell inside, and prints the section it hammers; the hammer's line on R78
   and the tomls' descriptions follow. Figures re-recorded, no target moves. The lock-order clause
   (every wait in ticket order, behind fewer sections than harts) is the family's constant and
   stays on all three.

### B103: the two-hart deadline notice is judged where the harts run at once (architect-oct21)

Finding (.wash/local/B103-report.md): the term that grows is the steward's wake on the notice to
its `lease_ended` send returning; under icount the other hart's verity/blkd reads are counted
inside it (steward's hart 24.6 ms of "user" time at rv32 two harts against 0.5 at one; sections
read about 5x longer), and with the harts at once the real two-hart cost is +1.7 ms of lock waits
(R78's shape), whole notice p99 8.6-9.9 ms against 40. No kernel or steward term to fix.

**Ruling: (a)**, by the settled precedent (K31's Ruling A, `sched-capped-mttcg`; LAT4): a cell
QEMU's turns move is recorded under icount and judged where the harts run at once, the target
unmoved. (b) is refused: a ratio to a same-boot yardstick needs a yardstick outside the measured
path (R10 is inside the notice), and the target is a responsiveness figure in milliseconds, which
is what a person feels; host time's speed is stated as the caveat, as sched-capped-mttcg's page
states it. (c) leaves the cell judged by nobody.

1. **Both widths, not rv32 alone.** rv64 at two harts reads 35.2 ms under icount by the same
   mechanism (x2.2 in the same term); a pass with 5 ms of margin from a measurement artefact is
   no judgement. The icount case records the deadline-notice p99 at two harts on both widths
   (record=rv64:2:N=k and rv32:2:N=k, k = 1-3), judges it at one hart as today, and the twin
   judges it at two harts on both.
2. **The twin:** `steward-latency-mttcg`, no icount, host time, `--quiet` class, rounds N = 1-3,
   both widths at [1, 2], the deadline notice's p99 at 40 ms net; every other cell stays in the
   icount case (the twin's other figures recorded, not judged, so one case judges each cell).
   Lease lengths come from the twin's own measured launches, as the case already derives them
   (twice the round's worst launch), never from icount's. Before it lands: three runs a width at
   [1, 2] alone, N = 1-3 clean, the figures on the page with their spread.
3. **The twin must be shown able to fail.** A case that reads 9 ms against 40 and has never
   missed proves little: the report states what regression the twin sees at 40 ms and shows one
   run that misses it (a known-bad ordering, such as the notice sent after the per-exit cleanup
   that STEWARD12 removed, if a test-only feature can restore it cheaply), or states why no cheap
   negative exists and what the sensitivity is (the real two-hart cost is +1.7 ms; the twin sees a
   term of about 30 ms, not that).
4. **What B103 lands.** Title: the deadline-notice p99 at two harts is judged where the harts run
   at once: under icount the other hart's turns are counted inside the steward's send (rv32 44.9,
   rv64 35.2 ms against 40), and the real two-hart cost is +1.7 ms of lock waits. Acceptance: the
   twin judged at 40 ms net on both widths at [1, 2], N = 1-3, three alone runs each; the icount
   case's two-hart cells recorded on both widths with the turns reason in the toml; scheduling.md
   "With the real steward" carrying both sets of figures, the SCHED5 finding bullet replaced by
   the turns sentence, the residual "A hart waits for QEMU's turns" gaining the notice cell beside
   the share case; testbench.md's host-clock list; steward.md's Leases if it repeats the figure.
   No target moves. Not Tier A: a case and pages; the reviewer is the orchestrator's choice.

### B109: the companions follow B103; the case counts become a check (architect-oct21, 2026-10-22)

The red on B103 is right by B103's item 1: steward-latency-n4 and -n16 judge the two-hart
deadline notice under icount on rv64 (27.2 and 29.9 ms against 40) by the same turns artefact.
**A node, B109, after B103** (added, todo). Not an extension of steward-latency-mttcg: the
companions exist because the trace ring is per boot and N = 4/16 fit no boot beside N = 1-3, and
the twins meet the same ring, so they are two case files, as the companions are: n4-mttcg (rv64
[1, 2]; rv32 [1] as its companion) and n16-mttcg (rv64 [1, 2]), host time, --quiet, the notice's
p99 judged at 40 ms net, other figures recorded, leases from their own launches, the icount
companions recording their two-hart cells with the turns reason. The negative question is
inherited from B103's answer, not redone. Three alone runs per twin per width before landing;
N = 16's host-time cost stated. Pages: the real-steward table, the QEMU-turns residual,
testbench.md's host-clock list. No target moves.

**The stale case counts (187 of 352 against 425 files): no objection to a small node, with a
shape.** A hand recount goes stale again by the next train; the count belongs to the docs gate
(redoubt-doccheck already holds C7 and C11): a rule that the counts testbench.md states equal the
case files (total, boot cases, under icount), failing the docs case when they drift, as C7 fails a
SECURITY row. The node recounts once and adds the check, so the number is never written by hand
again. Not before AGENT0's chain; size S.
