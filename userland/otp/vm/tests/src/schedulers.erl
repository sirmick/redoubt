%% Fixture for vm/tests/schedulers.rs and redoubt/tests/schedulers.rs: work that crosses
%% schedulers through every path the system lock guards (spawns, sends, ETS, timers, exits), with
%% answers that show nothing was lost. Only BIFs are used: the tests load no OTP modules.
%% Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/schedulers.erl
-module(schedulers).
-export([busy/0, online/0]).

%% Eight workers sum their ranges while pairs ping-pong messages and a table takes every
%% worker's inserts; then the schedulers, the sums, the round trips and the table's size.
busy() ->
    Self = self(),
    T = ets:new(t, [public, set]),
    Workers = [spawn(fun() -> Self ! {sum, I, sum(I * 100000, I * 100000 + 50000, T, 0)} end)
               || I <- seq(1, 8)],
    Pongs = [begin
                 P = spawn(fun pong/0),
                 spawn(fun() -> Self ! {pinged, ping(P, 2000)} end)
             end || _ <- seq(1, 4)],
    Sums = [receive {sum, I, S} -> S - expected(I) end || I <- seq(1, 8)],
    Pings = [receive {pinged, N} -> N end || _ <- Pongs],
    erlang:send_after(10, Self, tick),
    Tick = receive tick -> tick after 5000 -> no_tick end,
    {erlang:system_info(schedulers), length(Workers), Sums, Pings, ets:info(T, size), Tick}.

sum(To, To, _, Acc) -> Acc;
sum(N, To, T, Acc) ->
    case N rem 1000 of
        0 when T =/= none -> ets:insert(T, {N, self()});
        _ -> ok
    end,
    sum(N + 1, To, T, Acc + N).

%% The sum of A..B-1 for worker I's range.
expected(I) ->
    A = I * 100000,
    B = A + 50000,
    (A + B - 1) * (B - A) div 2.

ping(_, 0) -> 0;
ping(P, N) ->
    P ! {self(), N},
    receive N -> 1 + ping(P, N - 1) end.

pong() ->
    receive {From, N} -> From ! N, pong() after 10000 -> ok end.

seq(A, B) when A > B -> [];
seq(A, B) -> [A | seq(A + 1, B)].

%% Rounds of two workers with one scheduler online and then two, a sleep before each, as the
%% bench's throughput case runs them: the schedulers, and the rounds finished.
online() ->
    Rounds = [begin
                  erlang:system_flag(schedulers_online, N),
                  receive after 20 -> ok end,
                  Self = self(),
                  [spawn(fun() -> Self ! {done, sum(0, 20000, none, 0)} end) || _ <- seq(1, 2)],
                  [receive {done, _} -> ok end || _ <- seq(1, 2)],
                  N
              end || _ <- seq(1, 10), N <- [1, 2]],
    {erlang:system_info(schedulers), length(Rounds)}.
