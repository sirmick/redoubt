# IPD2: a person's sockets at `ipd` are the manifest's: alice's account gets 32

Node IPD2 (parent AGENTS). Tier A (`ipd`'s admission and sizing, and the serving library's
override rule in `libs/rt`), size M (the owner's answer said S; the mechanism it assumed does not
reach a session, below), reviewer: a red (kernel-red-4 or the orchestrator's ipd red), with the
simplifier and editor as Tier A. Base: origin/main 453a48bfc. Owner's answer (Q4 in
`.wash/local/owner-questions-2026-10-18.md`, "Owner's answers"): alice's `net` gets 32 sockets;
ipd's entry re-derived by the rule with the arithmetic on ipd.md; bob unchanged. Ruling:
`.wash/local/wave-2026-10-18-rulings.md` "Q4 → IPD2". Independent of SCHED5, B94, B95, WEIGHT1.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs` (C11: no package ids in
  cases or comments). No edit and no git operation while prebuilt builds or cases run. Copy a
  failing job's log aside before a rerun.
- **A `libs/rt` contract change is built against every consumer** before any boot: every crate
  using `Admission`/`Override` (`servers/ipd`, `servers/consoled`, `libs/rt`'s ninep,
  `tools/testbench`; grep, don't trust this list) builds and runs its host tests.
- Messages at most 2000 bytes; detail in a file. Read long pages by section.
- Pages carry no dates or ids; a number on a page shows its arithmetic. A miss is a finding, not
  a tuning. TIME-WAIT counting stands (a closed socket keeps its unit until it is gone): do not
  touch it.

## Why the existing override does not do it

`limits=BADGE:INFLIGHT:STATE:SOCKETS` overrides an **account-0 root badge's** bucket
(`SocketCaps::of`, `AdmitKey::root()` is `Some` only for account 0; serving.md "Overrides").
A session's `/net` is minted by the steward's grant and, from its first request, admitted in the
**session's account's** bucket (serving.md "A minted connection is its holder's"): alice's
sockets are account 1001's, at the defaults (8 sockets, 4 + 8 `State`), shared by her sessions
at `pool / (n + 1)`. So the image's alice has about three per session beside the console's.

## The rule (the ruling's; design within it)

1. **An account's bucket may have caps of its own, from `ipd`'s arguments.** A new argument
   `account=ACCOUNT:INFLIGHT:STATE:SOCKETS` (ACCOUNT non-zero, decimal, each once, at most 8, the
   same fields and parsing as `limits=`) gives that account's **unlabelled** bucket those caps;
   a labelled bucket of the account keeps the defaults (`ipd` refuses labelled callers, R60, so
   it holds no socket). The manifest is the one source: the number lives in `ipd`'s arguments,
   as `sshd`'s and the steward's do; no principal key, no init or steward change.
2. **In the serving library**: `Override` is keyed by an account-0 root badge, as today, or by a
   non-zero account's unlabelled bucket; the worst case counts each override slot at the larger
   of its caps and the default's, unchanged. The share rule within the bucket (`pool / (n + 1)`)
   is unchanged: alice's sessions still divide her bucket fairly, and an agent never holds a
   socket. serving.md "Overrides" states the second key and why (a person's capacity is the
   manifest's, per account, as the root-badge form is per daemon).
3. **Sizing re-derived by the rule** (ipd.md "Sizing", the entry rule): one override bucket at
   5 / 4 / 32, sockets as `State` units (`state_for` 4 + 32), fids `files_for(32)` = min(72,
   `MAX_FIDS` 64) — state that the fid cap binds live sockets below 32 (2 fids each) while
   TIME-WAIT sockets hold no fid, which is the churn this is for; `SOCKET_BYTES` 17,408 each;
   `max_sockets` grows by the override's units over a default slot's. Compute the image's need
   (today seven defaults, 4,988,928 bytes, 1,218 pages) with alice's slot, the pages with the heap
   cap and the 128-page stack, against the entry's 4,096 (3,894 of budget). The heap cap (74) is
   said to hold "every socket's buffers": check that against `max_sockets` × `SOCKET_BYTES`; if the
   cap does not hold the new worst case, raise `heap_pages` and the entry's pages by the rule
   (twice the measured peak, testbench.md "The memory budget" table's `ipd` row re-measured), and
   if today's cap already does not hold today's worst case, that is a finding: report it with the
   arithmetic before fixing it. The arithmetic goes on ipd.md and in `servers/ipd/tests/sizing.rs`.
4. **The image**: `ipd`'s args gain `account=1001:5:4:32`; bob (1002) has no `net` and no
   override. Every other manifest unchanged unless a case needs it.

## Cases

- Host (`redoubt-ipd`, `redoubt-rt`): the account form parses and is refused malformed, zero,
  twice, or past 8; an account override applies to its unlabelled bucket only, a labelled bucket
  of the same account and another account keep the defaults; sizing with the image's args
  (the assertion with its arithmetic); the root-badge form unchanged.
- Bench, new, both widths: `session-tcp-churn` (a name of your choosing, stated): alice's SSH
  session beside her console session opens, echoes and closes connections to the bench's peer in
  succession, more than the old three within TIME-WAIT (say eight within 10 s, the number derived
  from her share at 32 with the console session holding its connection: state it), every connect
  answered; then the share's own bound is still met: the connect past her session's derived share
  while the others linger is `{error, emfile}`. The verdict is the peer's (its connection count and
  capture) for the successes and `ipd`'s `too_many` for the refusal (rule F).
- Unchanged in verdict, both widths: `session-tcp-echo`, `session-tcp-loopback`, `net-tcp`,
  `net-pinned`, `net-attacks`, `sshd-refuses-slots`, `steward-six-principals`, `agent-login`,
  `memory-host-tests` / the memory cases that read `ipd`'s heap record; the short gate.

## Pages

ipd.md "Sizing" (the argument, the arithmetic, the status line's new tests); serving.md
"Overrides" (the account key and its reason); beamlet.md "A session's sockets are its share" (the
"three sockets" sentence: what the image's alice now gets, with its arithmetic, and that a
principal without an override gets the defaults); sessions.md only if it states a socket count;
testbench.md's memory table if `ipd`'s heap cap moves; SECURITY.md R26 rows if their tests change.
Summaries checked: say which.

## Owned paths

`libs/rt/src/server/admit.rs` (hotspot: every server's admission; keep the change additive),
`servers/ipd/src/{args,sizing,fs,server}.rs`, `servers/ipd/tests/`, `image/manifest.json` (ipd's
entry, line ~54; WEIGHT1 edits the steward's, line ~84, in the same file: rebase whichever lands
second), the new toml, the pages above. Not the steward, not init, not the kernel.

## Checkpoint

After the host tests and the sizing arithmetic: one note on the QA thread `IPD2-sizing` with the
numbers (need, pages, heap cap against `max_sockets`), before any boot.

## Report

Each gate with its exit code; the arithmetic; what was deleted; the case's derived count.
