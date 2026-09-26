# Unattended operation

## Idea

A box that runs for long periods without anyone at it: backups of its volumes, crash records kept
across reboots, updates applied in the field, monitoring from outside, and a rescue console for
when the system does not come up.

## Why it is not a goal

Every milestone assumes a person nearby: the first owner is enrolled on the physical console, a
failed update falls back to the other slot at the next boot, and a crash is reported on the
console and to the steward. M5 (persist, install, share) keeps state across reboots and restarts
services, which is the base for all of this, but none of it is needed to meet the hard goals.

## What it would need

- **Backup:** a capability to read a volume's snapshot, held by a backup service, with labelled
  volumes backed up only to a labelled destination, or encrypted under a key that stays in `keyd`.
- **Crash records:** exit notices and panic reports written to the steward's audit log, surviving a
  reboot, readable under the label check.
- **Field updates:** the A/B mechanism with updates fetched through a gateway capability and
  verified by the loader, never by the fetcher ([packages](../servers/pkg.md#system-updates)).
- **Monitoring:** a read-only capability to resource use and service state, per principal, with no
  global counters ([the shell](../userland/shell.md#resource-use)).
- **A rescue console:** a trusted path on the physical console that can boot the other slot or a
  recovery image, reached without the steward.

**Attack cases:** a backup of a labelled volume never lands where an unlabelled principal reads it;
monitoring shows no other principal's activity; a fetched update that fails verification never
boots; the rescue path is not reachable from a session.
