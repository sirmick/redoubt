# jobs.mk: the bench's cases and builds as make targets; every recipe runs through `q`, the
# machine's scheduler, which leases real cores and pins the command to them.
#
#   make -f <checkout>/scripts/jobs.mk -C <worktree> <targets>   (an absolute path to this file)
#
# No -j is needed: this file sets a high one and q paces the machine. Every cargo you run by
# hand goes through q too: `q run --cores 8 -- cargo build ...`, `q run --cores 4 -- cargo test ...`.
#
# targets
#   rv64/<case>  rv32/<case>   that one case on that width; log target/jobs/<w>-<case>.log, and
#                              when it fails its console and session logs beside it as
#                              target/jobs/<w>-<case>.smp<N>*.log (`--keep-failed`); from
#                              target/prebuilt with no cargo when its <w>/index.json exists
#                              (`target/prebuilt/testbench --prebuilt target/prebuilt`), else
#                              `cargo testbench`; a case with no `arch` (it boots nothing)
#                              runs under rv64 only
#   prebuilt                   every case's pieces for both widths built once into
#                              target/prebuilt (8 cores; one job, the widths in turn: their
#                              userland disks compile the same Mix projects). Make it again after
#                              any change to the tree: a run from one made from another is refused
#   build-rv64   build-rv32    ./build --arch <w> --programs            (8 cores)
#   docs                       cargo testbench docs                    (2 cores)
#   cases-rv64   cases-rv32    every case of that width but the quiet ones
#   quiet-rv64   quiet-rv32    the host-clock cases of that width (run on the quiet core set)
#                              (neither takes a case with `whole_run = false`: by its own target only)
#   all-rv64     all-rv32      cases + quiet
#   set CASES="<case>..."      those cases on both widths, at once, each in its own class (as
#                              rv64/<case> and rv32/<case>); add -k to run every one past a failure
#   list / list-classes        the case names; every class's members
#
# The machine's QEMU lock (docs/testbench.md "On a shared host"): a whole bench (cases-*, quiet-*,
# all-*, or a set holding every case of the boot, quiet or net class) is what the timed cases'
# bounds are measured under, with QEMU its alone for the run, so one make invocation of those
# targets holds q's `qemu` lock exclusively from before its first case to its own exit (the
# `qemu-held` target: a lock-only lease, `q run --cores 0 --lock qemu`, whose command ends when
# this make's pid is gone, so the hold ends with the lease by any exit path, a killed make
# included); a second whole bench waits for the first. Every single boot case (a case with an
# `arch`, run by name or by a smaller set) takes the lock shared, `q run --share qemu`: single
# boots run beside each other as before, and one started while a whole bench holds the lock
# waits, printing one line naming the holder and since when (in its log, and echoed with its
# result), then runs; it never fails for the wait. A case that boots nothing (no `arch`, or a host-tests or Elixir kind) takes no lock. A
# boot by hand (`cargo testbench --smp N` sweeps, `./launch`) takes it shared the same way.
#
# Classes (docs/testbench.md "On a shared host"):
#   quiet   host-clock cases: crates asserting wall-clock bounds (rt, client, r4),
#           bench-ssh-guest and the loopback deadline case (they expect a timeout), and the
#           cases whose VM session, its console on the hub's hold, must still be alive after
#           a hold boundary (a host stall of a second or more at the boundary ends it), and the
#           multi-threaded TCG cases, whose harts are host threads (sched-lock-contention-4-mttcg
#           and smp-lock-wait-4-mttcg bound host times): `q run --quiet`, one at a time on the reserved cores while the
#           rest of the machine keeps running
#   net     a [net] table opening host sockets: each leases the host ports its boot forwards
#           (the TCP forwards and the poke) from q (`--ports N`, REDOUBT_PORTS=lo-hi), so two
#           run side by side without racing for a port; one with peers or dials alone needs none
#   fanned  a host-tests case with a `fanout`: 2 cores to build, and each job asks q for its own
#   bounded other host-tests cases and the Elixir oracles (they build beamlet): 4 cores
#           (RUST_TEST_THREADS follows the lease)
#   boot    everything else: one core per guest hart (the case's largest smp on that width), icount-pinned,
#           a verdict beside anything; a timeout_secs expiry beside other work is rerun alone

