# IPD2 report: a person's sockets at ipd are the manifest's; ipd's heap holds every socket

Branch `wp-IPD2` in `.worktrees/IPD2`, base `453a48bfc`. Head and commits: see the final section.
The ruling: `wave-2026-10-18-rulings.md` "Q4 → IPD2" and "IPD2-sizing" (options 1 + 3).

## What was delivered

**Commit 1: serving library + ipd argument.**
- `libs/rt/src/server/admit.rs`: `Override` gains `account: u64`, keyed as `AdmitKey` is:
  `(0, root badge)` as before, or `(account != 0, badge 0)` for that account's unlabelled bucket.
  - `Override::covers(&AdmitKey)` is the one matching rule, used by the admission's `cap` and by
    ipd's `SocketCaps::of`.
  - `with_overrides` refuses an override that is neither form, or names a bucket twice. The
    worst-case rule and the share rule are unchanged.
  - **Deleted:** `AdmitKey::root()`. Its last caller was `SocketCaps::of`.
- Consumers' literals gain `account: 0`: `servers/consoled/src/server.rs` (`init_override`),
  `libs/rt/fuzz/fuzz_targets/ninep_server.rs`, `servers/ipd/src/fake.rs`.
- ipd `account=ACCOUNT:INFLIGHT:STATE:SOCKETS`:
  - parsed as `limits=` is (fields <= 64); the account is non-zero, appears once, at most 8
    beside at most 8 `limits=`, and needs no scope;
  - `Limit` gains `account`;
  - `SocketCaps` holds the `Override`s.
- `tests/size-budget.toml`: the `libs/rt` ceiling goes from 4,106 to 4,110, with a
  `Size budget:` line in the commit (the four code lines of `account` / `covers` / the checks).

**Commit 2: heap term, backstop, image, case, pages.**
- `servers/ipd/src/sizing.rs`:
  - `sizing(budget, heap_pages)` refuses unless `heap_pages` >= `heap_pages_for(max_sockets)` =
    `HEAP_BASE_PAGES` (74) + ceil(max_sockets x 17,408 / 4,096);
  - with an uncapped heap (0), the budget must hold need + that heap;
  - the budget's `State` cost is now `CONNECTION_BYTES` (512): each socket is counted once. The
    Cost change stays inside ipd's sizing, so there is no residual.
- `servers/ipd/src/stack.rs`: `new_socket` is fallible (`try_reserve_exact` on both buffers),
  answering `too_many` and setting `heap_short`.
  - `listen` makes its whole backlog before changing any state, and on failure removes what it
    made.
  - `refill` stops at a failure.
- `servers/ipd/src/bin/ipd.rs`: reads the heap cap from the startup block, and says
  `HEAP_SHORT` once.
- `tools/testbench/src/case.rs`:
  - `ALWAYS_FORBIDDEN` gains `ipd: an invariant broke`, so every boot case forbids the backstop
    line;
  - the per-entry manifest check asserts `heap >= sizing.heap_pages`, or for an uncapped heap
    counts it in the budget.
- Manifests:
  - **image:** `account=1001:5:4:32`, heap 533;
  - the 11 capped test entries get their heap derived (beamlet-footprint and
    beamlet-compile-footprint 278; boot-profile-unverified, confined, latency, many-sessions,
    three-contexts, vault-launch 431; pipe, idle 380; six-principals 644);
  - the 23 uncapped entries already fit inside their budgets.
- `servers/ipd/fuzz/fuzz_targets/args.rs`: the new `sizing` signature. It was already stale,
  calling `sizing()` with no arguments, and it asserted a scope for every limit; an account entry
  now has badge 0.
- New case `tests/session-tcp-churn.toml`.

## The arithmetic

| | Old (main) | New |
| --- | --- | --- |
| Default bucket | 712,704 B | 509,952 B (State at 512) |
| Alice's bucket | — | 542,720 B (40 fids + 24 units more) |
| Image need | 4,988,928 B, 1,218 pages | 3,602,432 B, 880 pages |
| `max_sockets` | 84 | 108 (6 x 12 + 36) |
| Heap cap | 74 | 74 + 459 = 533 |
| Entry use | — | 880 + 533 + 128 = 1,541 of 4,096 |

