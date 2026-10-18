# ATK13 report: the last two scripted rows

Branch `wp-ATK13`, worktree `.worktrees/ATK13`, base `4096f3861` (origin/main), head `7ab7887ed`,
one commit, clean tree. Reviewer: the orchestrator's red pick.

## Delivered

- `tests/steward-labelled-agent-no-sink.toml` (new). On `steward-approve-box`'s path: alice's
  agent submits a request for an agent labelled 7 (alice-secrets), alice approves it at
  `approve@box` with the screen's code, the labelled agent runs in its vault on the order the
  vault session staged (`nosink`). Its record: `[net: false, net_clone: {:error, :not_found},
  far: {:error, :enetunreach}, listed: {:error, :enetunreach}, loopback: {:error,
  :econnrefused}]`. The agent then returns, so its lease ends on its own (`exited 0`,
  `LeaseEnded`, `{7}` back to 10 processes).
- `userland/shell/lib/redoubt/agents/no_sink.ex` (new): `Redoubt.Agents.NoSink`, dispatched by
  `Redoubt.Agents.Echo` on the order `nosink` (one line in `echo.ex`). Probes: `/net` in the
  namespace table, `/net/tcp/clone` lookup, `gen_tcp.connect` to 10.0.9.101:80 (the bench's
  second peer), to 10.0.2.15:22 (the box's listed address), to 127.0.0.1:22.
- `docs/plan/m3-agents.md`: the R60 row names `net-attacks` for ipd and the new case for the
  agent, with the two walls; the R44 row cites keyd's four host tests (run by `r4-host-tests`),
  states why no boot arm exists (keyd's badges are sshd's `ssh_host` and the steward's `audit`;
  none reaches a session or lease until keys in leases, keyd.md section + M6 page), and names a
  login's end-to-end host-signature check (`steward-ssh-two-principals`) as the boot evidence.
- `docs/userland/agents.md` "Leases": status line gains `bench:steward-labelled-agent-no-sink`;
  the no-network bullet cites it for a labelled agent.

## Why the verdict is the system's (rule F)

- The bench peers' counts: 10.0.9.100:80 counts exactly 1 (alice's unlabelled session's control,
  with a `/net`, echoed "control"); 10.0.9.101:80 counts 0 (the agent's target). The agent cannot
  write the peer's record.
- The steward's audit and usage lines in `1001/{7}`: Submitted, Approved (key, hash),
  AgentStarted, 12 processes, LeaseEnded, 10 processes: the labelled agent genuinely ran and
  ended, from the approval path.
- The agent's record is read from `/vault/out/result` by alice's vault session, from the volume
  the agent cannot leave; the agent has no console and prints nothing the case reads. The record
  is the brief's stated verdict; the peers' counts are the independent one.
- No timed claim.

## The row's resolution (the checkpoint clause)

The labelled agent cannot even attempt the sink: with no `/net` in its namespace, beamlet's
`gen_tcp` runs the in-VM loopback stand-in (`userland/otp/vm/lib/beamlet_tcp.erl`), which answers
`enetunreach` for any non-loopback address and `econnrefused` for a loopback port with no
listener in that VM, before any 9P request. So no request of the agent's reaches ipd; the
namespace is the first wall and R60 the second, tested by `net-attacks`'s labelled caller.

Consequence for the pages: ipd.md R60's status line and SECURITY.md's R60 row are **unchanged**:
the case does not exercise R60, and adding it would be a false claim (and doccheck C7 keeps the
two equal). keyd.md R44 is unchanged: its status already lists the cited host tests; the reason
for the absent boot arm lives on the m3-agents row, as the brief asked. SECURITY.md untouched, so
no collision with ATK12.

## Gates (all through `make -f scripts/jobs.mk -C .worktrees/ATK13`, q leases)

| gate | result |
| --- | --- |
| `set CASES=steward-labelled-agent-no-sink` ×3 | rv64 PASS 11.6 s / 11.6 s / 11.2 s; rv32 PASS 10.5 s / 14.7 s / 10.6 s; rc=0 each |
| `set CASES=steward-approve-box` | rv64 PASS 14.8 s, rv32 PASS 17.6 s |
| `set CASES=session-tcp-loopback` | rv64 PASS 7.5 s, rv32 PASS 9.4 s |
| `set CASES=net-attacks` | rv64 PASS 0.4 s, rv32 PASS 0.5 s |
| `set CASES=size-budget`, `unsafe-budget` | PASS, PASS |
| `set CASES=formatting` | PASS 25.5 s (before the case fix), 26.6 s (after) |
| `set CASES=docs` | PASS 4.3 s, 1.9 s, 1.5 s (after each docs edit; last on the committed tree) |

Logs: `.tmp/ATK13/gates1.log`, `case2.log`, `gates2.log`, `gates3.log`, `pass-rv64.log`.

Two earlier failing runs (`.tmp/ATK13/case1.log`, `rv64-*`/`rv32-*` logs kept) were my expect
patterns, not the attack: the shell wraps `{:result,` onto two lines (now `\s+`, as approve-box's
`{:asked,` pattern), and alice's read of her unlabelled agent's `{:ok, %{request: N}}` raced that
agent's write (dropped: it is `steward-approve-box`'s claim). In both failing runs the labelled
agent's record, `exited 0` and `LeaseEnded` were already on the console.

## Summaries checked

- `docs/userland/sessions.md` 190-194: a *vault session* has a `/net` and ipd answers `eacces`;
  a *labelled lease* has no `/net` and gets `enetunreach`. Distinct and both right; no change.
- `docs/userland/beamlet.md` 583-588 (the stand-in), `docs/servers/steward.md` 743-748 (the
  lease batch: no vault, no network, no console), `docs/plan/m3-agents.md` Progress ("a lease,
  with no `/net`, has no network"): consistent; no change.
- README.md, GETTING-STARTED.md, image/README.md: no claim affected.
- `docs/SECURITY.md` R60/R44 rows: unchanged, reasons above.

## Not run / risks

- No whole bench (the train's). No steward-latency. No server, kernel or steward code touched.
- `unsafe` count: unsafe-budget PASS; no Rust changed.
- The case's `timeout_secs = 1800` is approve-box's; it runs in ~12 s.
