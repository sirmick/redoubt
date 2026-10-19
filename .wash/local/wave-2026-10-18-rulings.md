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
