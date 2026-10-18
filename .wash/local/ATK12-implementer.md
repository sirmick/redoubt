# ATK12: resources, a hostile launch from a session, and leaky state

Node ATK12 (parent `agent-suite`). Tier A (attack cases; the bench). Size M. Reviewer: a red,
the orchestrator's pick. Ruling: `.wash/local/wave-2026-10-18-rulings.md` "The agent-suite
step". Pages: docs/plan/m3-agents.md "A scripted hostile user" (three rows you fill),
docs/servers/littlefsd.md "R48 (a quota per attach root)", docs/servers/walfsd.md "Quotas",
docs/servers/init.md "R31"/"R32", docs/servers/steward.md "R37 (vault non-interference)",
docs/kernel/processes.md "R36"/unpredictable ids, docs/testbench.md "Rule F". Patterns:
`tests/steward-home-quota.toml`, `tests/stub-launch.toml` (a malformed ELF), `tests/
process-attack.toml`, `tests/steward-vault-session.toml`.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run. Copy a failing job log aside before a rerun.
- Messages at most 2000 bytes; detail in a file. Read pages by section.
- A verdict comes from a party the attacker cannot impersonate (rule F): a server's refusal
  line, the kernel's, the victim's own saved value, or a canary; never the attacker's output.
- A flaky test is a bug; a case orders on events. You touch no server or kernel code; a finding
  goes to the Architect on a QA thread. The shared page is m3-agents.md's table: your rows only.

## The cases

1. **`walfsd-pools`** (shared pools: filling the `data` volume does not fail Alice's saves;
   flooding walfsd with handles does not grow its table). Bob fills his home to its quota and
   opens fids until his per-attach-root cap, each over-cap open refused by name; alice saves to
   her home throughout. Verdicts (the `steward-home-quota` shape): bob's writes hit `enospc` /
   his open hits the cap refusal (walfsd's lines), alice's saves succeed (her session's value),
   and walfsd's budget holds no more pages after than before (its usage line): a quota is per
   attach root and a handle flood takes no memory beyond the cap (R48, R26).
2. **`session-hostile-launch`** (a malformed ELF or startup block from a user parent hurts only
   the child). Alice's session launches, from her home, a malformed ELF and an image cut short
   (stage the bytes as `stub-launch` does); each launch is refused before the child runs or the
   child faults alone. Verdicts: the client's refusal is named (the launch returns the error,
   read in alice's session), or the kernel's fault line names the child's PID alone, and alice's
   session answers at its prompt after (R31, R32). From a user parent is the row's "not yet"; the
   kernel's half is `stub-launch`/`process-attack` already.
3. **`vault-no-leak`** (no leaky state while a vault works). Alice's unlabelled session records a
   stated list of observables before, during and after her vault session writes files, launches
   a program and submits a request: her sub-budget's usage (`budget usage`), a fresh request id
   and session id, the PID of a program she launches, her home volume's qids, file versions and
   a directory listing, and (if reachable) an approval notification count. None changes with the
   vault's work, and the unlabelled session cannot write, truncate, create or remove in the
   vault's volume (it has no connection to it: `:enoent`/refused). Verdicts: the recorded values,
   read in the unlabelled session, equal across the vault's activity; the write attempts are
   refused by walfsd (R37, R25, R36).

**The observable list is the case's claim and goes on the page** (steward.md R37's section or
m3-agents.md's row): it is the list of things an unlabelled observer can see, asserted not to
move. An observable that *does* change with the vault's work is a finding reported to the
Architect with its mechanism (which call, which field), never struck from the list to make the
case pass. If you find one, stop and send it; it may be a design hole (R37) or a kernel id leak
(R36), and the ruling decides.

## What each verdict rests on (the reviewer checks)

- Every verdict is the victim's saved value, a server's refusal, or the kernel's line; the
  attacker's output is never the proof.
- (3)'s observable list is complete enough to matter (usage, ids, PIDs, qids, versions,
  listings, notifications) and is on the page; the case reads each from the unlabelled side only.
- The hostile launches in (2) are genuinely malformed (the bytes staged, not a valid image), so
  R31/R32 are exercised.

## Pages

- m3-agents.md "A scripted hostile user": the three rows gain their case ids.
- littlefsd.md R48, walfsd.md "Quotas", init.md R31/R32, steward.md R37: status lines gain the
  cases; R37's section gains the observable list if that is its home. SECURITY.md rows for R48,
  R31, R32, R37, R36 grow. Summaries: say which.

## Owned paths

`tests/walfsd-pools.toml`, `session-hostile-launch.toml`, `vault-no-leak.toml` and their
manifests/staged data (the malformed images under `tests/data/`); the pages above. Not
`servers/`, not `kernel/`, not `libs/`, not `tests/steward-latency.toml`.

## Gates

Each case both widths; `set CASES=formatting`, `set CASES=docs`; `steward-home-quota`,
`stub-launch`, `steward-vault-session` unchanged in verdict; `size-budget`, `unsafe-budget`.
Report each with its exit code and the verdict line per case; for (3), the observable list and
that every value held.

## Checkpoint

After the first case passes both widths, one progress note; for (3), the observable list before
you build it, so the Architect confirms it is complete.
