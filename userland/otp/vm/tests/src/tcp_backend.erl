%% Fixture for redoubt/tests/tcp.rs: gen_tcp over beamlet_tcp, against a scripted /net on the fake
%% kernel, and the in-VM loopback when the namespace has none. OTP's prim_file, inet and stdlib
%% come from the pinned toolchain, the wire codec from libs/wire/elixir. Each function returns
%% what the test reads from the console. Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/tcp_backend.erl
-module(tcp_backend).
-export([echo/0, active/0, quiet/0, refused/0, listen/0, standin/0]).

%% The bench's peer, which the script reaches.
-define(PEER, {10, 0, 9, 100}).

%% A connection, two sends echoed back, the peer's name, our half close, then the peer's end.
echo() ->
    {ok, S} = gen_tcp:connect(?PEER, 7, [binary, {active, false}]),
    ok = gen_tcp:send(S, <<"hello">>),
    {ok, Back} = gen_tcp:recv(S, 5),
    ok = gen_tcp:send(S, <<"again">>),
    {ok, Back2} = gen_tcp:recv(S, 0),
    {ok, Peer} = inet:peername(S),
    ok = gen_tcp:shutdown(S, write),
    Closed = gen_tcp:recv(S, 0),
    ok = gen_tcp:close(S),
    {Back, Back2, Peer, Closed}.

%% Active mode with line packets: what the peer sends arrives as messages, its end as tcp_closed.
active() ->
    {ok, S} = gen_tcp:connect(?PEER, 7, [binary, {active, true}, {packet, line}]),
    L1 = receive {tcp, S, A} -> A end,
    L2 = receive {tcp, S, B} -> B end,
    End = receive {tcp_closed, S} -> closed end,
    {L1, L2, End}.

%% A quiet peer: the read waits past the hub's bound and is asked again; what then comes arrives.
quiet() ->
    {ok, S} = gen_tcp:connect(?PEER, 7, [binary, {active, false}]),
    R = gen_tcp:recv(S, 0),
    ok = gen_tcp:close(S),
    R.

%% Refusals, each by the name the script gives it: the box's own addresses before any stack, a
%% host nobody answers at as the closed state, a name as nxdomain.
refused() ->
    Own = gen_tcp:connect({127, 0, 0, 1}, 22, [binary]),
    Self = gen_tcp:connect({10, 0, 2, 15}, 22, [binary]),
    Nobody = gen_tcp:connect({10, 0, 9, 200}, 7, [binary]),
    Name = gen_tcp:connect("example.com", 80, [binary]),
    {Own, Self, Nobody, Name}.

%% A listener: the socket the script accepts arrives with its remote end and what it sent.
listen() ->
    {ok, L} = gen_tcp:listen(8000, [binary, {active, false}, {backlog, 2}]),
    {ok, S} = gen_tcp:accept(L),
    {ok, Peer} = inet:peername(S),
    {ok, Got} = gen_tcp:recv(S, 0),
    ok = gen_tcp:send(S, Got),
    ok = gen_tcp:close(S),
    ok = gen_tcp:close(L),
    {Peer, Got}.

%% Without a /net: the loopback stand-in serves this VM's own listeners, any other address is
%% unreachable, and a name has no resolver.
standin() ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, {_, Port}} = inet:sockname(L),
    Self = self(),
    spawn(fun() ->
        {ok, A} = gen_tcp:accept(L),
        {ok, B} = gen_tcp:recv(A, 0),
        ok = gen_tcp:send(A, B),
        Self ! served
    end),
    {ok, C} = gen_tcp:connect({127, 0, 0, 1}, Port, [binary, {active, false}]),
    ok = gen_tcp:send(C, <<"loop">>),
    {ok, Back} = gen_tcp:recv(C, 0),
    receive served -> ok end,
    Far = gen_tcp:connect(?PEER, 7, [binary]),
    Name = gen_tcp:connect(localhost, 80, [binary]),
    {Back, Far, Name}.
