# IPC2-steward-family: the extended P10 cannot pass with R2's new turn order alone

Branch wp-ipc2 (kernel and model R2 = least recently served, all checkpoint gates green).
Extension as the MOD1 thread states it: vault approve/deny are vault work (with-world only);
vault session ends, Hold, Crash and CrashServing run in both worlds. Command:
`in-dev cargo test --release -p redoubt-model --test properties -- steward_noninterference`.

## Finding 1: a spontaneous crash blames whichever call is due, and that depends on vault work

Seed 12, op 75 (CrashServing, run in both worlds; correct R2, no mutation). Queued before it:
vault calls from ops 53, 56, 72 (vault Work, with-world only) and an unlabelled call from op 70
(account 1001, []). The crash op takes one message and crashes holding it. With the vault's work,
the least recently served group is a vault group whose call arrived first (ops 53/56 precede 70),
so the vault is blamed (filtered from the unlabelled audit view). Without it, the only waiting
call is 1001's, so 1001 is blamed. Unlabelled audit view differs: an extra
`Blamed { account: 1001, labels: [] }` without the vault.

This is independent of the turn rule: any rule serves the vault's earlier calls first, so a crash
at an instant blames a different call. The single cursor made it worse; least-recently-served does
not close it. R2's guarantee (one label set's *order* is invariant) holds; *which slot* is being
served at a crash is not invariant. The Architect's answer on MOD1-r2-cursor-blame rejected (b)
("if the vault's queued work suppressed or moved blame, that would itself be the leak"): here it
moves blame under the new rule.

## Finding 2: vault approvals shift the owner's unlabelled session ids (steward, not R2)

Seed 42, op 40 (owner's unlabelled Login) returns a different session id. Ops 24-39: a vault
session submits AgentWithLabel and the owner approves it (vault work), which starts a vault agent.
`steward.rs` names sessions by a per-principal counter `started[principal]` ("session-n", and the
id mixes it), so the owner's next unlabelled session is numbered after the vault agent. In the
real steward ids are CSPRNG, but the per-principal name counter is a real counter.

## Experiment

A crash *caused by a call* (CrashServing{s}: the server answers what it takes until it takes s's
call, then crashes holding it; Hold/Crash dropped; vault approve/deny dropped): passes 10,000
seeds on the correct rule, but `R2OneCursor` is then NOT CAUGHT. The cursor's leak shows only
through spontaneous crash blame, which the new rule also leaks. (Copy:
/tmp/ipc2-policy-exp-callcaused.rs.)

## Options

(a) Scope P10's crashes to crashes caused by a call (finding 1), with spontaneous-crash blame
    written as a timing residual of R37 (it is service-slot timing, like latency); add to P10 an
    observation of the order the server takes unlabelled calls, which R2 promises invariant and
    which catches R2OneCursor. Weakens R37's text: owner?
(b) Keep R37 strict: blame for a crash only when its cause is attributable (spontaneous crash
    blames nobody?) -- a steward/R21 change, not this package.
(c) Finding 2: steward names sessions per (principal, label set), or by CSPRNG only; a steward
    change outside IPC2's paths, or P10 compares session ids up to renaming.

IPC2 can land deliverables 1, 2, 4 (with the observation in (a)), 5 and the pages once (a) or
another ruling is given; deliverable 3 as written cannot pass.
