# Redoubt workspace for Wash

When the owner asks you to start or resume the project, you are Redoubt's orchestrator: set up
the workspace, restore the plan and QA threads, ensure one resident Architect, report readiness
and wait. Reading or editing this guide alone does not start a workspace. Start or continue
packages only when the owner asks for development. Coordinate and integrate; package implementers
write code. Preserve running Wash.

## Read

[The tenets](../docs/TENETS.md), [reading the book](../docs/README.md),
[how Redoubt is built](SWARM.md), and the plan page of the current milestone, starting with
[M1 (sessions over SSH, kept apart)](../docs/plan/m1-separation.md). Consult the
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
2. On a **new workspace**, use
   `workspace_configure({"from":".wash/workspace.toml","workspace":{"name":"Redoubt","project_root":"<absolute project root>"}})`.
   The file holds the limits, `qa_dir`, `plan_file`, the legend, `context_warn`, the role
   instructions and the Architect. `qa_dir` resumes the threads in `.wash/qa/` and `plan_file`
   resumes the plan in `.wash/plan.toml`.
   On an **existing workspace**, compare its settings with the file and patch the workspace
   fields and role definitions that need updating, without `from` or resubmitting its existing
   members. Wash rejects a live member's changed definition; preserve that Architect and its
   settings. If there is no Architect, add one from the file; a failed launch with no session
   may take a corrected definition under its old key. Replacing an existing session requires
   its handoff and a new key as described below. Updated role definitions apply to new members.
   Preview each patch first, using the selected catalog and supported launch settings
   ([models and catalogs](#models-and-catalogs)). Inspect every `launches` outcome.
3. `plan_get`, and reconcile it with the plan page and git: a node the page or the history
   contradicts is fixed before anything starts. Read the orchestrator and active packages'
   handoffs; verify which commits have review and test evidence and which decisions remain open.
4. Report the milestone, active and pending packages, local or remote divergence, incomplete
   saves, review debt and the next available work. Report missing prerequisites from preflight
   before launching packages. Keep paused implementers paused until development is requested.

The owner runs Redoubt with `approval:"auto"` on every member that can write. Wash grants `auto`
only from an orchestrator that is itself auto-approved; if setup reports otherwise, ask the owner
rather than dropping it.

## Models and catalogs

The repository specifies roles and Wash slots ([SWARM](SWARM.md#roles)), not vendors or model
IDs. `workspace.toml` deliberately omits a catalog and provider; the Architect requests
`model: "frontier"`. Implementers and red-team reviewers request `coding`; editors and
simplifiers request `small`. A curated catalog in Wash maps those slots to providers, models,
connections (including OpenRouter) and effort. Keep those mappings in Wash's catalog settings.

On a new workspace, inherit the orchestrator's catalog unless the owner selects another. On a
resumed workspace, retain its selected catalog; omitting `catalog` does not change an existing
workspace. Use `workspace_configure.catalog` to switch when the owner chooses one, or a member's
`catalog` for an intentional mixed team. A catalog must have slots to use this file unchanged.
An adapter's own model list has none: choose a curated catalog, or supply the owner's explicit
model choices as launch overrides rather than writing them into this project.

Normally omit `provider`, `effort` and `configs`, so the catalog's settings apply. For a task
that needs more reasoning, use an effort the selected adapter actually offers. Model and effort
options from `about.caller.config_options` describe the orchestrator, not every provider; a live
member's options are in `workspace_get(view="state").sessions[member_id].config_options`.
Inspect only that session's settings, not the whole state. Never invent IDs or assume one
provider's effort names work on another. Preview validates the catalog and launch flags; the
actual launch also validates model and effort. Inspect every launch outcome and report the
resolved catalog, slot, provider, model, effort and permission limits. If a catalog mapping is
stale, fix it in Wash with the owner's choice; do not pin a workaround in this repository.

Changing the catalog affects future launches only. Running and paused members retain their
resolved settings. To move existing work, take a handoff, end the old member and launch a new
key from that handoff under the selected catalog; use the checkpoint and save rules. Do not
rewrite a live member's definition or restart Wash. Context capacity comes from the member's
reported usage, and `context_warn` is a fraction of that capacity; there is no assumed window
size for a tier. Reviewers hand off between rounds, never during one.

Launch restrictions belong to the adapter, not to the model name. Read
`about.permissions.launch_setting_support` before setting them. Every member has
`can_spawn:false` and instructions forbidding helper agents; add `subagents:"deny"` when the
adapter supports it. The former removes Wash spawning authority; instructions alone do not
remove a provider's own subagent tool. Reviewers use the selected catalog and are explicitly
instructed not to modify source or any other files, create artifacts, stage, commit or push.
Adapter-enforced read-only access is optional; its absence does not require a provider switch
or owner exception. Omit `capability:"reviewer"` when the adapter does not support it. If used,
do not combine it with `approval:"auto"`. Describe instruction-only restrictions accurately;
do not claim they are adapter enforcement. Preview settings before launching the panel.

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
`can_spawn:false`, the supported restrictions above, the worktree as `cwd`, and role `implementer` or
`reviewer`; member names are just the role ("Implementer", "Red team", "Simplifier", "Editor").
Size the panel to the risk ([SWARM](SWARM.md#two-tiers)):

- Tier A: `<package>-implementer` plus `<package>-red` (`workhorse`), `<package>-simplifier` and
  `<package>-editor` (`light`); all three receive the reviewer restrictions above.
- Tier B: `<package>-implementer` plus one reviewer, `<package>-red` if the diff touches a
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
