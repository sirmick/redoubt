-module(stacktraces).
-export([start/0, id/1]).
%% Stack traces: functions, arities or arguments, and {file, line} locations, as BEAM reports
%% them. BIF entries carry BEAM-specific error_info, so only their MFA is compared.
start() ->
    [trace(fun() -> clause(id(x)) end),
     trace(fun() -> nested(3) end),
     trace(fun() -> throw_here() end),
     trace(fun() -> badmatch_here(id(1)) end),
     bif_top(trace(fun() -> element(id(5), id({a})) end)),
     trace(fun() -> ?MODULE:missing(1) end)].

id(X) -> X.

trace(F) ->
    try F() of
        V -> {no_exception, V}
    catch
        C:R:St -> {C, R, [{M, Fn, A, strip(L)} || {M, Fn, A, L} <- St, M =:= ?MODULE orelse M =:= erlang]}
    end.

%% Keep only file and line: the location is what we compare.
strip(L) -> [{K, V} || {K, V} <- L, K =:= file orelse K =:= line].

bif_top({C, R, [{erlang, F, A, _} | Rest]}) -> {C, R, [{erlang, F, A} | Rest]};
bif_top(Other) -> Other.

clause(1) -> one.

nested(0) -> erlang:error(bottom);
nested(N) -> [nested(N - 1)].

throw_here() -> throw(up).

badmatch_here(X) ->
    {ok, _} = X,
    unreachable.
