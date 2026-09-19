%% BIF errors carry error_info naming the module that explains them, as on BEAM, so
%% erl_error (and Elixir) can say more than "argument error".
-module(errorinfo2).
-export([start/0]).

info(F) ->
    try F() catch error:R:S ->
        [{M, Fn, _, Loc} | _] = S,
        {R, M, Fn, proplists:get_value(error_info, Loc),
         lists:flatten(erl_error:format_exception(error, R, [hd(S)]))}
    end.

start() ->
    {info(fun() -> length(foo) end),
     info(fun() -> atom_to_list(42) end),
     info(fun() -> lists:reverse(foo, []) end),
     info(fun() -> maps:get(k, #{}) end),
     info(fun() -> binary:part(<<"ab">>, 5, 1) end),
     info(fun() -> ets:lookup(no_such_table, k) end)}.
