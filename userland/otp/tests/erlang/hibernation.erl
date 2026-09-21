%% erlang:hibernate/0,3: a hibernating process waits for a message without consuming it, then
%% carries on (hibernate/0) or starts over in the given function (hibernate/3); gen_server's
%% hibernate_after makes idle servers hibernate and they keep answering.
-module(hibernation).
-behaviour(gen_server).
-export([start/0, resume/1, init/1, handle_call/3, handle_cast/2]).

resume(Parent) ->
    receive M -> Parent ! {resumed, M} end.

start() ->
    Self = self(),
    P0 = spawn(fun() -> ok = erlang:hibernate(), receive M -> Self ! {woke, M} end end),
    P3 = spawn(fun() -> erlang:hibernate(?MODULE, resume, [Self]) end),
    receive after 20 -> ok end,
    P0 ! hello, P3 ! there,
    R0 = receive {woke, A} -> A after 1000 -> timeout end,
    R3 = receive {resumed, B} -> B after 1000 -> timeout end,
    {ok, S} = gen_server:start(?MODULE, [], [{hibernate_after, 5}]),
    receive after 30 -> ok end,
    Before = gen_server:call(S, ping),
    receive after 30 -> ok end,
    After = gen_server:call(S, ping),
    {R0, R3, Before, After}.

init([]) -> {ok, 0}.
handle_call(ping, _From, N) -> {reply, {pong, N + 1}, N + 1}.
handle_cast(_, N) -> {noreply, N}.
