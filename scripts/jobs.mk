# jobs.mk: the bench's cases and builds as make targets; every recipe runs through `q`, the
# machine's scheduler, which leases real cores and pins the command to them.
#
#   make -f <checkout>/scripts/jobs.mk -C <worktree> <targets>   (an absolute path to this file)
#
# No -j is needed: this file sets a high one and q paces the machine. Every cargo you run by
# hand goes through q too: `q run --cores 8 -- cargo build ...`, `q run --cores 4 -- cargo test ...`.
#
# targets
#   rv64/<case>  rv32/<case>   that one case on that width; log target/jobs/<w>-<case>.log: from
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
# Classes (docs/testbench.md "On a shared host"):
#   quiet   host-clock cases: crates asserting wall-clock bounds (rt, client, r4),
#           bench-ssh-guest and the loopback deadline case (they expect a timeout), and the
#           cases whose VM session, its console on the hub's hold, must still be alive after
#           a hold boundary (a host stall of a second or more at the boundary ends it): `q run
#           --quiet`, one at a time on the reserved cores while the rest of the machine keeps
#           running
#   net     a [net] table opening host sockets: one at a time among themselves (`--lock net`)
#   fanned  a host-tests case with a `fanout`: 2 cores to build, and each job asks q for its own
#   bounded other host-tests cases and the Elixir oracles (they build beamlet): 4 cores
#           (RUST_TEST_THREADS follows the lease)
#   boot    everything else: one core per guest hart (the case's largest smp), icount-pinned,
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
quiet += steward-ssh-idle
quiet := $(filter $(cases),$(quiet))
net := $(filter-out $(quiet),$(sort $(basename $(notdir $(shell grep -lE '^(forward *=|\[\[?net\.(peer|dial|poke))' $$(grep -lE '^\[net\]' tests/*.toml))))))
fanned := $(filter-out $(quiet) $(net),$(sort $(basename $(notdir $(shell grep -lE '^(fanout *=|\[fanout\])' tests/*.toml)))))
bounded := $(filter-out $(quiet) $(net) $(fanned),$(sort $(basename $(notdir $(shell grep -lE '^kind *= *"(host-tests|elixir)"' tests/*.toml)))))
boot := $(filter-out $(quiet) $(net) $(fanned) $(bounded),$(cases))
# cases with no target: one run, under rv64
hostonly := $(sort $(basename $(notdir $(shell grep -LE '^arch *=' tests/*.toml))))
# cases out of the whole run: only by their own target
byname := $(sort $(basename $(notdir $(shell grep -lE '^whole_run *= *false' tests/*.toml))))
logs := target/jobs
prebuilt := target/prebuilt
# cores for a boot case: its largest smp value (default 1)
smp = $(or $(lastword $(sort $(shell grep -oE '[0-9]+' <<< "$$(grep -E '^smp *=' tests/$(1).toml 2>/dev/null)"))),1)

.PHONY: list list-classes docs prebuilt build-rv64 build-rv32 cases-rv64 cases-rv32 quiet-rv64 quiet-rv32 all-rv64 all-rv32 set \
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

define case_recipe
@mkdir -p $(logs); w=$(@D); c=$(@F)
bench=(cargo testbench); [ -f $(prebuilt)/$$w/index.json ] && bench=($(prebuilt)/testbench --prebuilt $(prebuilt))
$(q) run $(1) --name $$w/$$c -- "$${bench[@]}" --exact --arch $$w $$c > $(logs)/$$w-$$c.log 2>&1; rc=$$?
grep -E '^(PASS|FAIL|SKIP)' $(logs)/$$w-$$c.log || tail -5 $(logs)/$$w-$$c.log
echo "$@ rc=$$rc ($(2))"; exit $$rc
endef

$(addprefix rv64/,$(boot)) $(addprefix rv32/,$(filter-out $(hostonly),$(boot))): %:
	$(call case_recipe,--cores $(call smp,$(@F)),boot)

$(addprefix rv64/,$(quiet)) $(addprefix rv32/,$(filter-out $(hostonly),$(quiet))): %:
	$(call case_recipe,--quiet,quiet)

$(addprefix rv64/,$(net)) $(addprefix rv32/,$(filter-out $(hostonly),$(net))): %:
	$(call case_recipe,--cores $(call smp,$(@F)) --lock net,net)

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