- **Fids.** `files_for(32)` = min(72, 64) = 64. With two roots in three shares, each session has
  about 20 working fids, so fids bind live sockets near 10. A TIME-WAIT socket holds no fid.
- **The case's count.** Alice's SSH session beside her console session (each session's `/net`
  is one working `State` unit in its own share) has floor(36 / 3) = 12 units: its connection and
  **11 sockets**. At the defaults it was floor(12 / 3) - 1 = 3.
- **Finding (main, reported first on IPD2-sizing).** The heap cap of 74 pages held about 8
  sockets above ipd's idle peak of 37, while the caps allowed 84 (357 pages). A socket past it
  panicked ipd. Alice's two sessions at 3 sockets and bob's lone session at 5 make 11, reachable
  at today's defaults. That figure is derived, not booted.
- **Measured.**
  - A socket is 2 large blocks of 2 pages, plus smoltcp's storage of 432 B
    (`size_of::<SocketStorage>()`, host 64-bit).
  - ipd's heap peak in the memory cases is unchanged at 37 pages (27 at boot), so the base of 74
    stands.
  - ipd's stack peak went from 13,672 to 13,736 bytes on rv64; the declared 7 pages are still
    more than twice that, and the table is updated.

## The new case: session-tcp-churn (both widths)

- **What it does.** One shell expression: 11 connects to 10.0.9.100:80, each echoed and closed,
  then a twelfth while those 11 sit in TIME-WAIT (10 s). Expected output:
  `{:churn, {11, {:error, :emfile}, true, MS}}`. MS was 1,110 on rv64 and 950 on rv32, against
  the 10 s bound.
- **Verdict (rule F).** The peer counts exactly 11 connections and the capture shows 11 SYNs to
  it, none to the box's own addresses. So 11 succeeded, and the twelfth was refused inside ipd:
  `too_many`, which the shell prints as `emfile`.
- **On main** (453a48bfc, scratch worktree, rv64): **FAIL**, session step 3, `MatchError` on
  `{:error, :emfile}`. ipd refuses a connect during the churn at the default share of 3, so on
  main the case fails as a refusal. Logs: `.tmp/IPD2/runs/rv64-session-tcp-churn*.log`.

## Gates

### Final head

The final head is **62c048249**:
- `24cbee6d3`: serving, the override keyed by an account, and ipd's `account=`; Size budget
  libs/rt 4,106 to 4,110.
- `62c048249`: ipd's heap term, backstop, image, case and pages; Size budget servers/ipd 2,926 to
  2,966.

The base is **453a48bfc**.

### Short gate (scripts/q, jobs.mk) on tree 00a50259f

00a50259f differs from 62c048249 only in the two size-budget ceilings, the testbench.md
stack-peak cell, and commit messages.

| Gate | Exit |
| --- | --- |
| `cargo test --no-fail-fast -p redoubt-rt -p redoubt-ipd -p redoubt-consoled -p redoubt-keyd -p testbench` | 0 |
| `libs/rt/fuzz: cargo check --bins` | 0 |
| `make build-rv64`, `make build-rv32` (`./build --programs`) | 0, 0 |
| `make prebuilt` | 0 |
| `make -k set CASES=...` | 2 |

The `set` covered 32 names, 64 results: 61 PASS, 3 FAIL.

The 3 failures:
- **size-budget**: `libs/rt` over its ceiling. Raised in commit 1, which also exposed
  `servers/ipd` over; raised in commit 2.
- **rv64 steward-three-contexts**: `walfsd: volume poisoned: create: blkd answered Failed`, a
  device fault under load. **PASS alone** in 8.0 s.
- **rv64 shell-commands**: mark `plain-ready` timed out. **PASS alone** in 19.2 s.

