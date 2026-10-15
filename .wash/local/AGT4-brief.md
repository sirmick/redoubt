# AGT4 brief: a `defmodule` at the prompt overflows the session VM's first-thread stack

## The symptom

AGT2's finding (`.wash/local/AGT2-report.md` "Finding"; logs `.worktrees/AGT2/.tmp/AGT2/run1`
and `run2/agt2-probe-*`): a three-line `defmodule` at the shell prompt over SSH, in or out of the
machine mode, ends the session's VM: `PROGRAM HALT: CPU Exception on PID 420: Store page fault
of 0x7ffecae8 at 0x002e2b70`. A probe at the plain prompt faults identically (PID 211, the same
two addresses: the recursion is deterministic). `{:one, 1 + 1}` before it is fine. Host beamlet
compiles the same block. No machine case compiles a module at the prompt.

## What the address is

- The first thread's stack sits under `0x8000_0000` (memory-layout.md:224-226: "first thread's
  stack (up to 128 pages launched; boot reserves top 32 pages)").
- `0x7fff_ffff - 0x7ffe_cae8 = 79,127` bytes = 19.4 pages below the top.
- The steward launches a session VM's first thread with `SESSION_STACK_PAGES = 18`
  (servers/steward/src/bin/steward.rs:105-107: "twice beamlet's measured peak, 36,136 bytes"),
  the peak `beamlet-footprint` measured (testbench.md:1636), a case that compiles nothing. The
  helper schedulers get `SCHEDULER_STACK_PAGES = 18` too (userland/otp/redoubt/src/lib.rs:648-650).
- So the fault is the first scheduler thread's native stack overflowing below its lowest mapped
  page: the compile needs more than 72 KiB of native stack on this VM. The term walks are
  iterative (userland/otp/vm/src/term/cmp.rs:65; `deep_terms_are_handled_iteratively`);
  something on the compile path is not, or the interpreter's per-call native frame times the
  compiler's depth exceeds it.

## Step 1: name the frame and measure the need

- `addr2line -e <packed beamlet ELF> 0x002e2b70`, both widths (the rv32 address differs: take
  it from an rv32 probe run), names the function that stored past the stack.
- On the host, run the same block under a stack-peak probe (the fake kernel's painted stack as
  MEM1's launcher paints, or a guard thread of exactly 18 pages) to get the compiler's deepest
  native recursion in bytes, and which code recurses: the interpreter per Erlang call, the
  loader, or a BIF over the AST.

## Step 2: the lever, in order of preference

(a) If one path recurses natively, make it iterative as the term walks were: no limit moves.

(b) Else the stack is declared from a new peak, by the rule the pages state (testbench.md:1606:
"declarations are twice the largest peak across those runs"): `beamlet-footprint` gains a
compile of the brief's module so the peak it reports includes the compiler;
`SESSION_STACK_PAGES` and `SCHEDULER_STACK_PAGES` follow it (one constant, stated once, as
lib.rs's comment already ties them); the steward's heap-cap arithmetic (11,092 of 11,136 pages:
testbench.md:1636-1640) is recomputed on the page and in the code with the pages that move, and
`sizes` on init.md if the session no longer fits.

Not an option: a separate compile thread with a bigger stack. The compiler is Erlang code on a
scheduler; every scheduler must carry what any process may run.

## Gates

- A machine case `shell-compile-module`: a session over SSH types the brief's `defmodule` at the
  prompt and calls a function of it, both widths, 5 runs in its class.
- `beamlet-footprint` both widths with the new peak quoted; the 14-case shell set; `size-budget`;
  `steward-ssh-two-principals` once (two sessions' stacks beside each other).
- init.md "Stacks" and testbench.md's memory-budget paragraph updated with the peak and the
  arithmetic; beamlet.md where it states the scheduler stack; development.md's compiler status
  line stays host-only until AGT3.

## Files

userland/otp (vm, redoubt/src/lib.rs), servers/steward/src/bin/steward.rs, tests/,
docs/testbench.md, docs/servers/init.md, docs/userland/beamlet.md.

Tier A (a session limit moves, or the VM's interpreter changes): steward-red-2. Size S-M.
