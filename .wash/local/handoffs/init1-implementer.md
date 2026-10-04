INIT1 handoff (init1-implementer). Worktree /home/mcloonan/redoubt/.worktrees/init1, branch wp-init1, base a9eed8300. Clean tree. Detail: .wash/local/INIT1-progress.md.

STATE
D1 device_info DONE, checkpoint accepted: 50f71a55e (redoubt-sys call/ret/error/tests, kernel device.rs device_info + redoubt.rs arm, abi.md rows, size kernel 7734, libs/sys 1006); a678eec1d (tests/programs/src/bin/device-info-attack.rs, tests/device-info-attack.toml, Cargo bin, rd::device_info). 5f7de2dd8 WIP model work (syscall/trace/gen/mutation/ghost/invariants/kernel arm, model/tests/device_info_contracts.rs, devices.md built + status, model.md R18 row, size model 10152). Before acceptance: rebase onto IPC2's merge, keep the model commit LAST, reword it to a real message (e.g. 'model: device_info and the check that its answer names the handle's device') keeping the 'Size budget: model: ...' line. Also rebase onto GATE1 before acceptance (brief).
D2 NOT STARTED, waits for K14's merge (orchestrator announces). I did not read setup_loader_process or loader main.rs. Per brief: loader loads kernel + 2nd entry only, IniE goes (kernel ptable.rs ~222 counts IniE), maps whole verified bundle RO into that process outside its link range, frames charged to root, never freed; a0/a1 = bundle addr/len (setup_loader_process: a0/a1 only); kernel budget.rs boot_budgets: root keeps 1 process, system 15, users 47, process in root on INIT_WEIGHT; same slots (budgets 1-3, Reset 4, console 5-6, devices on), no boot/log endpoint (boot_endpoint, boot_log_endpoint). Keep K14's USER_STACK_* lines as one block. Ask the orchestrator before touching the grants refusal if a case expects it. New case bundle-mapped.
D3, D4 NOT STARTED (orchestrator: do not start).

COMMANDS (from the worktree; never host cargo)
.wash/local/in-dev = /home/mcloonan/redoubt/.wash/local/in-dev
in-dev cargo testbench --allow-skip > /tmp/init1-bench.log 2>&1 (whole bench, ~40 min; tail it). in-dev cargo testbench device-info-attack; in-dev cargo testbench size-budget. Case logs: target/testbench/<case>-<arch>-smp1.log. in-dev cargo +nightly fmt --all [--check]; in-dev cargo run -q -p redoubt-doccheck. Model: in-dev bash -c 'cargo test -q --release -p redoubt-model --test device_info_contracts; REDOUBT_MODEL_MUTATIONS=DeviceInfo,R18 cargo test -q --release -p redoubt-model --test mutations -- --nocapture'. Killing in-dev does not kill the container's processes: ps and kill them by path.

TRAPS
- Encoding: a1 kind tag 1 MMIO/2 IRQ/3 Reset, a2-a3 a (u64 lo/hi), a4-a5 b (u64), a6 flags bit0 DMA, a7 0. Decoder (libs/sys ret.rs DeviceInfo::read) refuses unknown kind, MMIO flags other than 0/1, any non-zero field the kind does not use, IRQ > u32, non-zero a7.
- Mutation DeviceInfoWrongKind (R18; IRQ reported kind 1) is caught only through Flow::DeviceInfo (pushed in the model kernel arm) checked by device_info_answer in invariants.rs, which recomputes from the handle table and k.devices; caught kernel_sequence seed 76. gen.rs uses below(101) with 99 = device_info.
- size-budget reports only the first crate over; recheck after each raise. Uncommitted raises fail it.
- doccheck C1: an '**Open:**' line outside a planned section fails; remove it when a section goes built.
- uaf-lent-page rv64 can flake under load from other agents' benches.
- No git stash; restructure commits with reset --soft and restaging by path.

OPEN
- Architect note (not acted on): devices.md 'Which process gets which device' (INIT2's, planned) still says 'the kernel still says nothing about which device a handle names', which now contradicts device_info.
