# Fair waiting by least recently served group

## What

[R2 (fair waiting)](../kernel/ipc.md#r2-fair-waiting) takes the oldest message of the group served
least recently. The kernel and the model still keep one round-robin cursor per endpoint over every
group, of every label set: `next_sender` and `deliver` in `kernel/src/message.rs`, and the
endpoint's `cursor` in `model/src/kernel.rs`.

## Why it matters

A vault session's call taken by a shared server moves the cursor, so the next unlabelled group
served depends on the vault's work. A crash of that server is blamed on the call it was serving
([R21 (crash blame)](../kernel/processes.md#r21-crash-blame)), and the blame is an audit record an
unlabelled reader may read and the input to logging an (account, label set) out
([R40 (blame by label set)](../servers/steward.md#r40-blame-by-label-set)). That is an intentional
path across a label boundary, which
[R37 (vault non-interference)](../servers/steward.md#r37-vault-non-interference) forbids. The
model's `steward_noninterference`, extended to vault approvals, vault session ends and crashes,
finds it in a few operations.

## Where

- `kernel/src/message.rs`: `next_sender` (the rank after the cursor) and `deliver` (which moves
  it); the endpoint frame's cursor word.
- `model/src/kernel.rs`: the endpoint's `cursor`, and `Ghost::took`, which checks I11 (fair turns).
- `model/src/policy.rs`: `steward_noninterference`, extended.

## Done when

- The kernel and the model take the head of the least recently served group. Each waiting group
  carries when its turn became due: its oldest message's arrival, or its last take, whichever is
  later, from one kernel counter no process reads. Ties go to the lower group key.
- The bound of I11, within k receives, still holds; `flood` and `Ghost::took` check it.
- `steward_noninterference` with vault approvals, vault session ends and crashes passes.
- A mutation that keeps one cursor across label sets fails it.
- [I11](../kernel/invariants.md#i11-fair-turns)'s "Kept in" names the new code, and
  this page is deleted.