Their logs are kept in `.tmp/IPD2/failed/`.

### PASS on both widths in the set

- **Static checks:** docs, formatting, unsafe-budget, no-cruft.
- **Smoke set:** userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear,
  lend-untouched-page.
- **Own case:** session-tcp-churn.
- **Unchanged in verdict:** session-tcp-echo, session-tcp-loopback, net-tcp, net-pinned,
  net-attacks, sshd-refuses-slots, steward-six-principals, agent-login.
- **Memory cases:** userland-read-only, steward-session-ends, steward-ssh-two-principals.
- **Changed-manifest cases:** beamlet-footprint, beamlet-compile-footprint,
  boot-profile-unverified, steward-context-idle, steward-many-sessions, steward-vault-launch,
  steward-confined-push, steward-latency.
- **Passed on rv32, and on rv64 when rerun alone:** steward-three-contexts, shell-commands.

### On 62c048249 after the last amend

`make prebuilt` 0; `make -k set CASES="size-budget docs formatting unsafe-budget no-cruft"` 0,
all PASS.

### Not run

- The whole bench: the train's job.
- The other changed-manifest cases with long runs (steward-latency-n4/n16, the deadline-burst
  pair, launch-overlap, lease-*, session-hostile-launch, vault-no-leak). Only their ipd heap cap
  changed, and a cap reserves nothing until used.

## Summaries checked

- **ipd.md** "Sizing": updated (argument, the two rules, the arithmetic, backstop, status 19).
  "Failure and restart": updated (heap; its status names the program test).
- **serving.md**: "Overrides" updated (second key and why); `admit` status 17; R26 status gains
  the case and the heap test (25), summary and prose updated.
- **beamlet.md** "A session's sockets are its share": updated (11 with the override's
  arithmetic; 3 at the defaults without one).
- **testbench.md**: always-forbidden list (four patterns, two places); memory table ipd row
  (heap cap 533, with the reason; stack peak 13,736).
- **SECURITY.md** R26 row: the same set as R26's status line (+ case, + heap test).
- **sessions.md**: states no socket count, so no change.
- **servers/README.md**: "Agents never get sockets" is unchanged and still true.
- **README.md, GETTING-STARTED.md, the plan pages**: no socket or ipd-sizing claims (grep), so
  no change.
- **init.md** "Heaps": its rule (cap >= 2 x peak; the entry rule) still holds, and ipd's stricter
  term lives on ipd.md. No change.

## Open risks

- **Socket table growth.** The SocketSet vector at 108 sockets (about 46 KB, doubling) is covered
  by the base's margin over the peak (37 pages), not by a separate term.
- **The 1,024 B per socket** for smoltcp state is measured on 64-bit only; rv32 is smaller.
- **The heap rule binds every capped ipd manifest.** A new manifest that copies an old ipd entry
  with heap 74 fails the bench check, which is the intent.

## Round 1 fold (62c048249 -> da1b2338a)

Range-diff: `.wash/local/IPD2-range-diff-r1.txt`. Commit 1 (24cbee6d3) is unchanged; commit 2 holds
the fold.

### Findings

- **editor P2.** Commit 1 alone passes size-budget: on a scratch worktree at 24cbee6d3,
  `cargo testbench size-budget` exits 0, with servers/ipd at 2,923 of 2,926 and libs/rt at 4,110
  of 4,110. Commit 2 raises servers/ipd 2,926 -> 2,973, and its Size budget line names only
  commit 2's content.
- **simplifier P2.** ipd.md "Failure and restart" gains a bullet: the backstop is the decision,
  with its reason (a failed allocation in a shared server stops it for every client; the line
  makes the broken invariant a failure every boot case sees).
- **red P3-1.** smoltcp's SocketSet and ipd's table are sized at start, `max_sockets` and
  2 x `max_sockets`, so neither grows. A compile-time assert in stack.rs: slot + 2 entries <=
  `SOCKET_BYTES - 2 x BUFFER`. Measured on 64-bit: slot 432, entry 240, so 912 <= 1,024. For the
  image the tables are 98,496 B; ipd.md derives it. A listener lasts only while its backlog holds
  a socket, so entries <= 2 x sockets.
