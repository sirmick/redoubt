// Redoubt swarm: run SWARM.md's three reviewers over a finished package, read-only.
//
// Args (plain JSON):
//   package  string, required. The WP id, e.g. "WP-K4".
//   branch   string. Defaults to "wp-" + lowercase package.
//   focus    string. Optional. A specific seam or finding to concentrate on.
//   baseRef  string. Optional. The ref the diff is taken against; defaults to HEAD.
//
// Launch with cwd set to the package's worktree so the reviewers see the code under review.
// Reviewers are read-only and fresh-context; they must not edit. Use this after a fix pass
// or to review a package that was built outside the swarm, then have the orchestrator
// synthesize and merge.

const pkg = String(args.package || "");
if (!pkg) {
  return { ok: false, error: "review-package: args.package is required" };
}

const branch = String(args.branch || ("wp-" + pkg.toLowerCase()));
const baseRef = String(args.baseRef || "HEAD");
const focus = args.focus ? String(args.focus) : "(none specified)";

const header = [
  "Work package: " + pkg,
  "Git branch: " + branch,
  "Diff base ref: " + baseRef,
  "Focus: " + focus
].join("\n");

const common = header +
  "\n\n## The diff\nInspect the current working-tree diff and the branch against " + baseRef +
  " (`git diff`, staged and unstaged). Read docs/SWARM.md, docs/TENETS.md, the package's " +
  "entry in docs/BUILD-PLAN.md, and the design notes it names. Do not edit anything, do not " +
  "commit, and do not run `git add`/`git commit`. Filter findings by evidence, not severity: " +
  "report only concrete current issues caused or made reachable by this diff, each supported " +
  "by source proof, a test or repro, or a contract contradiction. Label P0/P1/P2 and end with " +
  "`Merge verdict: BLOCK`, `Merge verdict: OK`, or `Merge verdict: OK with notes`. Say " +
  "exactly `No issues found.` when nothing qualifies.";

const reviews = await runs.all([
  {
    key: "redteam",
    agent: "reviewer",
    context: "fresh",
    label: "Red team " + pkg,
    task: common +
      "\n\n## Your angle: red team\n" +
      "Attack the package against docs/KERNEL-SPEC.md, docs/TENETS.md and PLAN.md's attack " +
      "suite. Hunt for the security property this package claims and the input that breaks " +
      "it: a hostile syscall argument, a malformed message, a resource-exhaustion case, a " +
      "label or capability bypass, a rule R1-R12 or invariant I1-I15 violation. Verify every " +
      "attack case takes its verdict from the system (kernel, victim, or clean power-off), " +
      "never from the attacker's output. Name the exact missing or vacuous test."
  },
  {
    key: "simplifier",
    agent: "reviewer",
    context: "fresh",
    label: "Simplify " + pkg,
    task: common +
      "\n\n## Your angle: simplifier\n" +
      "Find code to delete. Check the package against TENETS.md 1. Flag unnecessary " +
      "abstraction, speculative scaffolding, duplicated structure, unused configuration, and " +
      "any TCB growth the spec did not require. Prefer deleting code to adding configuration. " +
      "Quote the exact lines that can go."
  },
  {
    key: "editor",
    agent: "reviewer",
    context: "fresh",
    label: "Edit " + pkg,
    task: common +
      "\n\n## Your angle: editor\n" +
      "Check that code, comments and design notes agree. Verify every `unsafe` block has a " +
      "true SAFETY justification, that names and paths are spelled the same everywhere, that " +
      "docs/FORMATTING.md is met, and that no comment claims something the code does not do. " +
      "Report each place a note and the code disagree and say which one should change."
  }
]);

return {
  ok: true,
  package: pkg,
  branch: branch,
  baseRef: baseRef,
  reviews: reviews.map(function (r) {
    return {
      key: r.key,
      ok: r.ok,
      runId: r.runId,
      outputReference: r.outputReference,
      error: r.error || null,
      output: r.output
    };
  }),
  next: "Orchestrator: synthesize the findings, fix or record the P0/P1 items, then re-run the " +
    "whole bench before merging (SWARM.md rule 4)."
};