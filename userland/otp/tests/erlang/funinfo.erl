%% erlang:fun_info/1,2 for local and external funs, including the module checksum (new_uniq).
-module(funinfo).
-export([start/0]).
start() ->
    X = 3,
    F = fun(A) -> A + X end,
    G = fun lists:map/2,
    Items = [type, module, name, arity, env, pid, index, new_index, uniq, new_uniq, refc],
    {[catch erlang:fun_info(F, I) || I <- Items], [catch erlang:fun_info(G, I) || I <- Items], length(erlang:fun_info(F))}.
