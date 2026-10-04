# Architect handoff (architect-5, 2026-10-02)

Read the earlier handoffs first: architect-handoff.md, architect-2-handoff.md, the third (in
Wash) and architect-4-handoff.md. Their working rules stand:
- pages go on main, staged by path;
- run doccheck before every commit:
  `./.wash/local/in-dev bash -c 'cd /work && cargo run -q -p redoubt-doccheck'`;
- QA bodies and answers to the orchestrator are at most 2000 bytes, so detail goes in a file;
- no thread names or dates on pages; a rule is cited first as "R12 (scheduling)", and a milestone
  is written in full;
- do not edit on main a page that another branch is rewriting. Give that package the exact lines,
  and check them at its review.
- decision_request blocks you, so end your turn after it. An owner's question back ("explain?")
  is not an answer: explain, and ask again on the same thread.
- K16's rule applies to everyone: nothing is written on a page before the code. Rulings that need
  code go into briefs as exact page lines, and the package writes them.

## Rulings of this stretch, and where each lives

| Thread | Ruling | Where |
| --- | --- | --- |
| STEWARD1-toolchain | Accepted: OTP and Elixir built from checksum-verified upstream source into the dev image under /opt/toolchains; the case FAILS, never SKIPs, on a wrong version | Merged (dev image rev 4, 87581a5e2); resolved |
| STEWARD1-rows-unreached | Keep lease.md's `StartAgent !not_locked` row: the core takes `now` from its embedder, and the row is the same guard as the login's keeper. The 42 rows the model traces never take are no follow-up: the families reach them at other seeds (I ran the mutations, all caught). | steward.md paragraph after "Each rule has one keeper", merged with STEWARD1; resolved |
| (INIT2 brief) | INIT_PAGES check: a fixed arena, and a bound on the manifest's cost in `root` compared before anything is created. Bucket counts: every declared (account, label set) at every shared server, plus one root badge per system caller. INIT1-merge reconciliations: listed, INIT2 owns the rest. K16 owns every count. | .wash/local/INIT2-implementer.md (rulings 1-7, exact page lines) |
| INIT2-manifest-rules | Q1: handed items are {endpoint, badge}. Q2: root, system and users are reserved names (R33). Q3: DEV_PUBLIC_KEY moves to libs/signing (K16 agreed). Q4: confinement by kind, in the order endpoint, volume, network, device, server instance, with a server's users counted as for buckets; a device or a receive endpoint held twice is always refused. Q5: defaults, plus three notes. | .wash/local/INIT2-questions-1-ruling.md (page lines); resolved |
| INIT2-confined-cores | **Owner:** R34's core rule is dropped, because the kernel and the cores are shared by every label set. The check is built in INIT2. | TENETS (Confinement, Side channels), init.md (confinement check, R34), SECURITY.md, GLOSSARY, agents.md: 1d109ac5f on main |
| K-destroy-simplify | Assessment for the owner. The destroy path is not as simple as the properties allow, because destruction runs IPC's pump mid-kill and IPC carries five dying/doomed checks. Recommended: one package after K16, "pumps at the boundary" (mark, sever, reap, then pump), called K19 if cut. A suspected order-dependent delivery of a dying-stamped send is to be confirmed by a model trace. | .wash/local/destroy-simplify.md; **awaiting the owner**; no node yet |
| K16-timer-flood-share | Kernel finding: irq.rs bills a timer entry's tail after expiry to the interrupted budget, against timer.md and scheduling.md. The case's floor stands. New node K20 (todo, Tier A, S). | .wash/local/K20-timer-entry-billing.md (brief, exact page lines); node K20 |

## Open, and what waits on what

- **GATE1** is in its sweep, and INIT1 merges behind it.
- **K16** is held at commit 4 (277a2c060). It needs K20 merged, then rebases. It also waits on
  INIT1. I asked the orchestrator to add K20 to K16's needs.
- **K20** is to be launched. Its first evidence is sched-timer-flood's shares on main after the
  fix, and the padding self-check.
- **INIT2** is in deliverable 2, on wp-init2. It must rebase onto main for 1d109ac5f before it
  writes init.md.
- **K19 (pumps at the boundary)** is cut only if the owner agrees. Then write the node and brief
  from destroy-simplify.md. It goes after K16, which rewrites the same walks.
- **Still unruled from earlier:** nothing that I know of. INIT_PAGES and the INIT1 reconciliations
  are now ruled in INIT2's brief.

## What to watch at the merges

- **GATE1:**
  - the notice is met net on both widths with two leases live;
  - the sweep's worst is recorded with the target it sets, and the per-width seed sentence is on
    scheduling.md;
  - kernel/README.md#containment goes to built.
- **INIT1:**
  - boot.md has no program count ("one initial process");
  - budgets.md's "Root, system and users" is built at 15 and 47;
  - devices.md's `device_info` is built.
- **K20:**
  - scheduling.md "Charging" and timer.md "R12 for timer work" carry exactly the brief's lines;
  - the sched-timer-flood figures in Responsiveness are re-measured;
  - no mutation is added (the model charges runtime only), and the report says so.
- **K16:**
  - every process and handle count moves with the code: budgets.md's table and figure,
    `MAX_START_HANDLES` in init.md, boot.md's handoff record;
  - the "(at most 63)" bullet in budgets.md is INIT2's rewording ("system's process limit"), not
    a new number;
  - its walks keep GATE1's targets;
  - destroy-simplify's K19 should rebase on K16's walks.
- **INIT2:**
  - the INIT_PAGES lines on budgets.md and init.md step 1;
  - the bucket "Sizing" lines and the confinement domains paragraph (Q4);
  - the handed {endpoint, badge} row, the reserved names, ssh-ed25519 only;
  - "The tree from the boot manifest" built, with no process count in it;
  - "What init does with the bundle" built;
  - todo/server-bucket-counts.md and its SUMMARY line gone;
  - the six-server case fails if root's usage exceeds the bound;
  - every refusal judged by init's line and the power-off status;
  - restarts stay planned (INIT3).

## Fragile pages

- **steward.md:** only its fixed `##` headings are allowed.
- **scheduling.md:** Responsiveness keeps growing measurement prose. K20 and GATE1 both touch
  it.
- **init.md:** INIT2 (planned sections) and K16 (the startup-block table) both edit it, in
  different sections.
