# B9 report

Branch wp-B9, head 7139105af, one commit on main f11e8c2e9:
`testbench: the keeper's tests scan /proc once the stand-in's exec is done`.
Test-only: tools/testbench/src/ssh.rs (the tests module, and one doc sentence on
`processes_naming`). The keeper's code is unchanged. Tier B.

## Cause

- **Reproduced** beside load: 1 of 100 runs of `ssh::tests::the_keeper*` on one leased core with
  two CPU hogs on it (one q job running the loop):
  `the_keeper_waits_to_the_deadline ... left: [] right: [<pid>]`, the scan right after the spawn.
- **`Command::spawn` returns before the child's `execve` is finished.** glibc's `posix_spawn`
  (`CLONE_VFORK`) lets the parent go when the new address space is installed, before the argument
  area is set up. Until then `/proc/<pid>/cmdline` reads empty, so the scan sees no `sh`. On a
  loaded host the scan lands in that window.
- **Ruled out:** dash does not exec away its single command here (checked: the cmdline stays
  `sh -c sleep 2 <named>`).
- **The keeper is not exposed.** It scans after a case's sessions have ended, long after its
  guests started.

## Fix

- **`exec_done(program, named, pid)`:** polls `processes_naming` until the pid shows as `program`
  naming the case, with a 20 s bound and a failure that names it. Both keeper tests call it after
  each spawn, then assert as before.
- **The late guest goes on an event:** `sh -c "read _"` with piped stdin, closed by the reaper
  thread, where it was `sleep 1`. A 200 ms pause before the close only makes it likely the keeper
  sees it running first; the verdict (None) is the same either way, as the comment says.
- **`processes_naming`'s doc** now says a process mid-`execve` briefly has no command line, and
  why the keeper never meets it.

## Counts

| Runs of ssh::tests::the_keeper* | Before | After |
| --- | --- | --- |
| Alone (`q run --quiet --cores 1`, nothing else on the core) | 0 of 30 (plan node) | 0 of 50 failed |
| Beside load (`q run --cores 1`, two busy loops on the same core in the job) | 1 of 100 | 0 of 50 failed |

## Gates on 7139105af, all rc=0

- prebuilt
- docs
- formatting
- size-budget
- unsafe-budget
- no-cruft
- the two counts above

## Docs checked

- docs/testbench.md, "Sessions and the loopback server" (the keeper): describes the keeper, not
  its tests, and the keeper is unchanged. No change.
- Its status list names `host:testbench::the_keeper_finds_what_names_the_case` and
  `host:testbench::the_keeper_waits_to_the_deadline`. Neither was renamed. No change.
