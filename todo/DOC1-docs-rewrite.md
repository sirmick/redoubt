# DOC1: the documentation rewrite (brief)

Brief for the DOC1 lead, agreed with the owner on 2026-09-25. It replaces the older
DOC1-docs-freshen.md. The lead turns it into a complete page manifest (plan mode). The owner
reviews that manifest before any page is written.

## Outcome

A complete, clean rewrite of Redoubt's documentation:
- A new reader learns the whole system from it.
- An auditor can check every security claim against code and tests.
- Every page says clearly what is built and tested and what is planned.

Nothing from the old docs survives unless it is rewritten into the new structure.

## What Redoubt is (the frame for every page)

Redoubt is a **prison**: a headless, multi-user OS built so that the most capable and most
malicious agents can run on it, do real work, and not get out.
- It runs on RISC-V softcores, nearly always over virtio (blk, net, console). It never has a
  local display, keyboard or mouse.
- **Auditable by construction.** There is no C and no shell script anywhere, userland included.
  The TCB (firmware interface, loader, kernel) is small enough to read like a textbook.
- **Authority is capabilities.** There is no ambient authority. A child's capabilities are the
  same as or narrower than its launcher's. An agent never inherits network access by default.
- **Network.** Default deny; allowlists written as names; blocklists only subtract from an
  allowlist.
  - DNS is mediated: a resolver answers only names in the caller's allowlist, and connections
    are made by name and pinned.
  - Agents get gateway capabilities (`gatewayd`: TLS, keys, request checks, logging), not
    sockets. Only people get name-scoped TCP.
  - Always forbidden: host and metadata addresses, the box's own services from inside, and
    inbound traffic other than SSH.
  - Future web serving terminates TCP, TLS and HTTP in isolated trusted servers
    (`ipd`/`tlsd`/`httpd`), split by privilege. Apps get parsed requests, and inbound access
    is only through SSH forwarding.
- **Residual risks are stated, never hidden.** The docs name the walls:
  - the approval human;
  - host virtio emulation;
  - allowed egress channels (a model API can carry data out);
  - side channels.

  Claims are "enumerated walls, each attack-tested", never "impossible".
- **Non-goals** (state them in TENETS):
  - no display, keyboard, mouse or GUI;
  - no Unix signals, `fork`, `setuid` or root;
  - no POSIX shell;
  - no symlinks or hard links (namespace binds instead);
  - no swap and no IPv6;
  - no inbound services except SSH (and SFTP/SCP inside it).

## Milestones (always written with their name)

Never write a bare milestone number. Always "M2 (usable shell)", including in status markers.
The same goes for any identifier a reader can't decode: say what it is at first use on a page.

- **M1 (separation and containment):** Alice and Bob log in over SSH into Elixir sessions and are
  kept apart. Alice's agent runs contained under a lease. The M1 attack suite passes.
- **M2 (usable shell):** the Elixir shell is a working environment:
  - a command mode;
  - file operations (copy, move, rename, delete, mkdir) and binds;
  - viewing and searching files;
  - native programs with stdin/stdout and pipes;
  - interrupting and killing jobs (by budget destruction; no signals);
  - line editing, history, completion and help;
  - showing resource use;
  - the editor.
- **M3 (files in and out):** SFTP and SCP inside SSH, confined to the session's capabilities and
  audited.
- **M4 (self-hosted development):** Redoubt is developed on Redoubt.
  - `git` goes through a gateway.
  - Elixir and Erlang compilers run on the box; Rust is built off-box and shipped signed.
  - The server APIs, client crates and Rust `std` target exist.
  - `gatewayd` reaches one model provider, with TLS and a name-allowlisted resolver.
  - The agent harness gives agents capabilities narrowed from their launcher's.
  - The audit log exists, and the escape-room harness (GAME) runs continuously.
- **M5 (persist, install, share):**
  - the steward's state survives reboots;
  - users and keys are managed at run time;
  - signed packages, trust lists and shared projects;
  - A/B updates and rollback;
  - the service supervisor;
  - wall-clock time and time sync;
  - log retention.

**Beyond M5** (not goals; only in `docs/beyond/`):
- the softcore/FPGA platform, SMP and full rv32;
- Python and Java runtimes ported to Rust (slow is accepted; the same audit rules apply);
- the web stack;
- unattended operation (backup, crash records, field updates, monitoring, a rescue console);
- a Rust OS facade, swap-like ideas, and others.

## Layout

