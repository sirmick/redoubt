#!/usr/bin/env escript
%% Usage: escript tools/census.escript DIR... (each DIR holds .beam files)
%% Census of opcodes and erlang-module imports used by sets of .beam files.
main(Dirs) ->
    lists:foreach(fun(D) ->
        Files = filelib:wildcard(filename:join(D, "*.beam")),
        {Ops, Imps} = lists:foldl(fun(F, {O, I}) ->
            {beam_file, _, _, _, _, Fs} = beam_disasm:file(F),
            O2 = lists:foldl(fun({function,_,_,_,Code}, Acc) ->
                    lists:foldl(fun(Ins, A) ->
                        Op = if is_tuple(Ins) -> element(1, Ins); true -> Ins end,
                        maps:update_with(Op, fun(N) -> N+1 end, 1, A) end, Acc, Code)
                 end, O, Fs),
            {ok, {_, [{imports, Im}]}} = beam_lib:chunks(F, [imports]),
            I2 = lists:foldl(fun({M,Fn,Ar}, A) when M =:= erlang; M =:= ets; M =:= binary; M =:= lists; M =:= maps; M =:= unicode; M =:= re; M =:= math; M =:= erts_internal; M =:= persistent_term; M =:= atomics; M =:= counters ->
                                  sets:add_element({M,Fn,Ar}, A);
                             (_, A) -> A end, I, Im),
            {O2, I2} end, {#{}, sets:new()}, Files),
        ByMod = lists:foldl(fun({M,_,_}, A) -> maps:update_with(M, fun(N)->N+1 end, 1, A) end, #{}, sets:to_list(Imps)),
        io:format("~s: ~p modules, ~p distinct instrs, imports by module ~p~n  instrs: ~p~n",
                  [D, length(Files), maps:size(Ops), ByMod, lists:sort(maps:keys(Ops))])
    end, Dirs).
