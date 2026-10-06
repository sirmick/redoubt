# jobs.mk: the bench's cases and builds as make targets, so make schedules them across the
# machine and every cargo under them shares one token pool (see `jobserver`).
#
#   eval "$(/home/mcloonan/redoubt/.wash/local/jobserver env)"      # once per shell
#   make -f /home/mcloonan/redoubt/.wash/local/jobs.mk -C <worktree> <targets>
#
# Never pass -j: MAKEFLAGS carries the shared jobserver and the pool decides how many of your
# targets (and of everyone else's, and of every rustc under them) run at once. A -j on the
# command line would start a private pool and oversubscribe the machine.
#
# targets
#   rv64/<case>  rv32/<case>   run `cargo testbench --arch <w> <case>`; log target/jobs/<w>-<case>.log
#   build-rv64   build-rv32    ./build --arch <w> --programs
#   host                       cargo testbench host-tests (every host-tests case)
#   docs                       cargo testbench docs
#   cases-rv64   cases-rv32    every shared/net/bounded case of that width (one QEMU each)
#   exclusive-rv64/-rv32       every host-clock case of that width, one at a time
#   list                       the case names (tests/*.toml); `list-exclusive` the alone ones;
#   list-classes               every class's members
#
# Sharing follows docs/testbench.md "On a shared host": a boot case runs beside anything (one
# extra token for its guest; a failure of one without icount beside other work is rerun alone
# before it counts); a host-clock case (kind = "host-tests" or "ssh-loopback", or a [net] table)
# is a verdict only alone, so its target waits for every shared job to end and holds every token
# while it runs. A target names one case exactly (a tests/*.toml stem); the bench itself takes
# that name as a substring filter, so a case whose name is a prefix of others' (sched-cluster,
# sched-latency) may run them too: read the log's header for what ran. A guest case that only ran out
# of timeout_secs beside other work has no verdict: rerun it alone.

