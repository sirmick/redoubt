B45 handoff (shell9-implementer, checkpoint at 70% context)

BRANCH STATE
- Worktree /home/mcloonan/redoubt/.worktrees/B45, branch wp-B45, clean, nothing uncommitted. Not pushed.
- Head dd3342126: one commit, "shell: a long printed line is drawn a slice at a time, and no longer ends the session", on origin/main db09d019f.
- The steward red is reviewing 46d8c6284, the same commit before the rebase. dd3342126 is that commit rebased onto db09d019f, with the machine case trimmed (ask 2). Tell the red the head moved; the diff from 46d8c6284 is only tests/shell-long-output.toml and the commit message.

GATES IN FLIGHT AT CHECKPOINT (background, started before the checkpoint)
- On dd3342126: ./test-shell, then cargo testbench --exact shell-long-output on rv64 and rv32, then doccheck.
- Logs: /home/mcloonan/redoubt/.tmp/B45/h-ts.log, h-rv64.log, h-rv32.log, h-doc.log.
- Each log's tail says PASS or FAIL. If the run died with this session, rerun:
  cd .worktrees/B45; export PATH=$HOME/.cargo/bin:$PATH BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
  q=/home/mcloonan/redoubt/scripts/q
  $q run --cores 8 -- ./test-shell
  $q run --cores 8 --lock net -- cargo testbench --arch rv64 --exact shell-long-output   (then rv32)
  $q run --cores 4 -- cargo run -q -p redoubt-doccheck
- Expect about 60-90 s per width for 70 KB plus 256 KiB at ~8 KB/s. Timeout is 240 s.

THE ORCHESTRATOR'S TWO ASKS (both done in dd3342126; only gates remain)
1. Rebase onto main db09d019f and rerun ./test-shell and shell-long-output. Rebased cleanly; the gates are above.
2. The machine case prints 70 KB and 256 KiB (262,144 z), and the session goes on. The 1 MiB lines (z and ESC) stay in the driver host test. Done: the case's description, its comment and timeout 240, and the commit message say 256 KiB.
Then report to the orchestrator as a question (reply_to its message def76890821b5c7fb186a15fff0a6b93): head dd3342126, the gates' exit codes, and that the red's diff from 46d8c6284 is the case trim only.

WHAT B45 IS (detail in .wash/local/B45-report.md)
- Cause: the shell's driver is the VM's top process (Redoubt.Shell.start → Driver.run). beamlet's VM-wide max_heap_words, a sixteenth of the session's budget = 352,256 words for 11,008 pages, counts heap plus off-heap binaries (vm.rs over_memory uses usage.total_words()). A process's own max_heap_size counts binaries only with include_shared_binaries. The old draw_text built per-byte and per-grapheme lists of the whole line, and the driver held the whole drawing beside a copy of the line. A 70 KB IO.puts got the driver killed, and the VM exited with {'EXCEPTION',exit,killed}.
- Fix:
  - lib/redoubt/term.ex: draw_text works in 4096-byte pieces, cut before a newline or at a UTF-8 sequence start. Text.visible/2 carries the tab column; advance_text walks graphemes with String.next_grapheme.
  - Term.slices/3 cuts text printed with no line open into 64 KiB put_chars pieces.
  - Term.text no longer copies a unicode binary.
  - The struct has a new field `tab` (code points of the printed line so far), so tab stops carry across cuts and prints.
  - lib/redoubt/shell/driver.ex: draw reduces over Term.slices, writing each slice before drawing the next.
  - lib/redoubt/term/text.ex: visible/2.
- Tests:
  - driver_test: start(heap_words: ...) sets max_heap_size with include_shared_binaries. 70 KB drawn on the model; 1 MiB of z and 1 MiB of ESC, checked with drawn_until(regex) and no model.
  - term_test: é at the 4096 cut with a tab after it; slices draw the same screen as the whole; an open line is not sliced.
  - tests/shell-long-output.toml: new case.
- Pages: shell.md "The terminal library", a new bullet "Printed text a piece at a time", plus bench:shell-long-output in its status list (tested count +1). Term's moduledoc.

FOLLOW-UP
- B46 (the orchestrator files it): drawing speed. The encoder draws ~8 KB/s on the machine; 1 MiB takes ~130-160 s. Likely fix: Text.visible scans printable ASCII runs as binary slices instead of one list element per byte, and advance_text takes ASCII runs at width 1 without next_grapheme. Strict track (escaping path).

TRAPS
- beamlet's VM limit counts binaries; a host test with Process.flag(:max_heap_size) misses that unless include_shared_binaries: true.
- In driver tests, the echo of the typed line contains the tag. Match printed values as ~r/\r\n:tag\r\n/ (or '^:tag' in cases), never the bare tag.
- Machine session cases need recipe = "image/boot.toml" (or vault-launch.boot.toml for beamlet-hello in /boot). A `[[file]] manifest` block without a recipe gives a loader PANIC.
- Never edit userland/shell while a testbench or prebuilt build runs in the same worktree: the build reads the tree.
- `./test-shell FILE` runs both VMs. Cargo commands for beamlet run in userland/otp (its own workspace), not the repo root.
- Scratch goes in /home/mcloonan/redoubt/.tmp/B45. A scratch test named test/redoubt/shell/scratch_test.exs must be deleted before commit; it is deleted now.

OTHER WORK THIS SESSION (all merged): SHELL9, B39, SHELL10, B42, RCMD1 (76fd2b9bd), RCMD2 (c2b328ea2). Old worktrees .worktrees/{SHELL9,B39,SHELL10,B42,RCMD1,RCMD1d} can be removed. The red's optional note on RCMD2 (shell-commands could assert File.mkdir("/home/bob") gives enoent) is still open for a later touch.
