# SHELL7 design checkpoint

Branch wp-SHELL7 off origin/shell (36d1450f9), worktree .worktrees/SHELL7. No code yet.

## 1. Resource use: what the commands show, and from where

Page (shell.md#resource-use): `top()`, `ps()`, `df()`, `free()`, `uptime()`, reading the budgets
the session holds through `budget_usage`, showing only the caller's own work.

What the session can read today, with no new native and no new call:
- its own budget, the named handle `budget` (`Redoubt.Budget.own/0`), through the existing
  native `budget_usage/1` (`Redoubt.Budget.usage/1`): pages {limit, used}, processes {limit,
  used}, weight {limit, carved}. R1 already limits a user-class read to budgets within its labels.
- the budgets it carved itself (a child handle `Budget.carve/1` returned).
- the VM's own view: `:erlang.memory/0,1` (beamlet has it), `Process.list/0` and
  `Process.info/2` for the session's Erlang processes, `System.monotonic_time/1`.

Proposed commands (one commandlet module, `Redoubt.Shell.Resources`, area "Session"):
- `free()`: the session's pages (limit, used, free) from its budget, and the VM's heap by kind
  (processes, code, binaries, atoms, ets) from `:erlang.memory`, in pages.
- `uptime()`: the kernel's clock since boot on Redoubt (beamlet's monotonic clock is the
  kernel's); on the host, since the VM started, and it says so.
- `ps()`: the session's budget (pages, processes, weight) and each budget it carved and still
  holds; plus the VM's Erlang processes by memory and reductions (the session's own code).
- `top()`: the same, refreshed each second as a screen program (Redoubt.Screen), q quits.
- `df()`: NOT buildable now: no file server answers a statfs or reads back FSD2's quota; it
  needs a server call (Tier A, main). Proposal: leave `df` planned with that named, or its own
  package.

Where the data comes from: the plain host platform answers every `redoubt` native
`{error, not_supported}`, so on `./shell` the commands show the VM's part and say the budgets are
the machine's; on `./shell --fake` the fake kernel's budgets are real. Tests: ExUnit on BEAM and
beamlet for the formatting over a usage map, and the not_supported path; the fake-kernel stage of
./test-shell for one real read.

Gaps: "their processes" in the page means the budget's native processes. The session gets no PID
from `launch` (a job is a ref), and jobs and pipes are planned (shell.md#interrupting-and-killing-jobs),
so no job table exists to list. Proposal: ps/top list the own budget, held carved budgets and the
VM's Erlang processes now; native jobs join with the jobs package; the page says so.

Authority: read-only, the session's own handles, through Redoubt.Budget.usage (already on main).
Nothing minted, carved or passed. But the shell-track rules put "anything acting with the
session's authority: ... budgets" on main's strict path. Question for you: are read-only usage
displays over the existing Budget.usage shell-track (Tier B, one reviewer), or main?

## 2. Footprint

Findings:
- Term's `Enum.chunk_while` (term.ex:237) is the one Enum call in Term that has no list fast path:
  it goes through `Enumerable.reduce`, so Enumerable (and Enumerable.List / .Range) load.
- Unconsolidated protocol dispatch calls `Protocol.__concat__/2` (Elixir protocol.ex:957, 971),
  so ANY unconsolidated dispatch loads Protocol (66 KB .beam): Inspect when the printer inspects
  a result, String.Chars on interpolation. So cutting Term off Enumerable may save only
  Enumerable + Enumerable.Range (~17 KB .beam, perhaps 8-15 pages) at the post-command peak, not
  Protocol, unless nothing else dispatches unconsolidated. To be measured, not assumed.
- Consolidating in the shell's mix.exs does nothing on Redoubt: the pack copies elixir's own
  ebin (tools/testbench/src/userland.exs, `app:elixir`), and beamlet finds system code first
  (the mix.exs comment). Real consolidation is the packer consolidating Elixir's protocols over
  every packed app and staging those in place of elixir's: the bench's bundle builder, a hotspot,
  Tier A ("the packer decides what the userland disk holds"), main.
- Behaviour cost of consolidation: a `defimpl` typed at the prompt for a consolidated protocol is
  ignored (Elixir warns). A REPL loses that. Owner's call.

Plan:
1. Measure first, in a scratch build: Term on a list recursion in place of chunk_while (and find
   the range enumerated at the prompt), then the modules loaded after one command and
   beamlet-footprint's rv64/rv32 peaks.
2. Term change: drawing path, so its own commit for main under the full rules.
3. Consolidation at the pack: main, Tier A, only if you and the owner accept the prompt-defimpl
   loss. Expected: Protocol gone (~25-45 pages), and faster dispatch.
4. Session size: 11,904 needs the rv64 peak <= 5,879 to drop to 11,776 (a cut of >= 25 from
   5,904). Changed only on main, with the measurement.
