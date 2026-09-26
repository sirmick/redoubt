# Scheduling extensions

## Idea

- **Time donation:** a server thread runs on its caller's budget for the length of a call, so the
  caller pays for the work it asked for.
- **CPU quotas:** a budget may use at most so much CPU time in a window, beyond its weight's share.

## Why it is not a goal

Servers pay for their own CPU, and a busy server delays others only by its own weight
([R12 (scheduling)](../kernel/scheduling.md#r12-scheduling)). Donation is intricate, and a donated
server thread stopped mid-call by its caller's lease or quota could hold the server's locks for
good ([scheduling](../kernel/scheduling.md#residual-risks)). Leases already bound time by deadline
and weight.

## What it would need

- Measured priority inversion that hurts a milestone's target, which is the reason to add either.
- For donation: a server that can be stopped at any point of a donated call without holding
  anything another caller needs, or a donation that ends at a point the server chooses.
- For quotas: a rule for what a quota-exhausted budget's open calls do, since its callers are
  waiting.

**Attack cases:** a caller cannot make a server run on the caller's budget longer than the call; a
caller's lease ending mid-call leaves the server able to serve others.
