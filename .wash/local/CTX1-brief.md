# CTX1: named session contexts over SSH

Owner's request, 2026-10-08. `ssh alice@box` gets alice's default context. `ssh alice.work@box` gets a second context, `work`: its own VM in its own budget. A context survives disconnect, and reconnecting with the same name reattaches to its running VM. `exit` at the prompt ends the context.

## Syntax

`principal[+label][.context]`, in that order only.
- `alice` is the default context.
- `alice.work` is the context `work`.
- `alice+tax.work` is the context `work` in the `tax` label set.

The separator is `.`, not `:`. scp, sftp and rsync split `user@host:path` at the first `:`.

## Rules: each one is an acceptance test

1. **Canonical parsing.** One order only; `alice.work+tax` is refused. Principal names may not contain `+` or `.`, and init's manifest check refuses one that does. The charset stays lower-case ASCII letters, digits, `_` and `-`, at most 64 bytes per part. No case folding.
2. **Authentication uses the principal alone.** The label and context are chosen only after it. Labels and contexts are per principal, and the steward checks this; it does not trust sshd's parse.
3. **No enumeration before authentication.** Refusals for an unknown principal, an unknown label or an unknown context look the same and take the same time.
4. **A context's identity is (principal, label set, name).** `alice.work` and `alice+tax.work` are different contexts. A labelled context's console never attaches to a channel of a different label set. sshd's channel labels check this a second time.
5. **One attachment at a time.** A second login to an attached context takes over. Both terminals are told, with the time and the client's address.
6. **A cap on live contexts per (principal, label set).** Beyond it, a login is refused; the oldest is never evicted. Failed authentication stays cheap.
7. **Reserved names take no suffix.** `approve+x` and `approve.x` are refused. init refuses principal names that collide with reserved ones.
8. **Steward restart.** A context name never reattaches to something that merely reuses the name. Either contexts end with the steward (K23's reap), or they are re-adopted by an identity the steward keeps (an id and a generation). The design checkpoint decides which, with the cost of each.
9. **Detached console.** Output goes to a bounded buffer and is replayed on reattach. Past the bound it is dropped with a count, which is shown on reattach.

## Lifecycle

- Closing SSH detaches; the VM keeps running.
- `exit` at the prompt ends the context.
- An idle timeout ends a context left detached. It is configurable per principal in the manifest, with a default.

## In the shell

- `contexts()` lists the session's own principal's contexts, in its own label set only.
- `detach()` closes the channel and leaves the context running.
- Ending another context is limited to the same label set. The steward grants this through a handle; it is a decision for the design checkpoint.

## Where the work is

- sshd parses the name.
- The steward keeps a context table and handles attach, detach, the console buffer, the cap and the restart rule.
- The shell gets its three commandlets.
- Pages: sessions.md (How to use it; Logging in; a new Contexts section), steward.md, sshd.md, and the M2 page.

Tier A: the steward red reviews.

## Cases

Host tests in sshd and the steward for each rule. Machine cases on both widths:
- reattach after disconnect (state kept);
- takeover (both terminals told);
- the cap (the next login refused, the oldest kept);
- a labelled context refused on an unlabelled login;
- the steward's restart, by the rule chosen.
