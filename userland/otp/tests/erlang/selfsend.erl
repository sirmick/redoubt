-module(selfsend).
%% A process that keeps sending itself messages still gets the ones others send it: messages to
%% itself queue behind them, as on BEAM (a regression: they used to starve them).
-export([start/0]).

start() ->
    P = spawn(fun() -> spin(0) end),
    P ! tick,
    timer:sleep(20),
    P ! {stop, self()},
    receive
        {stopped, N} when N > 0 -> ok
    after 2000 ->
        exit(P, kill),
        starved
    end.

spin(N) ->
    receive
        {stop, From} -> From ! {stopped, N};
        tick ->
            self() ! tick,
            spin(N + 1)
    end.