```
README.md                  (repo root) short: what Redoubt is; pointer into docs/
GETTING-STARTED.md         (repo root) build, run, test
docs/
  README.md                reading order, the status legend, how to render
  TENETS.md                threat model, guarantees, tenets, non-goals (outranks all)
  SECURITY.md              the audit register (below)
  GLOSSARY.md              principal, session, vault, lease, label, badge ...; the Unix
                           equivalent where one exists; ONE vocabulary used everywhere
  SWARM.md                 how packages are built and reviewed (absorbs .pi/agents)
  PROJECT.md               Wash and orchestrator instructions (moved from the root)
  SUMMARY.md               mdBook table of contents
  kernel/                  boot chain and verified boot, memory layout, handles and
                           objects, IPC, memory, budgets and scheduling, timer, devices
                           and DMA, the syscall/ABI reference, invariants, the model
  servers/                 overview (trust tiers, crash blame, restart), then one page
                           each: init and the manifest, steward, keyd, bootfsd, the file
                           server, blkd, netd, ipd, resolver, gatewayd, sshd (with SFTP),
                           consoled, pkg, supervisor; the wire protocol (9P, typed tables)
  userland/                what a person, agent or developer sees: sessions and
                           namespaces, the Elixir shell, files and binds, native programs
                           and Rust std/client crates, agents/leases/labels, file
                           transfer, the development environment, packages
  plan/                    one page per milestone M1-M5: goal, attack suite, remaining
                           work in order, progress
  todo/                    concrete follow-ups, one per file
  beyond/                  ideas past M5, one per file
```

**Deleted:**
- `.pi/`, once its content is folded into SWARM and PROJECT;
- everything in `docs/legacy/`, after the migration below;
- ANSWERS, QUESTIONS, HISTORY, ARCHITECT-NOTES, the 2026-09-22 archive, STATUS, BUILD-PLAN,
  root ASTRA and DEVICE-GRANTS, all of which are folded in first.

Crate READMEs shrink to one or two lines and a pointer to their page. The Wash QA log is not
documentation: it moves out of `docs/` (e.g. `.wash/QA.md`).

## Page rules

1. **Written as if M1-M5 were complete.** Each section has exactly one status line, one of:
   ```
   Status: built · tested: <test>, <test>
   Status: built · partly tested: <what is missing>
   Status: planned · M2 (usable shell)
   ```
   - `tested:` names existing tests: bench cases, model properties or planted-bug mutations,
     fuzz targets or host tests.
   - A section that is part built and part planned is split.
2. **Written from the code** wherever the code exists: the code and its tests are the source.
   The legacy docs, ANSWERS and the QA log are used only for design intent (planned parts) and
   for residual risks.
3. **Stable rule IDs.** Existing rule and invariant IDs (R1.., I1..) keep their numbers and move
   to the page that owns them.
   - New rules take the next free number, and IDs are never reused ("R7: withdrawn").
   - At a page's first citation of an ID, give its short name: "R11 (no mapping is ever
     replaced)".
4. **Template for kernel and server pages:**
   - **Purpose.**
   - **Interface** (calls or protocol).
   - **Authority:** what it holds, what it can grant, what it must never get.
   - **Security properties,** each with its rule ID and proving test.
   - **Failure and restart,** including crash blame.
   - **Residual risks.**
   - **Why:** short design reasons, inline.

   Userland pages use Purpose / How to use it / What it can and cannot do / Why.
5. **Planned sections** end with an **Open:** list of their undecided questions. These replace
   QUESTIONS.md.
6. **No process material:** no package IDs (WP-…), answer or question numbers, commit hashes,
   Wash/QA thread names or review-round references. Provenance lives in git.
7. **One vocabulary** (GLOSSARY). Say "principal", not "user", where the design means principal.
8. **Diagram-heavy.**
   - Mermaid for flows, sequences and state machines. svgbob (ASCII art to SVG) for memory maps
     and box layouts.
   - Text only, inside the page: no binary images.
   - Solid lines are built, dashed lines are planned. Every diagram has a caption.
   - Required at least:
     - trust boundaries / prison walls (TENETS, SECURITY);
     - capability holdings per server;
     - sequences for boot, IPC call and reply, lending, login, an agent call through
       `gatewayd`;
     - state machines for the process, lease, budget and DMA-run lifecycles;
     - capability delegation trees (launcher, agent, sub-agent);
     - memory layouts and ABI register use;
     - the network path (agent, `gatewayd`, `tlsd`, `ipd`, `netd`).
9. **SECURITY.md,** one row per security property:
   - property;
   - rule ID;
   - where it is enforced (code path);
   - the test that proves it;
   - status (with milestone name);
   - residual risks.

   It is the single place an auditor starts. Every page's properties appear in it and nothing
   else does.
10. **Code comments** cite pages and rule IDs ("kernel/memory.md R11"), never answer numbers,
    package IDs or review threads.

## Rendering and checking

- **mdBook** (Rust) renders `docs/` to HTML with navigation and search, driven by
  `docs/SUMMARY.md`. Mermaid goes through the mdbook-mermaid plugin; svgbob through an svgbob
  plugin. Raw pages must still read well on GitHub, which renders Mermaid natively.