SHELL := /bin/bash
.ONESHELL:
.DELETE_ON_ERROR:
here := $(dir $(lastword $(MAKEFILE_LIST)))
js := $(here)jobserver
cases := $(sort $(basename $(notdir $(wildcard tests/*.toml))))
# Classes, derived from the facts the page names (docs/testbench.md "On a shared host"):
#   alone   host-clock: crates asserting wall-clock bounds (rt, client, r4 = keyd/consoled/bootfsd,
#           rt-miri) and bench-ssh-guest (expects a timeout); model-host-tests is bounded (its
#           thread count comes from RUST_TEST_THREADS and its wall-clock bounds are counts)
#   net     a [net] table that opens host sockets (forward/poke/peer/dial): shared, but one at a
#           time among themselves (the port race)
#   bounded other host-tests cases: shared, cargo test's threads held to BOUNDED_THREADS
#   shared  every other case (boot cases; an empty [net] table binds no host port)
# ssh-loopback cases left the alone class: the loopback sshd runs in inetd mode over a
# ProxyCommand and binds no port; only bench-ssh-loopback-deadlock expects a timeout (testbench.md).
# host-tests (the case) runs every crate's host tests, the alone ones included, so it is alone too.
alone := rt-host-tests client-host-tests r4-host-tests rt-miri bench-ssh-guest bench-ssh-loopback-deadlock host-tests
alone := $(filter $(cases),$(alone))
net := $(filter-out $(alone),$(sort $(basename $(notdir $(shell grep -lE '^(forward *=|\[\[?net\.(peer|dial|poke))' $$(grep -lE '^\[net\]' tests/*.toml))))))
bounded := $(filter-out $(alone) $(net),$(sort $(basename $(notdir $(shell grep -lE '^kind *= *"host-tests"' tests/*.toml)))))
exclusive := $(alone)
shared := $(filter-out $(alone) $(net) $(bounded),$(cases))
logs := target/jobs

.PHONY: list list-exclusive host docs build-rv64 build-rv32 cases-rv64 cases-rv32 \
	$(addprefix rv64/,$(cases)) $(addprefix rv32/,$(cases))

list:
	@printf '%s\n' $(cases)

list-exclusive:
	@printf '%s\n' $(exclusive)

list-classes:
	@printf 'alone: %s\n' "$(alone)"; printf 'net: %s\n' "$(net)"; printf 'bounded: %s\n' "$(bounded)"; printf 'shared: %s cases\n' "$(words $(shared))"

host:
	+@mkdir -p $(logs); $(js) all cargo testbench host-tests > $(logs)/host.log 2>&1; rc=$$?; grep -E '^(PASS|FAIL|SKIP)' $(logs)/host.log; echo "host rc=$$rc (alone)"; exit $$rc

docs:
	+@mkdir -p $(logs); $(js) share cargo testbench docs > $(logs)/docs.log 2>&1; rc=$$?; tail -3 $(logs)/docs.log; echo "docs rc=$$rc"; exit $$rc

build-rv64 build-rv32: build-%:
	+@mkdir -p $(logs); $(js) share ./build --arch $* --programs > $(logs)/build-$*.log 2>&1; rc=$$?; tail -2 $(logs)/build-$*.log; echo "build-$* rc=$$rc"; exit $$rc

# Static pattern rules: make does not try implicit pattern rules for .PHONY targets.
$(addprefix rv64/,$(shared)) $(addprefix rv32/,$(shared)): %:
	+@mkdir -p $(logs); w=$(@D); c=$(@F)
	$(js) take cargo testbench --arch $$w $$c > $(logs)/$$w-$$c.log 2>&1; rc=$$?
	grep -E '^(PASS|FAIL|SKIP)' $(logs)/$$w-$$c.log || tail -5 $(logs)/$$w-$$c.log
	echo "$@ rc=$$rc (shared)"; exit $$rc

$(addprefix rv64/,$(exclusive)) $(addprefix rv32/,$(exclusive)): %:
	+@mkdir -p $(logs); w=$(@D); c=$(@F)
	$(js) all cargo testbench --arch $$w $$c > $(logs)/$$w-$$c.log 2>&1; rc=$$?
	grep -E '^(PASS|FAIL|SKIP)' $(logs)/$$w-$$c.log || tail -5 $(logs)/$$w-$$c.log
	echo "$@ rc=$$rc (alone)"; exit $$rc

$(addprefix rv64/,$(net)) $(addprefix rv32/,$(net)): %:
	+@mkdir -p $(logs); w=$(@D); c=$(@F)
	$(js) net cargo testbench --arch $$w $$c > $(logs)/$$w-$$c.log 2>&1; rc=$$?
	grep -E '^(PASS|FAIL|SKIP)' $(logs)/$$w-$$c.log || tail -5 $(logs)/$$w-$$c.log
	echo "$@ rc=$$rc (net)"; exit $$rc

$(addprefix rv64/,$(bounded)) $(addprefix rv32/,$(bounded)): %:
	+@mkdir -p $(logs); w=$(@D); c=$(@F)
	$(js) bounded cargo testbench --arch $$w $$c > $(logs)/$$w-$$c.log 2>&1; rc=$$?
	grep -E '^(PASS|FAIL|SKIP)' $(logs)/$$w-$$c.log || tail -5 $(logs)/$$w-$$c.log
	echo "$@ rc=$$rc (bounded)"; exit $$rc

# The shared, net and bounded cases of a width; the alone ones are exclusive-rv64/-rv32, run after
# (an exclusive target started beside shared ones closes the gate while it waits and starves them).
cases-rv64: $(addprefix rv64/,$(filter-out $(exclusive),$(cases)))
cases-rv32: $(addprefix rv32/,$(filter-out $(exclusive),$(cases)))

# Ask for these rather than listing exclusive cases by name: make then starts them one at a
# time instead of launching every one to queue on the lock, each holding a job token idle.
.PHONY: exclusive-rv64 exclusive-rv32
.NOTPARALLEL: exclusive-rv64 exclusive-rv32
exclusive-rv64: $(addprefix rv64/,$(exclusive))
exclusive-rv32: $(addprefix rv32/,$(exclusive))
