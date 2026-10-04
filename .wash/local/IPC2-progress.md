# IPC2 progress

## Early checkpoint (deliverables 1 and 2), branch wp-ipc2 at d83508fa4 (WIP, to be folded)

### Design (no gap found; nothing improvised)
- Kernel: a new word in each thread's IPC page, `W_DUE` (word 16; `W_WORDS` moves to 17). At
  `send` it is set to the message's `W_SEQ` (arrival). `deliver` (delivered or refused, after the
  record re-check, as the cursor update was) draws `now = mm.next_seq()` and writes it to `W_DUE`
  of every message of the taken group still queued on that endpoint. `next_sender` takes the
  minimum by (due, group order, seq). So a group's turn = max(last take, oldest message's
  arrival), from the one counter `next_seq` (kernel-only, no process reads it). Ties to the lower
  group key. A group with nothing queued keeps nothing; there is no per-group table.
- `Endpoint::cursor` and its frame words go (`kernel/src/endpoint.rs`, endpoint `WORDS` 8+MAX_LABELS
  -> 4). endpoint.rs was not listed in owned paths but holds "the endpoint frame's cursor word"
  the brief names; the edit is only the cursor's removal and doc lines.
- I11: after a take the group's due is greater than every waiting group's, and a waiting group's
  due never changes until it is taken; new groups arrive later. So at most k-1 groups go first:
  the bound stays k receives.
- Model: `Msg::due` (arrival = message id from `next_msg`; take draws a fresh `next_msg`), a
  `served` helper used by delivery and refusal (calls `Ghost::took`, restamps the group).
  `next_sender` takes min (due, key, id) of each group's eligible head. The endpoint `cursor`
  stays in the model, read only by the new mutation `R2OneCursor` (old behaviour), which is in
  `Mutation::ALL` under R2.

### Commands (all via .wash/local/in-dev, from the worktree)
- `./build --arch rv32 --programs` -> exit 0 (no kernel warnings).
- `cargo test --release -p redoubt-model --test properties -- flood kernel_sequences` -> exit 0,
  both pass.
- `cargo testbench redoubt-ipc` -> exit 0: redoubt-ipc and redoubt-ipc-attack PASS rv64 and rv32.
- `cargo testbench ipc-outcomes` -> exit 0, PASS rv64 and rv32.
- `cargo testbench model-host-tests` -> exit 1: `mutations_are_caught` fails only because
  `R2OneCursor` is NOT CAUGHT (confirmed with REDOUBT_MODEL_MUTATIONS=R2). Expected until
  deliverable 3 (steward_noninterference extension) lands.
- `cargo +nightly fmt --all --check` -> exit 0.

### Next
Deliverables 3-5: extend `steward_noninterference` (vault approvals/denials, session ends,
crashes, both worlds), confirm it catches `R2OneCursor`; the bench case; pages (R2 status, residual
removed, I11 Kept in, todo deleted, doccheck), size budget check, whole bench, branch rebuild.

## Deliverables 3-5 (assignment dc0f59c7), branch tip d194277af (3 WIP commits)

### Done
- `W_DUE` write commented: one write per queued message of the taken group, at most WAIT_CAP;
  never process-readable (from `next_seq`).
- Model `Endpoint::cursor`: written by `served`, read only under `Mutation::R2OneCursor`.
- Deliverable 5, bench case `ipc-fair-label-sets` (tests/ipc-fair-label-sets.toml,
  tests/programs/src/bin/ipc-fair-label-sets.rs). Sole judging program (rule F, as
  endpoint-destroy-full): groups low=(1001,[]), vault=(1001,[9]), high=(1002,[]). Two rounds on
  fresh endpoints: high served; [vault served]; low then high queue; judge takes both. The judge
  reads the badges the kernel delivered (children only send and exit, hold no console).
  `cargo testbench ipc-fair-label-sets` -> exit 0, PASS rv64 and rv32, order [1,3] both rounds.
  Against main's kernel (message.rs/endpoint.rs/budget.rs checked out temporarily, then
  restored): exit 1 on both widths, with-vault order [3,1].
- Pages: ipc.md R2 status (partly tested text + bench:ipc-fair-label-sets), the cursor residual
  removed; invariants.md I11 status + Kept in rewritten for W_DUE/next_seq; SECURITY.md R2 and
  I11 rows' Tested by (doccheck C7 requires it). `cargo run -q -p redoubt-doccheck` -> exit 0.
  `cargo +nightly fmt --all --check` -> exit 0.

### Blocked: deliverable 3 (and so 4), thread IPC2-steward-family to the Architect
Detail: .wash/local/IPC2-steward-family-question.md. The thread's extension (vault approve/deny
as vault work; vault ends and crashes in both worlds) fails on the correct R2:
- seed 12: a spontaneous crash blames whichever call is in service, which depends on the vault's
  queued calls (timing, any turn order);
- seed 42: vault approvals start vault agents, shifting the owner's per-principal session counter
  (steward.rs `started`).
With call-caused crashes and vault approvals dropped, the family passes but R2OneCursor is not
caught. model-host-tests still exits 1 on R2OneCursor alone.

### Left after the ruling
Deliverables 3-4; then R2/I11 status lines name mutation:R2OneCursor (and model.md's R2
mutation row, SECURITY rows); todo page and its SUMMARY.md line deleted; model.md's
noninterference text and residual, kernel-attack-gaps.md lines 11 and 109 (outside owned paths,
lock step); whole bench; branch rebuild.

