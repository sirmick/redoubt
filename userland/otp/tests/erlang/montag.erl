%% monitor/3 with {tag, Tag}: the down message starts with Tag; demonitor's flush removes it.
-module(montag).
-export([start/0]).

start() ->
    P = spawn(fun() -> receive stop -> ok end end),
    R = erlang:monitor(process, P, [{tag, {mine, 1}}]),
    P ! stop,
    Got = receive {{mine, 1}, R, process, P, Why} -> {tagged, Why} after 1000 -> none end,
    Dead = erlang:monitor(process, P, [{tag, gone}]),
    Noproc = receive {gone, Dead, process, P, W2} -> W2 after 1000 -> none end,
    Q = spawn(fun() -> ok end),
    R2 = erlang:monitor(process, Q, [{tag, t2}]),
    receive after 20 -> ok end,
    Flushed = erlang:demonitor(R2, [flush, info]),
    Left = receive {t2, R2, _, _, _} -> left after 0 -> none end,
    {Got, Noproc, Flushed, Left}.