- **The docs checker,** a small Rust tool run as a bench case, fails when:
  - a section has no status line, or has a malformed one;
  - `tested:` names a test that does not exist;
  - a bare M1-M5 appears without its name;
  - a process reference appears (WP-, answer/question numbers, hashes, QA thread names);
  - a rule ID is claimed by two pages, or cited by code or docs but owned by none;
  - a relative link does not resolve;
  - SECURITY.md and the pages disagree about properties;
  - a binary image appears under `docs/`.
- **`mdbook build`** is also a bench case.
- **Tooling preflight:** the orchestrator installs mdbook and the plugins before launch (members
  never install toolchain parts). The checker must itself follow the tenets: Rust, small, no
  unsafe.

## Migration

1. **Freeze.** No package edits docs until the switch-over. R3 and the rest of M1
   (separation and containment) wait for DOC1.
2. **Move.** `git mv` the current `docs/*` to `docs/legacy/` in one commit. Move the Wash QA log
   out of `docs/` and point Wash at its new path.
3. **Inventory.** Before writing, extract from `legacy/`, ANSWERS, QUESTIONS, the archive and the
   QA log:
   - every accepted decision;
   - every residual risk and security caveat;
   - every open question;
   - every rule and invariant ID.

   Each item gets a destination page and section. This is the list the red team signs off
   against.
4. **Skeleton and manifest** (the lead's plan, owner-reviewed): every file and heading with its
   status line and sources; the ID map; the diagram list per page; the style guide; one finished
   sample page (kernel IPC).
5. **Write the sets.**
   - Kernel first; it's mostly built and sets the example.
   - Then servers and userland; these may run in parallel.
   - Then the top level (README, TENETS, SECURITY, GLOSSARY, `plan/`, `todo/`, `beyond/`) and
     process (SWARM and PROJECT, absorbing `.pi/`).
   - Each set is reviewed before the next begins.
6. **Owner review** of the whole, plus the red team's inventory sign-off: every item landed, none
   softened.
7. **Switch over,** in one commit:
   - code comments and test descriptions cite the new pages and IDs;
   - the root README and GETTING-STARTED are rewritten;
   - `.pi/` is deleted;
   - the checker and mdbook cases are enabled.
8. **Deprecate.** Delete `docs/legacy/` (git keeps it). HIST1 comes after this.

## Team and workflow

A small swarm, mostly sequential. One voice matters more than speed.

| Role | Tier | Job |
|---|---|---|
| Lead | frontier (Opus, high) | The manifest and style guide, the sample page, the kernel set, the top level; holds the voice; hands off when full |
| Writer | workhorse (Opus, medium) | Servers, then userland, from the manifest and guide; a fresh writer per set |
| Inventory readers | light (Sonnet, low), 2-3 short-lived | Extract the inventory from ANSWERS, QUESTIONS, the archive and the QA log in parallel (grep and page through the 1.5 MB log; never read it whole) |
| Red team | workhorse | Per set: claims true against the code; every "built" backed by a real test; no inventory item lost or softened |
| Editor | light (Sonnet, low) | Voice and template conformance, vocabulary, links, no process leftovers |
| Architect | frontier | Answers design questions that "written as if complete" forces on planned areas |

**Process rules:** the ones from PROJECT.md and SWARM.md.
- Resident members; review rounds cite findings by reviewer and number.
- Handoff files at about 250-300K context; commit before 50K of uncommitted work.
- A checkpoint after every commit group; never end a turn without a report or a waiting status.
- Stage paths by name; never stash.

Work happens in `.worktrees/doc1` on branch `wp-doc1`. The orchestrator merges.

## What the lead delivers first (plan mode)

The manifest:
- every page, with its path, purpose, sections (each with its status line), sources (code
  paths and tests; legacy sections and inventory items), rule IDs owned and required diagrams;
- the style guide and templates;
- the inventory's structure and how the readers split it;
- the order of work, with parallelism and handoff points;
- the acceptance checks per page and per set;
- the checker's exact rules;
- the switch-over steps.

The owner reviews it. Writing starts only after "DOC1 PLAN APPROVED: write".

## Carry into todo/ (known follow-ups at the time of writing)

- The `sched-latency` steward decision-wake target miss on rv64: re-pin the target or change the
  test. This is an owner decision.
- The R10 (budget destruction) cost near its 30 ms target: fix before the steward work.
- A masked IRQ lost before the first receive (level-latch).
- The loader stub's test-coverage follow-ups.
- The kernel `print!` panic re-entry.
- `process_map` backing its source before refusing bad flags.
- K5's carve-lead rescale follow-up.
- The host-side `ssh-loopback` bench cases fail on this host.
