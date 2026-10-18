# ATK12 report: walfsd-pools, session-hostile-launch, vault-no-leak

Branch `wp-ATK12` in `.worktrees/ATK12`, rebased on origin/main after train 68 (4096f3861).
Head: see the final section. No server, kernel or lib code; three cases, seven pages.

## Delivered

- `tests/walfsd-pools.toml` (R48, R26). Bob fills his 8 MiB home on the `data` volume
  (`{:bob_full, {8000, {:error, :enospc}}}`: 8,000 KiB in, walfsd's refusal), opens handles on
  `/home/bob/hello` until walfsd refuses by name (`{:flood, {15, {:error, :emfile}}}`: 15 admitted,
  his bucket's share of the server's 32 files with two badges in it, the 16th `too many open
  files`), holds them 30 s, and a create on his full home is `enospc` again. Alice saves 64 KiB
  files to her home while he fills, after, while he holds the handles and after: each
  `{{65536, 65536}, ms}` (size stat'd, bytes read back, elapsed ms), then lists her home. Verdicts:
  alice's own values; walfsd's refusals of bob as progress; the bench's memory scan with
  `heap_fits = true`: `heap walfsd:data 37 of 64 pages` on both widths with the flood held.
  Timed claim: each save under 5 s, twice the worst measured once per width (1,735 ms rv64,
  2,408 ms rv32, both while bob filled, beside other work) rounded up to a second; `within = 20`
  is the ssh margin, stated in the toml as no part of the claim.
- `tests/session-hostile-launch.toml` (R32, R31). Alice's session makes four images in her home
  from `/boot/beamlet-hello` (recipe `tests/data/steward/vault-launch.boot.toml`, the one with
  beamlet-hello public): 40 bytes, half, zeros, empty; launches each through `:redoubt.launch`
  in a 256-page budget carved from her own. The stub refuses the three malformed ones: child
  `{:exited, 111}`, budget `pages: {256, 0}, processes: {1, 0}` read as it ended. The empty image
  and a startup block with handle name `"Not A Name"` are `{:error, :bad_name}`, the native's
  name for every client refusal (`userland/otp/redoubt/src/system.rs`, `Error::Refused(_) =>
  "bad_name"`), the budget untouched. Her processes in use equal before and after
  (`{:after, true}`); `{:alive, ms}` under 1 s (measured 0 ms rv64, 6 ms rv32); a well-formed
  `exec` prints and exits 3. Verdicts: the victim session's own answers. The half image exits
  111 (the stub refuses a segment reaching past the image before mapping) rather than faulting:
  the fault arm of R32 stays stub-launch's (`segment bytes past the mapped image`), which
  needs headers that lie within a whole image; a session cannot be made to fault a child here
  without a crafted header per width, left out.
- `tests/vault-no-leak.toml` (R37, R36, R25). The observable list as confirmed by the Architect
  (thread ATK12-observables, with amendments A and B applied): see the m3-agents.md row and the
  R37 paragraph. Alice's unlabelled SSH session: `{:vault_probe, [:enoent, :enoent, :enoent,
  :enoent]}` (write, mkdir, rm, open-for-write in /vault: no binding), `{:b_budget, {11392, 10},
  2, :ok}` (limits, processes used, the 2048-page probe carved and destroyed), `{:b_contexts,
  [{"", :attached}]}`, `{:b_home, [{"hello", :regular, 13, {{1970, 1, 1}, {0, 0, 0}}}]}`, an exec
  of beamlet-hello `{:exited, 3}` with `pages: {256, N}`, its own push submit `{:req1, {:ok,
  %{request: R1}}}` with its one `[an approval is waiting]`; then the vault session (pty) stages
  `/vault/notes/{memo,second}` and `/vault/secret`, execs beamlet-hello, submits `notes/memo`
  for declassification (its own notice arrives); then the unlabelled session: `{:quiet, 1}`
  matched from the prompt with no `[` outside an escape sequence (no notice reached it),
  `{:same, true}` (the whole record equal), `{:a_budget, {11392, 10}, 2, :ok}`, `{:a_home, ...}`
  equal, the probe again all `enoent`, a second exec, `{:req2, ...}` with its notice, and
  `{:ids, R1, R2, false}`: the second id no successor of the first by one or two. Console:
  `distinct` holds over the three `Submitted { request: N }` records (all three in `1001/{7}`:
  a push into the vault is audited in the request's label set, where it lands; the vault's
  audit seeing an unlabelled action is an upward flow, not an unlabelled observation). Run 6
  (sessions passed, my console pattern expected `1001/{}` for the pushes): rv64 ids
  18148661639220265039 / 17734113734757167670 (the vault's 16446473173857708587); rv32
  10480951847233969112 / 10459980508120921103 (the vault's 14001476313148355998); `{:same,
  true}` and `{:ids, .., .., false}` on both.

## Pages

- `docs/plan/m3-agents.md`: the three rows (54, 59, 60) carry their cases.
- `docs/servers/littlefsd.md` R48 (4), `docs/servers/walfsd.md` Quotas (11), `docs/servers/serving.md`
  R26 (19, summary text: three boots flood a real walfsd), `docs/servers/init.md` R31 (9), R32
  (11, a paragraph on the session launch), `docs/servers/steward.md` R36 (a sentence), R37 (status
  and the observable paragraph, naming what a session cannot reach).
- `docs/SECURITY.md` rows R31, R32, R26, R36, R37, R48: the benches added in page order; after
  the rebase onto train 68, R26/R36/R37 rows set from the merged page lists (main's first).
- Summaries checked, no change needed: README.md (M3 line is milestone status), GETTING-STARTED.md,
  docs/README.md, docs/plan/m3-agents.md Progress and Remaining work (speak of "every not yet",
  still true for the harness rows), image/README.md: none names these rows.

## Gates (exit codes from target/jobs logs; all through jobs.mk)

- formatting PASS, docs PASS (before and after the train-68 rebase), size-budget PASS,
  unsafe-budget PASS (run 5/6).
- session-hostile-launch: PASS both widths in runs 4, 5, 7 (bounded; run 3 measured): 3x.
- walfsd-pools: PASS both widths in runs 4, 5, 7 (bounded; run 3 measured): 3x.
- vault-no-leak: PASS both widths in run 7 (16.2 s rv64, 17.4 s rv32); runs 8a and 8b: see the
  final section.
- Reference cases, run 7, both widths, all PASS: steward-ssh-two-principals, steward-context-cap,
  steward-lease-ends (the steward set), steward-home-quota (260.8 s rv64, 249.5 s rv32),
  stub-launch, steward-vault-session, steward-vault-launch, walfsd-quota, littlefsd-quota.
  Logs: `.worktrees/ATK12/target/jobs/<w>-<case>.log`; run logs `.tmp/ATK12/run[1-8].log`;
  failed boots' session and console logs copied under `.tmp/ATK12/`.

## Found on the way

- A session that carves a child of all its free pages (4,096 of the ~4,100 free at its prompt)
  starves its own VM: once the runtime panicked (exit 101, `lease alice/session-2 ... exited
  101`) at the VM's next heap growth; twice the carve was refused `out_of_memory`. Its own
  authority only (one VM is one trust domain); the probe is 2,048. Not a finding under the
  brief (no observable moved); noted for the reviewer.
- Staged files have mtime 0 (`{{1970, 1, 1}, {0, 0, 0}}`): the box's files carry no wall clock,
  so mtimes are a weak observable; sizes and listings carry the claim.
- The shell redraws a typed line after an escape sequence even with no pty; a `\A` anchored
  expect must admit CSI sequences (the quiet step).
- No unlabelled-side observable moved with the vault's work: nothing to report to the Architect.

## Final state

- Head `cdcb1e0af` on `wp-ATK12`, three commits on origin/main 4096f3861 (train 68), tree clean,
  WIPs folded; each commit is one case with the pages whose status lines name it:
  `754ffcf79` walfsd-pools (littlefsd R48, walfsd Quotas, serving R26, register, row),
  `e4428e5ad` session-hostile-launch (init R31/R32, register, row),
  `cdcb1e0af` vault-no-leak (steward R36/R37, register, row).
- vault-no-leak: PASS both widths in run 7 and twice more in run 8 (10.9 s and 10.9 s rv32,
  11.9 s and 11.7 s rv64): 3x each width, on the committed content.
- Gates on the committed content: formatting, docs (after the train-68 rebase), size-budget,
  unsafe-budget PASS; the three cases 3x both widths; the steward set and the seven reference
  cases both widths PASS (run 7).
- Next: review (kernel-red-3, the orchestrator's pick). Not run: the whole bench (the train's).

## Risks and open items

- `heap_fits = true` makes walfsd-pools a sizing question (an eighth free) rather than the 2x
  rule; walfsd:data at 37 of 64 is the number the page states.
- The handle count 15 is admission's share at the image's `buckets`; a manifest change moves it
  and the description's number.