## Final (owner chose option (a), fba3bc92e; counter ruling cc6b697f2). Branch wp-ipc2 at e83cd04f9

Commits on a76994302:
1. b144ec481 model: the steward numbers sessions and agents per label set (size 10092->10093)
2. 98358590c model: a lease's end is audited with the lease's labels (size ->10095). Every audit
   record carries its request's labels (steward.md "The audit log"); LeaseEnded carried none, so
   a vault agent's lease end reached unlabelled readers (P10 seed 208). Needed for P10 to pass.
3. ce73f93ac testbench: the process case queues its first caller before the second
   (proc-test.rs). Its two-caller blame scenario relied on the old lowest-group-first order;
   under oldest-first the callers raced (whole bench: process FAIL both widths, blamed 43 not 42).
   Now 50 ms stagger; 3/3 runs PASS both widths.
4. e83cd04f9 kernel: a receive takes the group served least recently: kernel, model R2,
   R2OneCursor, P10 extension (option (a)), steward take log, bench case, pages, todo deleted,
   size 10095->10179.

P10 harness choices (test construction, no rule): ops naming a session that vault work started
are vault work; the session an op names is renamed into the without-world by the op that started
it (a vault label set's numbering follows its own history, which includes the vault's work);
results are never renamed. Crashes at an instant and crashes a vault's call causes are left out
(a vault-caused crash moves when the server takes earlier unlabelled calls: seed 52).

What P10 catches: with (a), R2OneCursor at seed 18 by the take-order check. Without (a)
(crashes all left out, no order check), R2OneCursor still caught at seed 14179 through an
unlabelled approval's kernel result. Every other mutation is still caught either way
(REDOUBT_MODEL_MUTATIONS=Policy,R2 exit 0; full model-host-tests exit 0).

Gates at the tip tree:
- whole bench `cargo testbench`: 264 PASS, 4 FAIL -> bench-ssh-loopback-openssh (podman absent on
  this host), process rv64/rv32 (fixed in 3), size-budget (fixed). After fixes: process x3 PASS,
  size-budget PASS at every commit, model-host-tests PASS (446.5 s, was ~240 s), unsafe-budget
  PASS, no unsafe added (0 lines).
- `./build --arch rv32 --programs` exit 0; fmt exit 0; doccheck exit 0.
- ipc-fair-label-sets PASS both widths; on main's kernel FAIL both (with-vault order [3,1]).

## Fix round 1 (assignment 6306aec3), branch wp-ipc2 at 08807fb8f on main 4ace1722d

Commits: 1c38cdc10 steward counter (size 10093); 72b949486 lease-end labels (+ steward.md
"request's or target's labels" and the lease-end sentence, red F4 / editor 4; size 10095);
f33f3b14f process case stagger; 08807fb8f R2 change with every other fix (size 10179).

Applied: red F1 (served_behind round: high x2 then low, expects [3,1,3]; passes both widths);
red F2 (order check compares whole unlabelled take lists incl. length); red F3 (steward.md R37
and residual, model.md: P10 replays crashes an unlabelled call causes; on its own or on a vault's
call = stated residual); red F4; simplifier 1 = gate `ep.cursor` write in served() under
broken(R2OneCursor) (keeps the mutation exactly today's cursor; a restamp-all would be a
different rule, lowest-key-first); still caught at seed 18. simplifier 2/3: Steward::take()
helper logs takes, serve/hold use self.reply(). Timing: session-id sets not session clones,
take-order lists built incrementally, audit views compared directly (no Debug format).
Editor 1, 2, 3, 5 applied (message.rs comment; R2 per-message due sentence and re-wrap; status
lines + SECURITY R2/I11 residual text).

Timing: host load 60-117 on 24 cores, so wall times are not comparable. P10 alone, same load:
user CPU 17m35s before, 17m11s after (-2%): P10's per-op checks were not the cost.
model-host-tests after: 497.5 s wall, 69m45s user (before: 446.5 s wall, at lower load).
Not measured: main vs tip CPU of the whole suite (checkout main fails, it is the shared
checkout's; use `git checkout 4ace1722d`).

Gates after the fold (all via in-dev): fmt --check 0; doccheck 0 (tip and lease commit);
ipc-fair-label-sets 0 (both widths); process 0 (both widths); model-host-tests 0;
size-budget PASS at tip. Whole bench --allow-skip NOT run after the fixes.

## Acceptance gates (assignment 596b34e7, ipc2-implementer-2), wp-ipc2 at 08807fb8f on main 4ace1722d

All via .wash/local/in-dev from the worktree; tree clean; no commit changed.
- `cargo testbench --allow-skip`: exit 0. 268 PASS, 0 FAIL, 1 SKIP (bench-ssh-loopback-openssh:
  podman not installed). ipc-fair-label-sets PASS rv64/rv32; process (+attack, chain-fault,
  lifecycle, review) PASS both widths; model-host-tests PASS 441.2 s wall; size-budget PASS;
  unsafe-budget PASS. Log: /tmp/ipc2-bench.log.
- `./build --arch rv32 --programs`: exit 0.
- `cargo +nightly fmt --all --check`: exit 0.
- `cargo run -q -p redoubt-doccheck`: exit 0.
- unsafe: `git diff 4ace1722d..HEAD` touches no line containing `unsafe`; counts unchanged
  (kernel core 19, arch 13, Sv39/SBI/PLIC 13, loader 18, paging 12, redoubt-sys 1; 0 undocumented).
