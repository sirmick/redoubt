# A browser GUI

## Idea

A desktop in the browser over one WebSocket. `webd`, in Rust, terminates TLS, authenticates, and
routes each person's WebSocket to their session; a router in the session multiplexes channels to
application processes and holds the window state. The applications and the router are Elixir in
the session VM, so the OS itself contains no graphics code.

## Why it is not a goal

It conflicts with a non-goal: no display, keyboard, mouse or GUI
([the tenets](../TENETS.md#non-goals)). It is recorded rather than dropped so that, if the non-goal
is ever amended, the design starts from its known finding: the desktop is drawn by the untrusted
session VM, so a browser can show one request while approving another. A passkey signs a hash the
person never sees.

## What it would need

- The no-GUI non-goal amended first, with the reason.
- **The GUI never approves.** It may say that an approval is waiting; the approval itself stays on
  its out-of-band channel ([R38 (out-of-band approval)](../servers/steward.md#r38-out-of-band-approval)).
- `webd` as an inbound service reached only through SSH forwarding, like the
  [web stack](web-stack.md).

**Attack cases:** nothing drawn by a session appears on an approval; a session cannot reach another
person's WebSocket.
