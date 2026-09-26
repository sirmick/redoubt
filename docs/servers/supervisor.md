# The supervisor

The supervisor keeps long-running services up after boot: services a principal or the system runs
beyond `init`'s fixed set of system servers. It starts each from a record, restarts it when it
exits within limits the record sets, keeps its log, and gives it exactly the authority its record
names on every start. It is the place for services that come and go at run time, as `init` is for
the servers the boot manifest names.

## Purpose

`init` starts and restarts the system servers the signed manifest names, and nothing else. Once the
steward keeps state and principals install packages, some code must run as a service: a
principal's own server, a project's build worker, a scheduled task. Without a supervisor each would
need a session kept open to run it, or its own restart logic. The supervisor gives them one
mechanism, under the same rules as every launch: the loader stub, fresh connections, and grants no
wider than their owner's.

## Interface

### Services

Status: planned · M5 (persist, install, share)

- **A service record** names an installed package's program, its owner (a principal), the grants its
  owner made for it from the package's requests, a budget carved from the owner's, and a restart
  policy. The steward keeps the records, since they carry grants.
- **Starting** a service is a launch like any other: a budget carved from the owner's, fresh
  connections for exactly the recorded grants, through the loader stub
  ([init](init.md#launching-through-the-loader-stub)).
- **Restarting.** When a service exits, the supervisor starts it again with the same record, up to the
  policy's limit in a window; past it, the service stays stopped and its owner is told.
- **Logs.** A service's console output is kept in a log its owner can read, bounded in size.
- **Control.** A service's owner can start, stop and inspect it; nobody else can.

**Open:** what the supervisor supervises (principals' services only, or also system services
after boot); whether it is part of the steward or a server of its own; how services are scheduled
to run at times; the restart policy's form; where logs live and how they are bounded.

## Authority

Status: planned · M5 (persist, install, share)

- The supervisor holds what it needs to launch: budgets to carve from, as the steward gives it, and
  connections to the servers whose fresh connections it asks for.
- A service holds exactly the grants its record names, never its owner's whole set.

**Open:** whether the supervisor holds budget handles itself or asks the steward for each launch.

## Security properties

### R73 (a restart never widens)

Status: planned · M5 (persist, install, share)

Every start of a service, first or restarted, gets exactly the grants and budget its record names,
through fresh connections; nothing a previous run held is handed to the next. So a service that was
compromised and crashed comes back with its recorded authority, not more.

**Open:** none.

## Failure and restart

Status: planned · M5 (persist, install, share)

- **A service crash-loops:** past its restart limit it stays stopped, and a crash blamed on another
  principal is the steward's to judge ([steward](steward.md#crash-blame)).
- **The supervisor restarts:** services keep running; it rebuilds its view from the steward's records
  and the exit notices it holds.

**Open:** how the supervisor learns of services still running after its own restart.

## Residual risks

- **A service runs unattended.** It spends its owner's budget and grants while nobody watches; its
  lease and budget bound it, not a person.
- **Logs are a channel between runs.** What a service wrote to its log is readable by its owner, and
  by nothing else.

## Why

- **Separate from `init`.** `init` runs the fixed set of servers the signed manifest names; services
  that principals add at run time need records that change, which belong with the steward's state,
  not in the boot's.
- **The same launch as everything else.** A supervisor with its own way of starting code would be a
  second place for launch rules to go wrong.
