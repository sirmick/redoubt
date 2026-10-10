shell-implementer-2 handoff (after SHELL3 and SHELL4, both merged: SHELL4 = main 44d970ab4). No open package, nothing uncommitted. Scratch for SHELL3/SHELL4 deleted; worktrees .worktrees/SHELL3 and .worktrees/SHELL4 (branches wp-SHELL3, wp-SHELL4) left for the orchestrator to clean.

## What SHELL4 left in the tree (for whoever builds on it)
- Redoubt.Screen.serve/5 + call/1: a screen asks its caller (the evaluator) for things only the caller may touch (a cat's raw file). call/1 monitors the caller. Screens run with the caller's heap limit and include_shared_binaries: true. terminal_size/0: :none unless the group leader is OTP group (getopts has :expand_fun) and the driver's size is known; the driver answers {:redoubt_screen, :size, pid} with {cols,rows}|:unknown (beamlet console_size :unknown on the machine: consoled/sshd don't serve consol yet).
- Redoubt.Screen.Pager (+ Pager.Doc pure model). Printer.value(%Lines{}) -> Pager.show; Printer.out/1 and the `out` commandlet bypass it. Lines.style nil|:help; Help.styled/1.
- Redoubt.Shell.Completer: Completer.tab/4 called from a closure the shell builds before each read (so the module loads at first Tab). Never parse typed text: call/1 scans brackets/commas on a head with strings/charlists blanked; Registry.fetch by string; Registry.names/0 from SHELL7's index. Test asserts atom_count unchanged (with warm-up Tabs first, since loading a module adds atoms).
- New param type `term`.

## Traps learned
- Code.Fragment.container_cursor_to_quoted interns atoms and ignores existing_atoms_only. cursor_context returns :none for :Elixir.X forms.
- beamlet (like BEAM) leaves refc binaries out of a process's own max_heap_size unless include_shared_binaries: true.
- File.stream! opens raw: only the opening process may read it.
- beamlet-footprint: every module loaded at the prompt costs pages (rv64 cap 11,885 = twice the peak). Never load a module at the prompt; check with .tmp/B32/loaded.sh (fake kernel) or the case's target/jobs/rv64-beamlet-footprint.log ("heap beamlet N of 11885"). Baselines after SHELL4: rv64 5,455, rv32 5,275.
- A cells diff sends only changed cells and skips blanks on clear: pty tests wait for single words, not phrases.
- Prebuilt fingerprints the tree: never edit the worktree while `make prebuilt` runs.
- ~S|...| ends at a `|` (a pipe!): use ~S'...' or ~S{...}. Write tool turns \uXXXX into raw chars: restore with python; check with the grep in the SHELL3 handoff.
- Gates for shell work: ./test-shell; make -f scripts/jobs.mk prebuilt; rv64/docs rv64/formatting; make -j -f scripts/jobs.mk set CASES="$(scripts/shell-cases origin/main)" (orchestrator asked for -j here).
- Splitting one worktree's changes into two commits: save full files, write the first commit's versions, commit, restore, commit.

## Findings still open (reported)
- None from SHELL4 beyond what was fixed. (The 256-column prompt bug was fixed on main by the shell batch.)