SHELL := /bin/bash
.ONESHELL:
.DELETE_ON_ERROR:
MAKEFLAGS += -j64 --no-print-directory
here := $(dir $(lastword $(MAKEFILE_LIST)))
q := $(here)q
cases := $(sort $(basename $(notdir $(wildcard tests/*.toml))))
quiet := rt-host-tests client-host-tests r4-host-tests bench-ssh-guest bench-ssh-loopback-deadlock
quiet += steward-sub-budget-flood steward-ssh-two-principals steward-vault-session steward-vault-launch steward-session-ends
quiet += steward-ssh-idle launch-idle
# pipe-sixty-four: 64 pipelines on the host's clock; beside heavy load on rv32 an ended piped's
# waiter can take longer than the VM's 1 s wait to hand its end over, and the seventh fresh piped
# is refused emfile (tests/pipe-sixty-four.toml).
quiet += pipe-sixty-four
quiet += sched-lock-contention-4-mttcg smp-lock-wait-4-mttcg sched-capped-mttcg smp-evict-mttcg smp-shootdown-mttcg sched-wake-no-preempt-harts-mttcg
quiet += deadline-placement deadline-flood-billed-traced-mttcg sched-exit-churn-mttcg
quiet += beamlet-schedulers-mttcg steward-latency-mttcg
quiet := $(filter $(cases),$(quiet))
net := $(filter-out $(quiet),$(sort $(basename $(notdir $(shell grep -lE '^(forward *=|\[\[?net\.(peer|dial|poke))' $$(grep -lE '^\[net\]' tests/*.toml))))))
fanned := $(filter-out $(quiet) $(net),$(sort $(basename $(notdir $(shell grep -lE '^(fanout *=|\[fanout\])' tests/*.toml)))))
bounded := $(filter-out $(quiet) $(net) $(fanned),$(sort $(basename $(notdir $(shell grep -lE '^kind *= *"(host-tests|elixir)"' tests/*.toml)))))
boot := $(filter-out $(quiet) $(net) $(fanned) $(bounded),$(cases))
# cases with no target: one run, under rv64
hostonly := $(sort $(basename $(notdir $(shell grep -LE '^arch *=' tests/*.toml))))
# cases out of the whole run: only by their own target
byname := $(sort $(basename $(notdir $(shell grep -lE '^whole_run *= *false' tests/*.toml))))
# a whole bench: a whole-run target among the goals, or a set holding every case of a boot class
covers = $(if $(filter-out $(CASES),$(1)),,yes)
whole := $(filter cases-rv64 cases-rv32 quiet-rv64 quiet-rv32 all-rv64 all-rv32,$(MAKECMDGOALS)) \
	$(if $(and $(filter set,$(MAKECMDGOALS)),$(or $(call covers,$(boot)),$(call covers,$(quiet)),$(call covers,$(net)))),set)
whole := $(strip $(whole))
# the QEMU lock a case's lease takes: shared for a single boot, nothing under a whole bench (it
# holds the lock) or for a case that boots nothing
share = $(if $(whole)$(filter $(1),$(hostonly)),,--share qemu)
logs := target/jobs
prebuilt := target/prebuilt
# cores for a boot case on a width: its largest smp value there (default 1): the one list, or the
# width's own in the per-width form (`smp = { rv64 = [1, 2], rv32 = [1] }`; a width left out runs 1)
smp = $(or $(lastword $(sort $(shell line=$$(grep -E '^smp *=' tests/$(1).toml 2>/dev/null); [[ $$line == *'{'* ]] && line=$$(grep -oE '\b$(2) *= *\[[^]]*\]' <<< "$$line"); grep -oE '[0-9]+' <<< "$${line#*=}"))),1)
# host ports a net case forwards: every port in its `forward` lists plus one per poke; `--ports N` for q, or nothing
ports = $(if $(filter-out 0,$(1)),--ports $(1))
ports_of = $(call ports,$(shell grep -oE '[0-9]+' <<< "$$(grep -E '^forward *=' tests/$(1).toml 2>/dev/null)" | wc -l | xargs -I{} expr {} + $$(grep -cE '^\[net\.poke\]' tests/$(1).toml 2>/dev/null)))

.PHONY: list list-classes docs prebuilt build-rv64 build-rv32 cases-rv64 cases-rv32 quiet-rv64 quiet-rv32 all-rv64 all-rv32 set qemu-held \
	$(addprefix rv64/,$(cases)) $(addprefix rv32/,$(cases))

list:
	@printf '%s\n' $(cases)

list-classes:
	@printf 'quiet: %s\n' "$(quiet)"; printf 'net: %s\n' "$(net)"; printf 'fanned: %s\n' "$(fanned)"; printf 'bounded: %s\n' "$(bounded)"; printf 'boot: %s cases\n' "$(words $(boot))"; printf 'rv64 only: %s cases\n' "$(words $(hostonly))"; printf 'by name only: %s\n' "$(byname)"

docs:
	@mkdir -p $(logs); $(q) run --cores 2 --name docs -- cargo testbench docs > $(logs)/docs.log 2>&1; rc=$$?; tail -3 $(logs)/docs.log; echo "docs rc=$$rc"; exit $$rc

prebuilt:
	@mkdir -p $(logs); $(q) run --cores 8 --name prebuilt -- cargo testbench --prebuild $(prebuilt) > $(logs)/prebuilt.log 2>&1; rc=$$?; tail -2 $(logs)/prebuilt.log; echo "prebuilt rc=$$rc"; exit $$rc

build-rv64 build-rv32: build-%:
	@mkdir -p $(logs); $(q) run --cores 8 --name build-$* -- ./build --arch $* --programs > $(logs)/build-$*.log 2>&1; rc=$$?; tail -2 $(logs)/build-$*.log; echo "build-$* rc=$$rc"; exit $$rc

# The whole bench's hold. The lease's command prints `held` once it runs, which is once the
# lock is this make's, and the recipe returns then, so no case of the run starts before; the
# command then outlives nothing but this make (the recipe's parent), and the lease, so the hold,
# ends with it however it ends. A q client waiting for another bench's hold prints the line.
qemu-held:
	@exec 3< <($(q) run --cores 0 --lock qemu --name "bench $(notdir $(CURDIR))" -- bash -c 'echo held; while kill -0 "$$1" 2>/dev/null; do sleep 1; done' _ $$PPID)
	read -r held <&3; echo "qemu lock held for this run ($(CURDIR)) from $$(date -u +%H:%M:%SZ)"

ifneq ($(whole),)
$(addprefix rv64/,$(cases)) $(addprefix rv32/,$(cases)): | qemu-held
endif

define case_recipe
@mkdir -p $(logs); w=$(@D); c=$(@F)
bench=(cargo testbench); [ -f $(prebuilt)/$$w/index.json ] && bench=($(prebuilt)/testbench --prebuilt $(prebuilt))
$(q) run $(1) --name $$w/$$c -- "$${bench[@]}" --exact --arch $$w $$c --keep-failed $(logs) > $(logs)/$$w-$$c.log 2>&1; rc=$$?
grep -m1 -E '^q: .* waits for the .* lock' $(logs)/$$w-$$c.log || true
grep -E '^(PASS|FAIL|SKIP)' $(logs)/$$w-$$c.log || tail -5 $(logs)/$$w-$$c.log
echo "$@ rc=$$rc ($(2))"; exit $$rc
endef

$(addprefix rv64/,$(boot)) $(addprefix rv32/,$(filter-out $(hostonly),$(boot))): %:
	$(call case_recipe,--cores $(call smp,$(@F),$(@D)) $(call share,$(@F)),boot)

$(addprefix rv64/,$(quiet)) $(addprefix rv32/,$(filter-out $(hostonly),$(quiet))): %:
	$(call case_recipe,--quiet $(call share,$(@F)),quiet)

$(addprefix rv64/,$(net)) $(addprefix rv32/,$(filter-out $(hostonly),$(net))): %:
	$(call case_recipe,--cores $(call smp,$(@F),$(@D)) $(call ports_of,$(@F)) $(call share,$(@F)),net)

$(addprefix rv64/,$(fanned)) $(addprefix rv32/,$(filter-out $(hostonly),$(fanned))): %:
	$(call case_recipe,--cores 2,fanned)

$(addprefix rv64/,$(bounded)) $(addprefix rv32/,$(filter-out $(hostonly),$(bounded))): %:
	$(call case_recipe,--cores 4,bounded)

$(addprefix rv32/,$(hostonly)): %:
	@echo "$@: no target; rv64/$(@F) runs it"

cases-rv64: $(addprefix rv64/,$(filter-out $(quiet) $(byname),$(cases)))
cases-rv32: $(addprefix rv32/,$(filter-out $(quiet) $(byname),$(cases)))
quiet-rv64: $(addprefix rv64/,$(filter-out $(byname),$(quiet)))
quiet-rv32: $(addprefix rv32/,$(filter-out $(byname),$(quiet)))
all-rv64: cases-rv64 quiet-rv64
all-rv32: cases-rv32 quiet-rv32
set: $(addprefix rv64/,$(CASES)) $(addprefix rv32/,$(CASES))
	@echo "set: $(words $(CASES)) cases"