- **red P3-2.** A residual on ipd.md with its arithmetic:
  - records at the budget's costs: 6 x 51,200 + 83,968 = 391,168 B = 95.5 pages, against the
    base's 37-page margin;
  - every record table grows with `try_reserve`, so a full heap refuses `no_memory`, never a
    panic;
  - the follow-up is to count the records in the heap term.
- **red P3-3.** The servers/ipd/fuzz `cargo check` is in the gate: exit 0.
- **simplifier P3a.** `Sizing::heap_holds` and `Sizing::budget_need` are used by `sizing()` and by
  the bench's manifest check.
- **simplifier P3b, P3c.** I did not have their text (only the orchestrator's summary), so no
  change was made for them.
- **editor P3a.** testbench.md's memory prose says why ipd's cap is not twice its peak, and the
  row is re-measured: stack 13,768; heap peak 61 (59 on rv32), which is 37 + 24 of socket tables.
  The base of 74 is twice the peak apart from the tables.
- **editor P3b.** Commit 2's body names the ipd args fuzz repair.

### Gates

On c7c4c159d (the code as at da1b2338a), all exit 0:
- `cargo test --no-fail-fast -p redoubt-rt -p redoubt-ipd -p redoubt-consoled -p testbench`;
- servers/ipd/fuzz and libs/rt/fuzz `cargo check --bins`;
- build-rv64 and build-rv32;
- prebuilt;
- `set CASES="size-budget unsafe-budget no-cruft formatting init-boot userland-boot
  userland-read-only steward-session-ends steward-ssh-two-principals session-tcp-churn
  session-tcp-echo session-tcp-loopback net-tcp net-pinned net-attacks sshd-refuses-slots"`:
  28 PASS.

On da1b2338a (testbench.md row/prose and the commit message only): prebuilt 0, and size-budget,
docs and formatting PASS.

## Round 2 fold (da1b2338a -> 279eac93d)

The range-diff is in `.wash/local/IPD2-range-diff-r2.txt`. Commit 1 (24cbee6d3) is unchanged;
commit 2 is amended.

### Findings

- **red P3-b.** The stack holds its table to `max_entries` = 2 x `max_sockets`, its capacity from
  the start.
  - `room()` requires an entry for each socket, which covers `allocate` and `refill`.
  - `listen` checks `len + backlog` before making any socket.
  - Past the bound, a socket or a listen is refused `too_many`, so the table never grows.
  - New host test: `a_full_table_refuses_a_listen_or_a_socket`. It builds listeners left with no
    backlog (accepted at a full cap) past the sockets, then shows the listen and the socket
    refused.
  - The ipd.md claim and the stack.rs comment are corrected: a listener can outlast its backlog.
  - Sizing status 19 -> 20.
- **red P3-a.** The backstop line is now described as reachable only at the records' residual.
  - ipd.md Residual: it is one heap, so records past the margin (several buckets at their request
    caps, never one person's alone) can leave a socket's buffers no room, and the line is reachable
    only there; every boot case forbids it since none reaches the residual.
  - "Unreachable" is reworded the same way in ipd.md (Sizing, Failure), testbench.md, `stack.rs`,
    `bin/ipd.rs` and `case.rs`.
- **editor P3.** Commit 2's body is rewrapped to 100 columns, with no line over 100. It also drops
  "unreachable" and names the table bound and its test.
- **Size budget.** servers/ipd 2,926 -> 2,978 ("fifty-two lines"); commit 2's line covers the
  table bound.

### Gate on 279eac93d

All exit 0:
- `cargo test --no-fail-fast -p redoubt-ipd -p testbench`;
- servers/ipd/fuzz `cargo check --bins`;
- prebuilt (both widths);
- `set CASES="size-budget docs formatting session-tcp-churn net-tcp"`: all PASS on both widths.
