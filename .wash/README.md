# How the work on Redoubt is run

This directory is the project's process: how Redoubt is built, by whom, and the record of it.
[The book](../docs/README.md) describes the system; nothing here restates it.

| File | What it is | Who writes it |
| --- | --- | --- |
| [SWARM.md](SWARM.md) | the process: roles, packages, reviews, questions, acceptance, commits | the owner |
| [PROJECT.md](PROJECT.md) | the orchestrator's setup, preflight and recovery | the owner |
| `workspace.toml` | the workspace definition Wash loads | the owner |
| `plan.toml` | the plan graph: packages, their order and state | Wash only |
| `qa/<thread>.md` | one file per question: the discussion behind each decision | Wash only |
| `local/` | handoffs, plans, scratch; never committed | members |

## Where to start

- **Orchestrator:** [PROJECT.md](PROJECT.md), which says what to read and how to set up.
- **Implementer:** [SWARM.md, the implementer](SWARM.md#the-implementer) and
  [staging, commits and handoffs](SWARM.md#staging-commits-and-handoffs), then your reading list.
- **Reviewer:** [SWARM.md, reviewers](SWARM.md#reviewers), then the diff you were given.
- **Architect:** [SWARM.md, the Architect](SWARM.md#the-architect) and
  [questions and decisions](SWARM.md#questions-and-decisions).
- **Anyone asking what is under way:** `plan_get` in a running workspace, or `plan.toml`.
- **Anyone asking why a rule is what it is:** the rule's page first; the thread named in the
  `QA:` trailer of the merge commit that brought it, second.

Never edit `plan.toml` or `qa/` by hand, and never read `qa/` whole: read one thread.
