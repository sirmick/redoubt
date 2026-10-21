%% beamlet-budget-flood (tests/beamlet-budget-flood.toml), the backstop: one allocation larger
%% than the VM's whole budget, made inside a native for its own work, which no check sees. The
%% budget refuses its pages, the VM ends, and init restarts it, which runs this again: left
%% running, it would reach init's restart limit, which reboots the machine.
-module(beamlet_budget_flood).
-export([start/0]).

start() ->
    io:put_chars("beamlet-budget-flood: one allocation past the budget\n"),
    %% A list of one 512 KiB binary 64 times holds the binary once, well within the process's
    %% limit; term_to_binary writes it out 64 times, 32 MiB in its own buffer, twice the budget's
    %% 16 MiB. (A binary of that size asked for directly is refused before it is made, and only
    %% its process ends.) When term_to_binary's buffer is checked as binary:copy is, this case must
    %% move to an allocation that still is not, or it tests nothing.
    Part = binary:copy(<<0>>, 512 * 1024),
    Big = term_to_binary(copies(Part, 64, [])),
    io:put_chars(["beamlet-budget-flood: the VM survived ", integer_to_list(byte_size(Big)), "\n"]),
    ok.

copies(_, 0, Acc) -> Acc;
copies(X, N, Acc) -> copies(X, N - 1, [X | Acc]).
