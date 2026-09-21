// Redoubt swarm: run one work package end to end.
//
// Args (plain JSON):
//   package           string, required. The WP id, e.g. "WP-K4".
//   branch            string. Defaults to "wp-" + lowercase package.
//   reads             string[] | string. The design notes the package reads.
//   delivers          string[] | string. The paths the package owns (staging boundary).
//   acceptanceCommand string. Defaults to "cargo testbench". Run as the implementer gate.
//   designQuestion    string. Optional. If present, the architect resolves it first.
//   notes             string. Optional extra instruction for the implementer.
//
// The orchestrator must launch this in the package's own worktree (cwd = that worktree) so
// the implementer and the reviewers share one checkout and the reviewers can see the diff.
// Worktree allocation is the orchestrator's job, not this script's: giving every child its
// own worktree would hide the writer's changes from the read-only reviewers.

const pkg = String(args.package || "");
if (!pkg) {
  return { ok: false, error: "run-package: args.package is required" };
}

const acceptance = String(args.acceptanceCommand || "cargo testbench");
const branch = String(args.branch || ("wp-" + pkg.toLowerCase()));
const reads = Array.isArray(args.reads) ? args.reads.join("\n- ") : String(args.reads || "(not given)");
const owns = Array.isArray(args.delivers) ? args.delivers.join("\n- ") : String(args.delivers || "(not given)");

const header = [
  "Work package: " + pkg,
  "Git branch: " + branch,
  "Design notes to read first:\n- " + reads,
  "Paths this package owns (stage only these; never `git add -A`):\n- " + owns,
  "Acceptance command: " + acceptance,
  String(args.notes || "")
].join("\n\n");

// Stage 0 (optional): resolve an open design question before any code is written.
let designReceipt = null;
let designBlock = "";
if (args.designQuestion) {
  designReceipt = await runs.run("architect", {
    agent: "architect",
    context: "fresh",
    label: "Resolve design question for " + pkg,
    task: header +
      "\n\n## Open design question\n" + String(args.designQuestion) +
      "\n\nFollow your formal protocol: decide whether this is settled, already answered, " +
      "open and answerable, or a genuine owner decision. Record an answer only through " +
      "QUESTIONS.md and ANSWERS.md. Report the question number(s), the note that now owns the " +
      "decision, and whether it is applied or still needs the owner."
  });
  designBlock = "\n\n## Architect finding\n" + designReceipt.output;
}

// Stage 1: implement, gated on the package's acceptance command run in this worktree.
const impl = await runs.run("implement", {
  agent: "implementer",
  context: "fresh",
  label: "Implement " + pkg,
  gate: acceptance,
  task: header + designBlock +
    "\n\n## Your job\n" +
    "Implement exactly this work package per docs/BUILD-PLAN.md and docs/SWARM.md. " +
    "Read the design notes named above first. Land the package's acceptance tests and any " +
    "attack case it requires, with the verdict coming from the system, never the attacker. " +
    "Stage only the owned paths; do not commit, so the reviewers see the diff. If you hit a " +
    "decision the frozen design does not settle, stop and report it as a blocking design " +
    "question; do not guess."
});

if (!impl.ok) {
  return {
    ok: false,
    package: pkg,
    branch: branch,
    stage: "implementation",
    reason: impl.error || "implementer gate failed",
    architect: designReceipt ? { ok: designReceipt.ok, runId: designReceipt.runId, output: designReceipt.output } : null,
    implement: { ok: impl.ok, runId: impl.runId, output: impl.output, outputReference: impl.outputReference }
  };
}

// Stage 2: SWARM.md's three reviewers, read-only, fresh context, over the same checkout.
const common = header +
  "\n\n## Implementation report\n" + impl.output +
  "\n\n## The diff\nUse the `watchdog_diff` tool to inspect the staged and unstaged working-tree " +
  "delta against your launch HEAD, plus the untracked-path inventory. Do not edit anything. " +
  "Filter findings by evidence, not severity: report " +
  "only concrete current issues caused or made reachable by this diff, each supported by " +
  "source proof, a test or repro, or a contract contradiction. Label P0/P1/P2 and end with " +
  "`Merge verdict: BLOCK`, `Merge verdict: OK`, or `Merge verdict: OK with notes`. Say " +
  "exactly `No issues found.` when nothing qualifies.";

const reviews = await runs.all([
  {
    key: "redteam",
    agent: "reviewer",
    context: "fresh",
    label: "Red team " + pkg,
    task: common +
      "\n\n## Your angle: red team (attack the spec, not just the code)\n" +
      "Attack the package against docs/KERNEL-SPEC.md, docs/TENETS.md and PLAN.md's attack " +
      "suite. Hunt for the security property this package claims and the input that breaks " +
      "it: a hostile syscall argument, a malformed message, a resource-exhaustion case, a " +
      "label or capability bypass, a violation of a rule R1-R12 or invariant I1-I15. " +
      "Confirm every attack case's verdict is taken from the system (kernel, victim, or " +
      "clean power-off), never from the attacker's output. Name the exact missing or " +
      "vacuous test."
  },
  {
    key: "simplifier",
    agent: "reviewer",
    context: "fresh",
    label: "Simplify " + pkg,
    task: common +
      "\n\n## Your angle: simplifier (what can be deleted)\n" +
      "Find code to delete, not to add. Check the package against TENETS.md 1 (Simple " +
      "enough to audit in full). Flag unnecessary abstraction, speculative scaffolding, " +
      "duplicated structure, configuration nobody uses, and any growth of the trusted " +
      "computing base that the spec did not require. Prefer deleting code to adding " +
      "configuration. Quote the exact lines that can go."
  },
  {
    key: "editor",
    agent: "reviewer",
    context: "fresh",
    label: "Edit " + pkg,
    task: common +
      "\n\n## Your angle: editor (code, comments and notes agree)\n" +
      "Check that the implementation matches the design notes it cites, that comments and " +
      "SAFETY justifications match what the code does, that tests/paths/names are spelled " +
      "the same everywhere, that docs/FORMATTING.md is met, and that every `unsafe` block " +
      "has a true SAFETY justification. Report any place a note and the code disagree, and " +
      "say which one should change."
  }
]);

const reviewRows = [];
let i = 0;
while (i < reviews.length) {
  const r = reviews[i];
  reviewRows.push({
    key: r.key,
    ok: r.ok,
    runId: r.runId,
    outputReference: r.outputReference,
    error: r.error || null,
    output: r.output
  });
  i = i + 1;
}

return {
  ok: true,
  package: pkg,
  branch: branch,
  acceptance: acceptance,
  architect: designReceipt
    ? { ok: designReceipt.ok, runId: designReceipt.runId, output: designReceipt.output }
    : null,
  implement: {
    ok: impl.ok,
    runId: impl.runId,
    outputReference: impl.outputReference,
    output: impl.output
  },
  reviews: reviewRows,
  next: "Orchestrator: fix or record findings, re-run the acceptance command and the whole bench, " +
    "then rebase on redoubt and merge one package at a time (SWARM.md rules 4 and 5)."
};