%% Stack traces of failing applies and BIFs: which frames BEAM reports and in what order.
-module(stacktraces2).
-export([start/0, s/2, t/1, u/0]).

s(_, N) -> N + foo.
t(M) -> M:foo().
u() -> apply(1, foo, []).

frames(F) ->
    try F() catch _:_:S -> [{M, Fn, A} || {M, Fn, A, _} <- lists:sublist(S, 3)] end.

start() ->
    {frames(fun() -> apply(?MODULE, s, [x, 1]) end),
     frames(fun() -> t(1) end),
     frames(fun() -> u() end),
     frames(fun() -> t({not_an_atom}) end)}.
