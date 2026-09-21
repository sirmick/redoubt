-module(processes).
-export([start/0, echo/0]).
%% spawn, send, receive (selective, with timeouts), links, monitors, registered names.
start() ->
    Self = self(),
    P = spawn(?MODULE, echo, []),
    P ! {Self, hello},
    R1 = receive {P, M} -> M after 1000 -> timeout end,
    self() ! b, self() ! a,
    R2 = receive a -> got_a end,
    R3 = receive b -> got_b end,
    R4 = receive nothing -> x after 10 -> timed_out end,
    Ref = monitor(process, spawn(fun() -> exit(boom) end)),
    R5 = receive {'DOWN', Ref, process, _, Why} -> Why after 1000 -> no_down end,
    process_flag(trap_exit, true),
    L = spawn_link(fun() -> exit(linked_exit) end),
    R6 = receive {'EXIT', L, Why2} -> Why2 after 1000 -> no_exit end,
    register(me, self()),
    me ! via_name,
    R7 = receive via_name -> {named, whereis(me) =:= self()} end,
    Ref2 = monitor(process, spawn(fun() -> ok end)),
    R8 = receive {'DOWN', Ref2, process, _, W} -> W end,
    Workers = [spawn(fun() -> Self ! {done, N * N} end) || N <- lists:seq(1, 20)],
    R9 = lists:sort([receive {done, V} -> V end || _ <- Workers]),
    {R1, R2, R3, R4, R5, R6, R7, R8, lists:sum(R9), is_process_alive(self())}.
echo() ->
    receive {From, Msg} -> From ! {self(), Msg}, echo() end.
