# DOC1 servers writer handoff

Assignment b6643053 (write docs/servers/, 17 pages, in manifest C4's order). Worktree
`.worktrees/doc1-servers`, branch `wp-doc1-servers`. Read the assignment's reading list first
(DOC1-HANDOFF.md Traps; manifest S, A, B, C4, D3, E, J-C10; kernel/ipc.md and devices.md as the
voice), then this file.

## Done
- `115650df4` `servers/README.md` (reference page: no IDs, headings Labels and Restarts and crash
  blame as ipc.md expects) and `servers/serving.md` (R25 the label check, R26 admission
  fairness, R27 badge allocation, R28 parked-call accounting; recorded in manifest B3 under
  "Servers list (as written)"). New todo pages `account0-share-chain.md`,
  `host-tests-in-bench.md`, both added to SUMMARY's Follow-ups (C12 needs every page linked).
- Code finding reported: `libs/rt/src/server/minted.rs:163-171` (`Minted::share`), the account-0
  self-minted chain opens a bucket per link (inventory A-34); residual on serving.md, todo written.

## Next (in order)
wire, init, steward, keyd, bootfsd, fsd, blkd, netd, ipd, resolver, gatewayd, sshd, consoled,
pkg, supervisor. Next free ID: **R29**. Commit every 2-3 pages; progress message after each.
- `wire.md` includes `libs/wire/tables/ninep_common.md` (line before it links the same file);
  every other table is included by its server page: `startup` (init), `keyd`, `bootfs`
  (bootfsd), `fsd`, `blkd`, `netif` (netd), `net_ctl` and `ipd` (ipd), `consol` (consoled).
- `init.md` must have a heading `Restarts and reboots` (kernel ipc.md links it; README.md too).
- Pages already linking forward anchors: README links `serving.md#the-9p-server-skeleton`,
  `serving.md#minted-connections`, `init.md#restarts-and-reboots`; kernel pages link
  `servers/steward.md`, `servers/netd.md`, `servers/init.md`.
- serving.md cites `host:redoubt-keyd::serving_grant_rolls_back_discard_missing_capability_and_error`
  (servers/keyd/tests/outcomes.rs); keyd.md can reuse it.
- Test gap noted on serving.md (not a code finding): the skeleton's `new_connection` rollback on
  a discarded reply is not attacked through `serve_with` (ninep.rs:533-539).

## Useful facts
- Crate names: `redoubt-rt`, `redoubt-wire`, `redoubt-wire-gen`, `stub`, `redoubt-keyd`,
  `redoubt-bootfsd`, `redoubt-consoled`, `redoubt-blkd`, `redoubt-netd`, `redoubt-ipd`,
  `redoubt-net-tests`, `littlefs`, `redoubt-model`. Fuzz targets: rt/{ninep_server,startup},
  wire/{json,ninep,typed}, blkd/{device,gpt,request}, ipd/{args,frames,session},
  netd/{device,request}, stub/plan, littlefs/{image,mutate}.
- Bench host-test cases: `r4-host-tests` (bootfsd, consoled), `wire-host-tests`,
  `blkd-host-tests`, `netd-host-tests`, `ipd-host-tests`, `d3-net-host-tests`. None runs
  `redoubt-rt`, `stub`, `littlefs`, `redoubt-keyd` (todo/host-tests-in-bench.md; link it from
  init, keyd, fsd).
- The steward is modelled: `model/src/{steward,policy}.rs`, `model/tests/policy_current.rs`,
  `host:redoubt-model::steward_policy`, `steward_noninterference`, and `Policy*` mutations.
- The net rig (`tests/net/src/rig.rs`) stands in for `init`: it launches real `netd`/`ipd`
  through the stub; its placement constants are `IMAGE_AT`, `STARTUP_AT`, `STACK_TOP`.

## Checks before each commit
`cargo run -q -p redoubt-doccheck -- --pages docs/servers docs/todo docs/SUMMARY.md 2>&1 | grep -v
'link \`\(\.\./\)*\(servers\|userland\|beyond\|TENETS\)' | grep -v 'link \`\`\|\`\` is linked' |
grep -v 'C6: link \`[a-z]*\.md\(#[a-z-]*\)\?\`: no such file'` (empty = only forward links left),
then `mdbook build docs` (only the mermaid version warning).

## Traps
- A section with its own status cannot have a subsection with its own status (double status);
  a `##` with only subsections must carry no text of its own.
- Milestone names must not wrap across lines (C3), nor must `R10 (destruction)`-style short names.
- A planned claim under a built status is a finding: move it to a planned section.
- Avoid "today", "yet", "now". Stage by path; never stash; never push.
