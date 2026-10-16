# AGT3: compiling on the box, proven in a boot

Branch wp-AGT3, worktree /home/mcloonan/redoubt/.worktrees/AGT3.
Head **66c36d3bb**: the one commit (df6003eec's content, which amended 2f7c2b4ca with the
review's two folds: beamlet.md's status token, the 11,265 wording) rebased onto AGT4's final head
c90196606, itself on main 0b487325b (train 36). First developed on base 326b6dca1 (scratch merge
of AGT4 9dfc1c743 + AGT1 0771a430f). Content-only rebase; after it: prebuilt rc=0, `set
CASES=shell-compile WIDTHS=rv64,rv32` rc=0 (PASS rv64 10.3 s, PASS rv32 9.5 s), docs PASS rc=0.
Train 37 = AGT4 + AGT3.

## Delivered

- `tests/shell-compile.toml` (new): one session as `agent` over SSH, `image/boot.toml`, 1 GiB,
  300 s, both widths. Tagged lines as `shell-commands`; the login lines sit in `expect_after`
  (the sessions start only once `expect` has all matched, case.rs:441).
- `docs/userland/development.md` "The compilers on beamlet": status line loses "runs on the host
  only", gains `· tested: bench:shell-compile, bench:shell-compile-module`; the differential
  suites' clause unchanged; one body sentence on where a compiled module lives on the machine and
  what the lookup never searches.
- `docs/plan/m3-agents.md` "Progress": one clause beside AGT1's.
- `docs/plan/m5-self-hosted.md` "Progress": the clause extended by `shell-compile`.
- No code on the box changed. `userland/otp` untouched.

## Step 4: brief vs pages (resolved with the orchestrator, option a)

The brief expected `Code.append_path("/home/agent/lib")` to leave the call raising. The pages say
otherwise: packages.md:77-82 (BL1) confines `Platform::load_module` to the bundle and the
profile, "never the session's writable namespace (a directory a session puts on its own code
path is its own choice)"; beamlet.md:334-338: `Absent` searches the code path's directories,
including added ones; vm.rs:980 `find_in_code_path` reads them through the session's files. The
case states BL1 as the pages do: step 3 is the BL1 verdict (purged name, `.beam` in home, the
call raises), step 4 shows append_path works as the page says, then `Code.delete_path` + purge
before the `load_binary` reload. The orchestrator confirmed (a); the owner is being told.

## The case's steps and the shell's printed values (identical on rv64 and rv32)

1. `[{mod, bin}] = Code.compile_string("defmodule Homemade do def double(xs), do: Enum.map(xs, &(&1 * 2)) end")` → `{:comp, Homemade, true, [2, 4, 6]}`
2. `File.mkdir_p("/home/agent/lib")` → `{:mk, :ok}`; `File.write(".../Elixir.Homemade.beam", bin)` → `{:wr, :ok}`; `File.stat!(...).size == byte_size(bin)` → `{:kept, true}`
3. `:code.purge(mod); :code.delete(mod)` → `{:del, true}`; `mod.double([1])` rescued → `{:gone, UndefinedFunctionError}`
4. `Code.append_path("/home/agent/lib")` → `{:path, true}`; `mod.double([1, 2, 3])` → `{:viapath, [2, 4, 6]}` (from the file in home); `Code.delete_path` + purge/delete → `{:unpath, true, true}`; call → `{:gone2, UndefinedFunctionError}`
5. `:code.load_binary(mod, charlist, File.read!(...))` → `{:reload, {:module, Homemade}}`; `mod.double([1, 2, 3])` → `{:again, [2, 4, 6]}`
6. `Code.eval_string("a + b", a: 1, b: 2)` → `{:eval, 3}`
7. `rm_rf("/home/agent/lib"), File.exists?(...)` → `{:rm, :ok, false}`; purge/delete → `{:clean, true}`

Console verdicts: steward `carved users/agent account 1003`, `audit 1003/{} ... Login { ...
principal: 2, key: [1-9]`, `users/agent/{} holds 11265 pages, 10 processes`, sshd `login agent:
session ..., labels []`, the shell's banner. Memory: the `holds 11265 pages` line is the
steward's carve at login (sizes.session + 1), a fixed figure printed before the compile, not a
measurement of it; the case measures nothing. No rv32 memory failure, no limit changed.

Transcripts kept: `.worktrees/AGT3/.tmp/AGT3/{rv64,rv32}-agent.ssh.pass.log`,
`{rv64,rv32}-console.pass.log`; the failed attempts' logs `*-shell-compile.{1,2}.log`.

## Gates (all through scripts/q)

- `make -f scripts/jobs.mk prebuilt` → rc=0 (three times: the prebuilt is keyed to the tree).
- `make -j4 -f scripts/jobs.mk set CASES=shell-compile WIDTHS=rv64,rv32` → rc=0:
  `PASS shell-compile [rv64, smp=1] 10.4s`, `PASS shell-compile [rv32, smp=1] 9.4s`
  (agent's prompt 2.95 s / 2.67 s after ssh started).
- `make -f scripts/jobs.mk docs` → `PASS docs`, rc=0, on the final tree.
- Not run: the whole bench, the unsafe ratchet, size budget (Tier B, no code change).

Attempts before the pass, my errors both: (1) the login line in `expect`, so the session never
started (300 s timeout both widths); (2) `mod.double([4])` prints `~c"\b"` (a charlist), so the
pattern `[8]` never matched though the call had answered from home.

## Summaries checked

- `README.md:44-45`: "compilers run on beamlet, on the host; a module defined at a session's
  prompt compiles on the machine": still true and not contradicted; left as is.
- `docs/userland/README.md:25,91`: "compilers on the box": consistent; no change.
- `GETTING-STARTED.md`, `docs/README.md`, `image/README.md`, `docs/userland/agents.md`,
  `docs/userland/shell.md`: no host-only compiler claim; no change.
- `docs/userland/beamlet.md` "beamlet on Redoubt" status: `bench:shell-compile` added beside
  `shell-compile-module` (review fold).
- `docs/userland/packages.md` BL1: the case cites it; the rule stands unchanged.
- `docs/plan/m5-self-hosted.md`: the Progress clause extended rather than reworded ("the one
  word" of the brief read as the minimal change).

## Open risks

None known. The case takes ~10 s of session time after a ~3 s login; 300 s is generous for the
pool.
