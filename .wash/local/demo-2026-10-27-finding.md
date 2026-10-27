# Finding from the owner's demo (2026-10-27): a session's VM aborts on a failed allocation the budget could afford

Image: `./launch --system` on rv64, 4 harts, origin/main ebf7ed881. Logs and the driver:
`.tmp/demo/` (`console.log`, `alice2.out`, `alice2b.exs.raw`, `bis1..5.*`).

**What happens.** In an ordinary SSH session as alice, typing a `defmodule` after using the shell's
file helpers ends the session: the VM prints `panicked at .../alloc.rs:665:9: memory allocation of
324144 bytes failed` and exits 101 (`steward: lease alice/session-N ... exited 101`, init's
`Terminating process`). The session is gone; the compile does not fail as an error.

**Reproduction (deterministic).**

| session | before the compile | result |
| --- | --- | --- |
| compile alone | nothing | ok, `Demo.fib(20)` in 32 ms after a 3.2 s compile |
| `File.write("c.txt", "x")` | plain OTP write | ok |
| `File.ls("/home/alice")` | listing | ok |
| `w(["x"], "c.txt")` then `free()` | the shell's helper | **abort, 1,048,576 bytes**; `free()` had just shown 4,281 of 11,392 pages free |
| `w` + `checksum` | helpers | abort, 324,144 bytes |
| `mkdir_p`, `w`, `append`, `cat \|> grep`, `table` | helpers | abort, 324,144 bytes (twice) |

So the failing allocation is far below the session budget's free pages; the limit hit is the VM
process's own heap cap (MEM2's allocator cap) or its arena, after the helpers' modules are loaded.
Two defects: (1) the compile path allocates without asking first (BEAM16/the VM-asks packages
covered heap growth, binaries and natives, not this path), so a refusal is a Rust abort, not an
Erlang error; (2) whatever the helpers leave behind makes a modest compile exceed the cap.

**Raise as a node** (Tier A, beamlet): identification first: which allocation (the compiler's
binary, code loading, the beam pack), against which cap; then the compile fails as
`{:error, :enomem}`-style in the session instead of killing it, and the shell-compile bench case
gains a run after the file helpers.

Also seen, expected: outbound TCP from the launcher's guest is refused (`-netdev user,restrict=on`),
port 22 is refused by alice's `net` policy (`eacces`); the vault session cannot write alice's home
(`permission denied`) and has no network (`enetunreach`).
