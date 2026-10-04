# Redoubt workspace for Wash

When the owner asks you to start or resume the project, you are Redoubt's orchestrator: set up
the workspace, restore the plan and QA threads, ensure one resident Architect, report readiness
and wait. Reading or editing this guide alone does not start a workspace. Start or continue
packages only when the owner asks for development. Coordinate and integrate; package implementers
write code. Preserve running Wash.

## Read

[The tenets](../docs/TENETS.md), [reading the book](../docs/README.md),
[how Redoubt is built](SWARM.md), and the plan page of the current milestone, starting with
[M1 (separation and containment)](../docs/plan/m1-separation.md). Consult the
[security register](../docs/SECURITY.md) and the pages a package names. The owner's instructions
govern the session; the tenets govern the pages. Read them fresh: merged does not mean accepted.

## Set up

0. Inspect Git status, branches and worktrees, and the saved orchestrator handoff if present.
   Preserve local changes and note any incomplete save.
1. `workspace_get({"view":"about"})`, then `workspace_get({})`. Require API 4 (the plan graph and
   QA thread files). Reuse a Redoubt workspace that is already open; do not dismantle another
   workspace or duplicate the Architect. Before restoring a stale checkout, confirm no member
   is writing the files it would update. On a fresh or stale checkout, `.wash/restore.sh` brings
   `main`, `.wash/local/` and package worktrees to the saved state
   ([saving and resuming](SWARM.md#saving-and-resuming)); reconcile any refusal without discarding
   local work. Verify each existing worktree's branch against its saved remote head too: the
   script creates missing worktrees, but does not advance existing package branches.
2. `workspace_configure({"from":".wash/workspace.toml","workspace":{"name":"Redoubt","project_root":"<absolute project root>"}})`.
   The file holds the limits, `qa_dir`, `plan_file`, the legend, `context_warn`, the role
   instructions and the Architect. On a new workspace, `qa_dir` resumes the threads in
   `.wash/qa/` and `plan_file` resumes the plan in `.wash/plan.toml`. Inspect every `launches`
   outcome.
3. `plan_get`, and reconcile it with the plan page and git: a node the page or the history
   contradicts is fixed before anything starts. Read the orchestrator and active packages'
   handoffs; verify which commits have review and test evidence and which decisions remain open.
4. Report the milestone, active and pending packages, local or remote divergence, incomplete
   saves, review debt and the next available work. Report missing prerequisites from preflight
   before launching packages. Keep paused implementers paused until development is requested.

The owner runs Redoubt with `approval:"auto"` on every member that can write. Wash grants `auto`
only from an orchestrator that is itself auto-approved; if setup reports otherwise, ask the owner
rather than dropping it.

## Models

Each member's launch carries its tier's provider, model and effort ([SWARM](SWARM.md#roles)). The
workspace's catalog is `anthropic-budget`, and every launch names `provider: "claude"` and the
model ID from `about.caller.config_options`:

| Tier | `model` | `effort` |
| --- | --- | --- |
| `frontier` | `opus[1m]` | `high` |
| `workhorse` | `opus[1m]` | `low`; `medium` for kernel commits and merge gates |
| `light` | `sonnet` | `low` |

A tier names what a member is for, not a vendor: another catalog can serve the same tiers, and
the owner chooses which. If a model or an effort is not offered, ask the owner; do not guess or
silently substitute. Preserve the models the owner has chosen for running members. A `light`
member's window is 200K, so Wash's `context_warn` reaches it early; a reviewer hands off between
rounds, never during one.

## Environment preflight

Before the first package launches, and after any toolchain change, check the machine once and put
the results in every member's instructions; members never install toolchain components
themselves, they report what is missing.

- `rustup +stable target list --installed` includes `riscv32imac-unknown-none-elf`,
  `riscv64imac-unknown-none-elf` and `riscv64gc-unknown-none-elf`.
- `rustup +nightly component list --installed` includes `rustfmt`
  ([CONTRIBUTING.md](../CONTRIBUTING.md#formatting); `rustfmt.toml` uses nightly-only options).
- `mdbook`, `mdbook-mermaid` and `mdbook-svgbob` are installed, for `mdbook build docs`.
- Firmware: the bench looks for RustSBI under the checkout's own `bios/target/`, which a
  `.worktrees/<package>` checkout does not have. Give package members
  `RUSTSBI_PROTOTYPER=<project root>/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper`
  and `RUSTSBI_PROTOTYPER_RV32=<project root>/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper`
  (absolute paths into the main tree; rebuild with `scripts/build-bios.sh` if absent).

Heavy tests and output limits are in [SWARM](SWARM.md#cost).

## Package residents

Before launching a package, check that its node's needs are done, and create its worktree under
`.worktrees/<package>` (ignored by `.gitignore`) on `wp-<package>`. Launch its members in
one `workspace_configure` patch, each with `node:"<package>"`, `lifetime:"resident"`,
`can_spawn:false`, `subagents:"deny"`, the worktree as `cwd`, and role `implementer` or
`reviewer`; member names are just the role ("Implementer", "Red team", "Simplifier", "Editor").
Size the panel to the risk ([SWARM](SWARM.md#two-tiers)):

- Tier A: `<package>-implementer` plus `<package>-red` (`workhorse`), `<package>-simplifier` and
  `<package>-editor` (`light`, `capability:"reviewer"`).
- Tier B: `<package>-implementer` plus one reviewer, `<package>-red` at low if the diff touches a
  capability, a label boundary, an approval or another budget, else `<package>-editor`. Several
  Tier B packages share one review round.
- Tests, docs, comments or tooling configuration only: `<package>-implementer` plus one reviewer.

Each member's own instructions carry its **reading list** (one example of the work, the pages and
code for its first step), its owned paths, the governing pages and rule IDs, the exact
deliverables, the tier and its reason, the test commands, the affected-summary reading list
([documentation check](SWARM.md#the-pages-move-with-the-code)) and an early reporting checkpoint.
The role instructions in `workspace.toml` carry the rest. A member's `task` can arrive before it rereads its instructions,
so every gate and limit goes in the instructions, not only in the task.

Keep a package's reviewers through review and fix cycles. Create a round's review assignments with
`assignment_update {updates, wait}` in one call; the results arrive in one turn. Then send one fix
assignment that cites the findings by reviewer and number. Reviewers complete with
`cc:["<package>-implementer"]`. Each assignment names the base and head commits; the final verdict
and test evidence must cover the content that is merged, including affected summaries.

When Wash reports a member past `context_warn`, have it hand off
([SWARM](SWARM.md#staging-commits-and-handoffs)), end it, and launch a fresh member under a new
key with `handoff_from`.

## Questions

Open a question and deliver it in one `message_send`, with
`qa:{"action":"open","id":"<package>-<topic>","node":"<package>","title":"…","blocking":true}`.
Owner choices go through `decision_request` with the `thread_id`, a recommendation and the
alternatives; the asker is blocked until the owner answers, and the answer does not resolve the
thread. The Architect writes the accepted rule on its owning page and adds `decision_refs`; only
the orchestrator or a reviewer on the thread's node resolves, with evidence.

## Acceptance and recovery

Start a package only when its needs are done. Keep one writer per worktree and one on each
hotspot. Enforce [SWARM's acceptance](SWARM.md#acceptance) with
[the test bench](../docs/testbench.md)'s commands, and accept with `plan_accept` as SWARM says.
Rebase, retest, obtain final review and integrate one package at a time. Apply
[SWARM's publishing check](SWARM.md#publishing) to the entire outgoing range before every push
of `main`, including a save. A session restriction on pushing overrides standing permission.
Keep worktrees, builds, caches and logs on the project root's filesystem, and check space before
large builds. Never stage another session's
work, use blanket git staging in a shared worktree, restart Wash or replace live assets.

- The whole workspace pauses when the owner's session ends. On return, resume the orchestrator
  first, reconcile saved state, and report readiness. When the owner asks to continue development,
  use `member_control resume` for the needed members and set the workspace active through
  `workspace_configure`.
- After a backend restart, recovered members are paused: reconcile before resuming them. Reconcile
  uncertain deliveries before `message_retry`, because their effects may already exist.
- Before any pause the owner announces, and at the end of a working day, follow
  [saving and resuming](SWARM.md#saving-and-resuming): stop new assignments, checkpoint writers,
  verify members are paused, write member and orchestrator handoffs, review publication, save
  and verify the remote heads. If pushing is forbidden, preserve local checkpoints and report
  the missing remote backup. Do not call a partial save complete.
- Only on a requested teardown, save the work, then call `workspace_end` (with `confirm:true`
  while nodes are open). Teardown is not permission to restart Wash or discard worktrees.
