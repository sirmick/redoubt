# Hardware approval

## Idea

High-stakes approvals signed by something the requester's machine cannot fake:
- a FIDO security key (an SSH `sk-` key) on a fresh `approve-hs@box` connection that accepts only
  the approver's credential, with the signature's user-verification flag checked, not just the key
  type;
- or a physical button or small display on the board ([the FPGA platform](fpga-platform.md)).

## Why it is not a goal

Every milestone's approvals happen on `approve@box`, a channel the steward alone drives
([the steward](../servers/steward.md#the-powerbox-and-approvals)). That protects the
approval from the requester on the box. What it does not protect is a person's own compromised
client machine, which a hardware key or a button on the board would; no milestone assumes that
threat.

## What it would need

- `sshd`'s support for `sk-` keys checked, including the user-verification flag.
- The high-stakes approvals named, and the steward refusing them on any other channel.
- For a button, a trusted display showing what is being approved, driven by the steward alone.

**Attack cases:** an `sk-` signature without user verification is refused; a high-stakes approval
on the ordinary channel is refused; nothing a session sends reaches the board's display.
