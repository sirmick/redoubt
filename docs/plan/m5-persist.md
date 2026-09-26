# M5 (persist, install, share)

## Goal

The box keeps running and changing without losing what it is:
- the steward's state survives reboots, and the capabilities it minted are re-minted after a
  restart;
- principals and their keys are added and removed at run time, and the first owner is enrolled on
  the physical console;
- principals install signed packages, choose whose code they run with trust lists, and share
  projects;
- the system updates A/B, with rollback protection;
- the service supervisor starts and restarts installed services;
- the box has wall-clock time, kept in sync;
- the audit log is chained, verifiable offline, and kept for a set time.

## Attack suite

- **No new authority without trust.** An unsigned or untrusted archive is refused before any parse
  (a fuzz corpus confirms the parser is never reached); code runs with a manifest's grants only if a
  key on the principal's trust list signed it
  ([R71 (no new authority without trust)](../servers/pkg.md#r71-no-new-authority-without-trust)).
- **Removing a key takes effect.** Removing a signer from a trust list stops that signer's running
  processes and leaves the principal's `use` records unchanged
  ([packages](../servers/pkg.md#signer-trust-and-granted-authority)).
- **A hostile package stays put.** An archive signed by a trusted key cannot write outside its
  principal's package directory; a module named like a system module is refused, and a `.beam`
  planted in a session's home is never loaded by name
  ([R74 (a hostile package stays in its principal's packages)](../servers/pkg.md#r74-a-hostile-package-stays-in-its-principals-packages)).
- **No rollback.** A bundle older than the version counter kept outside both slots does not boot,
  and a user's crash loop cannot force a rollback, because health is judged before any session
  starts ([R72 (no rollback below the counter)](../servers/pkg.md#r72-no-rollback-below-the-counter)).
- **A restart never widens.** A service restarted by the supervisor holds exactly its recorded
  grants and no more ([R73 (a restart never widens)](../servers/supervisor.md#r73-a-restart-never-widens)).
- **Persistence adds nothing.** State read back after a reboot re-mints only what was recorded, and
  a server that restarts keeps no authority of its own
  ([the servers](../servers/README.md#restarts-and-crash-blame)).
- **The audit log catches tampering.** A record dropped, reordered or edited is found by the
  offline verifier ([the steward](../servers/steward.md#retention-chaining-and-the-verifier)).
- **Membership is revocable.** Removing a project member destroys that member's revocation scope,
  and every capability minted into it dies
  ([R41 (narrowing by revocation scope)](../servers/steward.md#r41-narrowing-by-revocation-scope)).

## Remaining work

In this order, after [M4 (self-hosted development)](m4-self-hosted.md):

1. **The steward's persistence**, run-time principals and first-owner enrolment
   ([the steward](../servers/steward.md#persistence-run-time-principals-and-enrolment)), with
   `budget_children`, which the restarting steward uses to find what it made
   ([budgets](../kernel/budgets.md#budget_children)), and keys sealed across boots
   ([keyd](../servers/keyd.md#sealed-keys-labelled-keys-and-keys-in-leases)).
2. **Wall-clock time and time sync** ([the timer](../kernel/timer.md#wall-clock-time-and-time-sync)).
3. **Packages**: the signed format, the pkg server, per-principal packages, profiles and trust
   lists ([packages](../servers/pkg.md), [installing](../userland/packages.md)), and user signing
   keys ([development](../userland/development.md#signing-keys)).
4. **Projects and sharing** ([the steward](../servers/steward.md#projects-and-sharing),
   [files](../userland/files.md#sharing-a-directory)).
5. **The service supervisor** ([the supervisor](../servers/supervisor.md)).
6. **System updates**: A/B slots, M-of-N signatures, the version counter
   ([packages](../servers/pkg.md#system-updates)).
7. **Audit retention, chaining and the offline verifier**
   ([the steward](../servers/steward.md#retention-chaining-and-the-verifier)).

## Progress

Nothing of this milestone is built. What it builds on: the signed bundle and its verification at
boot ([boot](../kernel/boot.md)), `keyd`, and `fsd`'s power-loss guarantee
([R50 (power loss leaves before or after)](../servers/fsd.md#r50-power-loss-leaves-before-or-after)),
all built and tested.
