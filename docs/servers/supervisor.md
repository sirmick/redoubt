# The supervisor

The supervisor keeps long-running services up: it starts each from a record, restarts it when it
exits within limits the record sets, keeps its log, and gives it exactly the authority its record
names on every start. Which services it supervises, principals' own or also the system's after
boot, and whether it takes over restarting from `init` or runs beside it, are open.

## Purpose

`init` starts and restarts the system servers the signed manifest names. Once the steward keeps
state and principals install packages, some code must run as a service: a principal's own server,
or a project's build worker. Without a supervisor each would need a session kept open to run it,
or its own restart logic. The supervisor gives them one mechanism, under the same rules as every
launch: the loader stub, fresh connections, and grants no wider than their owner's.

## Interface

### Services

Status: planned · M5 (persist, install, share)

- **A service record** names an installed package's program, its owner, the grants its
  owner made for it from the package's requests, a budget carved from the owner's, and a restart
  policy. The steward keeps the records, since they carry grants.
- **Starting** a service is a launch like any other: a budget carved from the owner's, fresh
  connections for exactly the recorded grants, through the loader stub
  ([init](init.md#launching-through-the-loader-stub)).
- **Restarting.** When a service exits, the supervisor starts it again with the same record, up to
  the policy's limit in a window; past it, the service stays stopped and its owner is told.
- **Logs.** A service's console output is kept in a log its owner can read, bounded in size.
- **Control.** A service's owner can start, stop and inspect it; nobody else can.

**Open:** what the supervisor supervises (principals' services only, or also system services after
boot), and whether it takes over restarting from `init` or runs beside it; whether it is part of the
steward or a server of its own; whether services can be started at set times; the restart policy's
form; where logs live and how they are bounded.

## Authority

Status: planned · M5 (persist, install, share)

- A service holds exactly the grants its record names, never its owner's whole set.
- Whatever the supervisor holds to launch, it holds no `system`-class budget handle: only `init` and
  the steward do
  ([R33 (no server holds a system budget)](init.md#r33-no-server-holds-a-system-budget)).

**Open:** whether the supervisor holds handles to its owners' budgets itself or asks the steward for
each launch.

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
- **The supervisor restarts:** it holds none of its earlier exit notices; it rebuilds its records
  from the steward's.

**Open:** how the supervisor learns which services are still running after its own restart.

## Residual risks

- **A service runs unattended.** It spends its owner's budget and grants while nobody watches; its
  budget and grants bound it, not a person.
- **A restart does not clean what the service wrote.** A compromised service that planted state in
  files it may write finds that state again when it restarts; a restart resets authority, not data.

## Why

- **The same launch as everything else.** A supervisor with its own way of starting code would be a
  second place for launch rules to go wrong.
- **Records with the steward.** A service's record carries grants, and grants are the steward's to
  keep.
