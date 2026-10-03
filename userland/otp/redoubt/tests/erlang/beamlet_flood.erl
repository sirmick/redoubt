%% beamlet-heap-flood (tests/beamlet-heap-flood.toml), the attack: a process of the VM floods its
%% heap. Its heap limit, a sixteenth of the VM's budget, kills it; the module sees it go, and the
%% same VM then still reads and echoes a typed line, so the flood did not end the VM.
-module(beamlet_flood).
-export([start/0]).

start() ->
    io:put_chars("beamlet-heap-flood: flooding\n"),
    {Pid, Ref} = spawn_monitor(fun() -> flood([]) end),
    receive
        {'DOWN', Ref, process, Pid, Reason} when is_atom(Reason) ->
            io:put_chars(["beamlet-heap-flood: the flood ended: ", atom_to_list(Reason), "\n"])
    end,
    io:put_chars("beamlet-heap-flood: type a line\n"),
    Line = io:get_line(""),
    io:put_chars(["echo: ", [C || C <- Line, C =/= $\n, C =/= $\r], "\n"]),
    ok.

%% Every step keeps everything it made, so the heap only grows.
flood(Kept) -> flood([lists:seq(1, 1000) | Kept]).
