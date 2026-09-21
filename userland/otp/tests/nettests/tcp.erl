-module(tcp).
-export([start/0]).
%% gen_tcp over the loopback: listen, accept, connect, raw and line and length-framed packets,
%% active and passive modes, closing. On the real BEAM this uses real sockets.
start() ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}, {reuseaddr, true}]),
    {ok, Port} = inet:port(L),
    Self = self(),
    spawn_link(fun() -> server(L, Self) end),
    {ok, C} = gen_tcp:connect("localhost", Port, [binary, {active, false}]),
    ok = gen_tcp:send(C, <<"hello\n">>),
    R1 = gen_tcp:recv(C, 0, 5000),
    ok = inet:setopts(C, [{packet, line}]),
    ok = gen_tcp:send(C, <<"two\nlines\n">>),
    L1 = gen_tcp:recv(C, 0, 5000), L2 = gen_tcp:recv(C, 0, 5000),
    ok = inet:setopts(C, [{packet, 4}]),
    ok = gen_tcp:send(C, <<"framed">>),
    F = gen_tcp:recv(C, 0, 5000),
    ok = inet:setopts(C, [{packet, 0}, {active, once}]),
    ok = gen_tcp:send(C, <<"active">>),
    A = receive {tcp, C, D} -> D after 5000 -> timeout end,
    ok = gen_tcp:send(C, <<"bye">>),
    Closed = receive {tcp_closed, C} -> closed after 5000 -> timeout end,
    {ok, {_, PeerPort}} = inet:peername(C),
    Server = receive {server, S} -> S after 5000 -> timeout end,
    {R1, L1, L2, F, A, Closed, PeerPort =:= Port, Server,
     gen_tcp:connect("localhost", 1, [], 1000)}.

server(L, Parent) ->
    {ok, S} = gen_tcp:accept(L, 5000),
    {ok, Hello} = gen_tcp:recv(S, 6, 5000),
    ok = gen_tcp:send(S, [<<"echo ">>, Hello]),
    ok = inet:setopts(S, [{packet, line}]),
    {ok, A} = gen_tcp:recv(S, 0, 5000), {ok, B} = gen_tcp:recv(S, 0, 5000),
    ok = gen_tcp:send(S, [A, B]),
    ok = inet:setopts(S, [{packet, 4}]),
    {ok, Fr} = gen_tcp:recv(S, 0, 5000),
    ok = gen_tcp:send(S, <<Fr/binary, "!">>),
    ok = inet:setopts(S, [{packet, 0}]),
    {ok, Act} = gen_tcp:recv(S, 6, 5000),
    ok = gen_tcp:send(S, Act),
    {ok, Bye} = gen_tcp:recv(S, 3, 5000),
    gen_tcp:close(S),
    Parent ! {server, Bye}.
