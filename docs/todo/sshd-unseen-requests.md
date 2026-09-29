# Requests the bench cannot see against sshd

## What

[Against Redoubt's sshd](../testbench.md#against-redoubts-sshd) gives its verdicts from `ssh`'s
exit status and the server's log. Three requests fall outside both:

- **`window-change`.** OpenSSH's `ssh` sends it only when its standard input is a terminal whose
  size changed, and the session runner gives it pipes, so no case sends one. `sshd`'s host tests
  send it with `sunset`'s client (`a_window_change_without_a_size_is_refused` and
  `a_window_change_over_the_largest_is_cut_to_it` in `servers/sshd/tests/core.rs`).
- **`env`.** `ssh` sends it (`SetEnv`) without wanting a reply, and the core refuses it without
  telling the platform, so the refusal shows nowhere. `sshd-loopback-r67` sends one, and can show
  only that the session still gets no shell and starts no console. `sunset`'s client cannot send
  `env`, so the host tests do not either.
- **Agent forwarding.** `ssh` asks for it without wanting a reply, and `sunset` refuses it without
  a word, so no case can see the refusal.

## Why it matters

The window size is one of the raw numbers the core checks before a session sees it, and `env` and
agent forwarding are requests the core must refuse; the bench is the independent client, and
today it never sees the first sent or the other two refused.

## Where

- `tools/testbench/src/ssh.rs`: the runner would give a `pty = true` session's `ssh` a
  pseudo-terminal of its own and resize it on a step (say `{ resize = [132, 43] }`), which needs
  `openpty` and so a small, documented `unsafe` or a crate that wraps it.
- `servers/sshd/src/lib.rs`: for `env` and agent forwarding, the core would tell the platform
  what it refused (a request's name), which the host platform logs; agent forwarding needs
  `sunset` to hand the request out rather than refuse it itself.

## Done when

- A case resizes a session's terminal and the server's log shows the console's new size, and one
  with a size of 0 shows none.
- A case sends `env` and asks for agent forwarding, and the server's log shows each refused.
