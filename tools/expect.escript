#!/usr/bin/env escript
%% Runs Module:start() on the real BEAM for every .beam in DIR and writes the result, formatted
%% with ~kw (maps in key order, which beamlet always uses) exactly as `beamlet` prints it, to DIR/Module.expected. The oracle for difftest.
%% Each test runs with an empty working directory, DIR/root/Module: beamlet gets the same
%% directory as its file system root (`--root`).
main([Dir]) ->
    true = code:add_patha(Dir),
    %% Tests with console input (a .stdin file next to the source) are run by difftest itself.
    [expect(Dir, M) || F <- filelib:wildcard(filename:join(Dir, "*.beam")),
                       M <- [list_to_atom(filename:basename(F, ".beam"))],
                       has_start(M), not reads_console(M)],
    ok.

reads_console(M) ->
    Src = proplists:get_value(source, M:module_info(compile), ""),
    filelib:is_regular(filename:join(filename:dirname(Src), atom_to_list(M) ++ ".stdin")).

has_start(M) ->
    {module, M} = code:ensure_loaded(M),
    erlang:function_exported(M, start, 0).

expect(Dir, M) ->
    Root = filename:join([Dir, "root", M]),
    _ = file:del_dir_r(Root),
    ok = filelib:ensure_path(Root),
    {ok, Cwd} = file:get_cwd(),
    ok = file:set_cwd(Root),
    try run(Dir, M) after file:set_cwd(Cwd) end.

run(Dir, M) ->
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
