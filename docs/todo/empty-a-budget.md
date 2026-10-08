# A dead steward's carves are emptied before its restart

## What

`init`'s rule for a dead steward cannot log its sessions out: the steward carves every
principal's budgets under `users`, and when it dies those carves are reachable by no handle
anyone still holds. `users` cannot be destroyed and made again, since a child's class is its
parent's and `budget_create` takes no class
([class is trust, not order](../kernel/budgets.md#class-is-trust-not-order)): `users` is the
kernel's, class `user`, made at boot, and anything `init` makes under `root` is class `system`.
`budget_reap` empties a budget one child at a time and keeps it
([the calls](../kernel/budgets.md#the-calls)), but `init` does not call it on `users` before it
starts the steward again. So a restarted steward finds `users` not empty and exits, and `init`'s
restart rule ends in a reboot ([init](../servers/init.md#restarts-and-reboots)).

## Why it matters

A steward crash costs every session and the machine, where it should cost the sessions only:
fail closed, but wider than it needs to be.

## Where

- `init`'s restart of the steward (`servers/init/src/bin/init.rs`), which would reap `users`
  until it has no children before starting the steward again.

## Done when

- `init` empties `users` when the steward dies and starts it again; the `steward-restart` case
  shows the console session back without a reboot.
- steward.md's and budgets.md's residuals say what the code does, and this page is deleted.
