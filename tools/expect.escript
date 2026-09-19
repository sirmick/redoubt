#!/usr/bin/env escript
%% Runs Module:start() on the real BEAM for every .beam in DIR and writes the result, formatted
%% with ~kw (maps in key order, which beamlet always uses) exactly as `beamlet` prints it, to DIR/Module.expected. The oracle for difftest.
main([Dir]) ->
    true = code:add_patha(Dir),
    [expect(Dir, list_to_atom(filename:basename(F, ".beam")))
     || F <- filelib:wildcard(filename:join(Dir, "*.beam"))],
    ok.

expect(Dir, M) ->
    Self = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        R = try M:start() catch C:E -> {'EXCEPTION', C, E} end,
        Self ! {self(), R}
    end),
    Result = receive
        {Pid, R} -> io_lib:format("~kw~n", [R]);
        {'DOWN', Ref, process, Pid, Reason} -> io_lib:format("~kw~n", [{'EXCEPTION', exit, Reason}])
    after 10000 ->
        exit(Pid, kill),
        "TIMEOUT\n"
    end,
    ok = file:write_file(filename:join(Dir, atom_to_list(M) ++ ".expected"), Result).
