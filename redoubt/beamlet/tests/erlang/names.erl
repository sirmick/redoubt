%% Sends and monitors by {Name, Node}, exit/2 followed at once by process_info, and
%% gen_server:multi_call on the local node.
-module(names).
-export([start/0]).

start() ->
    register(names_me, self()),
    {names_me, node()} ! local,
    Local = receive local -> got after 100 -> none end,
    Dropped = ({nobody_here, node()} ! x),
    Remote = ({x, 'other@host'} ! y),
    NoConn = erlang:send({x, 'other@host'}, y, [noconnect]),
    BadName = try nobody_here ! z catch error:badarg -> badarg end,
    P = spawn(fun() -> receive after infinity -> ok end end),
    register(names_victim, P),
    R1 = erlang:monitor(process, {names_victim, node()}),
    Down2 = try erlang:monitor(process, {names_victim, 'other@host'}) catch error:badarg -> badarg end,
    exit(P, kill),
    Info = process_info(P, registered_name),
    Alive = is_process_alive(P),
    Down1 = receive {'DOWN', R1, process, O1, W1} -> {O1, W1} after 100 -> none end,
    {ok, S} = gen_server:start({local, names_srv}, gen_server_echo(), [], []),
    Multi = gen_server:multi_call([node()], names_srv, ping),
    gen_server:stop(S),
    {Local, Dropped, Remote, NoConn, BadName, Down2, Info, Alive, Down1, Multi}.

gen_server_echo() -> names_echo.
