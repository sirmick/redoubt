%% beamlet-budget-flood (tests/beamlet-budget-flood.toml), the backstop: one allocation larger
%% than the VM's whole budget, made inside a native, which no check between slices sees. The
%% budget refuses its pages, the VM ends, and init restarts it, which runs this again: left
%% running, it would reach init's restart limit, which reboots the machine.
-module(beamlet_budget_flood).
-export([start/0]).

start() ->
    io:put_chars("beamlet-budget-flood: one allocation past the budget\n"),
    %% 32 MiB in one piece, twice the budget's 16 MiB and below the binary limit's 128 MiB.
    Big = binary:copy(<<0>>, 32 * 1024 * 1024),
    io:put_chars(["beamlet-budget-flood: the VM survived ", integer_to_list(byte_size(Big)), "\n"]),
    ok.
