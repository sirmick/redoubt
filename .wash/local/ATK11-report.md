# ATK11 report: the scripted hostile user at the shared servers

Branch `wp-ATK11` off `origin/main` 23795a6e4. Four attack cases, each judged from the victim's
side (rule F), no server or kernel code touched. Bounds measured once per width from the probe's
own printed elapsed ms, with the arithmetic in each toml's comment.

## Cases (each PASS rv64 + rv32, 3x each; zero failures)

1. **`tests/walfsd-flood-turn.toml`** (R2, R26). Bob seeds a file and floods `walfsd:data` from
   MAX_WAITERS (6) processes reading without pause; Alice, a separate principal and so a separate
   waiting group, overwrites and reads back a 64 KiB file in her home throughout. Each probe
   returns within 10 s. Verdict: Alice's shell prints `{n, 65536, <ms>}` for n=1..8 within the
   bound. Worst single probe operation measured: 1.87 s rv64, 1.54 s rv32 (bound = ~5x the larger
   rounded up, far below a starved op which would not return while the flood runs).

2. **`tests/vault-flood-turn.toml`** (R2, R37). Alice's unlabelled session seeds a file; her vault
   session `alice+alice-secrets` (may read the unlabelled volume) floods `walfsd:data` from 6
   processes; the unlabelled session runs the probes of (1). R2 and R26 key the vault's
   `{alice-secrets}` group apart from the unlabelled `{}` group of the one principal, so the bound
   is the same 10 s. Worst probe: 1.42 s rv64, 1.52 s rv32.

3. **`tests/blkd-fairness.toml`** (R2 at blkd). Bob floods `walfsd:data` with endless 512 KiB
   writes from 6 processes; Alice's vault session writes and reads back a 64 KiB file through
   `walfsd:alice-secrets`. Both walfsd instances share one `blkd` (image/disk.toml) and are
   account-0 callers keyed by their own budgets, so R2 at blkd gives each its turn. Each probe
   returns within 10 s. Verdict: the vault shell prints `{n, 65536, <ms>}`. Worst probe: 1.50 s
   rv64, 1.48 s rv32.

4. **`tests/net-pinned-ssh.toml`** (R28, R4a). Alice's session opens one `gen_tcp` connection to
   the peer over `/net` and makes 64 reads, each a 20 ms `recv` her own timeout abandons, every one
   of which ipd answers and frees; Bob logs in over SSH while she parks; then Alice's echo on the
   same connection round-trips. Verdicts (all from the victim/peer): `{:parked, 64}` (no read
   refused at once for a full share), `{:echo, {:ok, "ping"}}`, Bob's `sshd: login bob` line
   (expect_after), and the peer's one connection with no SYN to the box's own addresses
   (`[net.peer] connections = 1`, `self_forbidden`). No time bound: the 20 ms abandon is the shape,
   not a tuning.

The flood is shown real in each: the attacker's shell prints `{:flooding, 6}` (its process count
reaching the hub's MAX_WAITERS), or alice prints `{:parked, 64}`.

## Gates (all PASS)

- new cases both widths, 3x each: walfsd-flood-turn, vault-flood-turn, blkd-fairness,
  net-pinned-ssh — 3/3 PASS on each of rv64 and rv32.
- `set CASES=formatting` PASS; `set CASES=docs` PASS; `size-budget` PASS; `unsafe-budget` PASS.
- regression unchanged in verdict: `net-pinned` (rv64+rv32), `steward-sub-budget-flood`
  (rv64+rv32), `steward-ssh-two-principals` (rv64+rv32) — all PASS.
- steward-latency not run (STEWARD5/7 hold that slot).

Logs: target/jobs/<w>-<case>.log; console logs under target/testbench/run-* (probe elapsed ms in
`*-alice*.ssh.log` / `*-alice+alice-secrets*.ssh.log`).

## Pages edited (my rows / the cases where a rule is proven)

- `docs/plan/m3-agents.md` "A scripted hostile user": the four rows gain their case ids (Endpoint
  flooding -> walfsd-flood-turn; vault WAIT_CAP -> vault-flood-turn; Pinned open calls -> + net-pinned-ssh;
  System fairness -> blkd-fairness). Edited only these four rows (ATK10/ATK12 own the others).
- `docs/kernel/ipc.md`: R2 summary + 3 bench bullets (tested 10 -> 13); R4a tested list + net-pinned-ssh.
- `docs/servers/serving.md`: R26 summary + walfsd-flood-turn, vault-flood-turn (tested 16 -> 18);
  R28 summary + net-pinned-ssh (tested 4 -> 5).
- `docs/servers/steward.md`: R37 tested list + vault-flood-turn.
- `docs/servers/ipd.md`: "Pinned and abandoned calls" tested 5 -> 6 (+ net-pinned-ssh); corrected the
  stale "no Elixir /net reader exists today" to the session's gen_tcp read, with net-pinned-ssh.
- `docs/SECURITY.md`: R2, R4a, R26, R28, R37 rows gain the new bench cases.

## Affected summaries checked

Searched docs/ for other references to these rows or case names: none outside the six pages above
(no book-index or milestone page lists them). No further summary change needed.

## Review fold by the orchestrator (steward-red-4, OK with notes; the bound was the orchestrator's call)

- The three probe cases asserted a 10 s step bound against worst measured probes of 1.42-1.87 s
  (about five times). Ruled the house rule: the verdict is the probe's own elapsed ms under 4,000
  (twice the worst measured, rounded up to a second), asserted by the expect's digits
  (`[0-3]?[0-9]{1,3}`); the step's 20 s bound is the bench's margin for the line to arrive over
  ssh on a loaded host, no part of the claim (a first round with the step bound itself at 4 s
  missed blkd-fairness 3 of 6 under full load while every probe's own ms was 1.6-1.9 s).
- Reruns after the fold under a fully loaded machine (STEWARD5's gate set, STEWARD7's latency run,
  ATK12's boots beside): 18 of 18 PASS over three rounds on both widths; worst probe 1,923 ms
  (walfsd-flood-turn rv64), blkd-fairness worst 1,617 ms, vault-flood-turn worst 1,866 ms.
- m3-agents.md's row said "10,000 sender threads"; it now says a hostile principal's every hub slot
  calling walfsd without end (the in_flight cap, 6 here, is the pressure the server sees).
