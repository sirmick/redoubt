%% TCP for beamlet: sockets are processes, and `gen_tcp` and `inet` reach them through OTP's
%% module-socket mechanism (a socket is `{'$inet', beamlet_tcp, Pid}`).
%%
%% This backend is a loopback network inside one VM: `listen/2` registers a port, `connect/4`
%% to a loopback address finds it. There is no other network: reaching one is the platform's
%% job (on Xous, a network server), and a VM that is not given one has none.
%%
%% Supported: binary and list modes; packet modes raw (0), line, and 1/2/4-byte length
%% headers; active true, false, once and N; recv with a length and a timeout; shutdown/close.
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(beamlet_tcp).
-export([connect/4, listen/2, accept/2, send/2, recv/3, unrecv/2, close/1, shutdown/2,
         controlling_process/2, setopts/2, getopts/2, peername/1, sockname/1, socknames/1,
         getstat/2]).

-define(SOCK(Pid), {'$inet', ?MODULE, Pid}).
-define(REGISTRY, beamlet_tcp_ports).
-define(FIRST_EPHEMERAL, 49152).

%% ---- the port registry: Port => listener, and ephemeral port numbers ----

registry() ->
    case whereis(?REGISTRY) of
        undefined ->
            %% Start it and wait until it has registered (or lost the race to another).
            Self = self(),
            Pid = spawn(fun() -> registry_init(Self) end),
            Ref = monitor(process, Pid),
            receive
                {Pid, started} -> demonitor(Ref, [flush]), Pid;
                {'DOWN', Ref, _, _, _} -> registry()
            end;
        R -> R
    end.

registry_init(Starter) ->
    try register(?REGISTRY, self()) of
        true -> Starter ! {self(), started}, registry_loop(#{}, ?FIRST_EPHEMERAL)
    catch error:badarg -> ok  % someone else won the race; the starter retries
    end.

registry_loop(Ports, Next) ->
    receive
        {From, Ref, {bind, 0, L}} ->
            {Port, Next1} = free_port(Ports, Next),
            monitor(process, L),
            From ! {Ref, {ok, Port}},
            registry_loop(Ports#{Port => L}, Next1);
        {From, Ref, {bind, Port, L}} ->
            case maps:is_key(Port, Ports) of
                true -> From ! {Ref, {error, eaddrinuse}}, registry_loop(Ports, Next);
                false -> monitor(process, L), From ! {Ref, {ok, Port}}, registry_loop(Ports#{Port => L}, Next)
            end;
        {From, Ref, {lookup, Port}} ->
            From ! {Ref, maps:find(Port, Ports)},
            registry_loop(Ports, Next);
        {From, Ref, ephemeral} ->
            {Port, Next1} = free_port(Ports, Next),
            From ! {Ref, Port},
            registry_loop(Ports, Next1);
        {'DOWN', _, process, L, _} ->
            registry_loop(maps:filter(fun(_, P) -> P =/= L end, Ports), Next)
    end.

free_port(Ports, N) when N > 65535 -> free_port(Ports, ?FIRST_EPHEMERAL);
free_port(Ports, N) ->
    case maps:is_key(N, Ports) of
        true -> free_port(Ports, N + 1);
        false -> {N, N + 1}
    end.

call(Pid, Req) -> call(Pid, Req, infinity).
call(Pid, Req, Timeout) ->
    Ref = monitor(process, Pid),
    Pid ! {self(), Ref, Req},
    receive
        {Ref, Reply} -> demonitor(Ref, [flush]), Reply;
        {'DOWN', Ref, _, _, _} -> {error, closed}
    after Timeout ->
        demonitor(Ref, [flush]), {error, timeout}
    end.

%% ---- listening and connecting ----

loopback({127, _, _, _}) -> true;
loopback({0, 0, 0, 0, 0, 0, 0, 1}) -> true;
loopback(localhost) -> true;
loopback("localhost") -> true;
loopback(_) -> false.

listen(Port, Opts) ->
    Owner = self(),
    L = spawn(fun() -> listener_init(Owner, Opts) end),
    case call(registry(), {bind, Port, L}) of
        {ok, P} -> L ! {port, P}, {ok, ?SOCK(L)};
        Error -> exit(L, kill), Error
    end.

connect(Address, Port, Opts, Timeout) ->
    case loopback(Address) of
        false -> {error, enetunreach};
        true ->
            case call(registry(), {lookup, Port}) of
                {ok, L} ->
                    Local = call(registry(), ephemeral),
                    Owner = self(),
                    S = spawn(fun() -> socket_init(Owner, Opts, Local, Port) end),
                    case call(L, {connect, S, Local}, Timeout) of
                        ok -> {ok, ?SOCK(S)};
                        Error -> exit(S, kill), Error
                    end;
                error -> {error, econnrefused}
            end
    end.

%% A listener queues incoming connections until someone accepts them.
listener_init(Owner, Opts) ->
    receive {port, Port} -> listener_loop(#{owner => Owner, opts => Opts, port => Port, queue => [], waiting => []}) end.

listener_loop(#{queue := Q, waiting := W} = St) when Q =/= [], W =/= [] ->
    [{From, Ref} | W1] = W,
    [{S, Remote} | Q1] = Q,
    From ! {Ref, {ok, S, Remote}},
    listener_loop(St#{queue := Q1, waiting := W1});
listener_loop(#{port := Port} = St) ->
    receive
        {From, Ref, {connect, Client, ClientPort}} ->
            %% The server end of the new connection, owned by whoever accepts it.
            Server = spawn(fun() -> socket_init(none, maps:get(opts, St), Port, ClientPort) end),
            Client ! {peer, Server}, Server ! {peer, Client},
            From ! {Ref, ok},
            listener_loop(St#{queue := maps:get(queue, St) ++ [{Server, ClientPort}]});
        {From, Ref, accept} ->
            listener_loop(St#{waiting := maps:get(waiting, St) ++ [{From, Ref}]});
        {From, Ref, {cancel, AcceptRef}} ->
            From ! {Ref, ok},
            listener_loop(St#{waiting := [W || {_, R} = W <- maps:get(waiting, St), R =/= AcceptRef]});
        {From, Ref, sockname} ->
            From ! {Ref, {ok, {{127, 0, 0, 1}, Port}}}, listener_loop(St);
        {From, Ref, {owner, Pid}} ->
            From ! {Ref, ok}, listener_loop(St#{owner := Pid});
        {From, Ref, close} ->
            From ! {Ref, ok},
            [From2 ! {R2, {error, closed}} || {From2, R2} <- maps:get(waiting, St)],
            [exit(S, kill) || {S, _} <- maps:get(queue, St)];
        {From, Ref, {getopts, _}} -> From ! {Ref, {ok, []}}, listener_loop(St);
        {From, Ref, _} -> From ! {Ref, ok}, listener_loop(St)
    end.

accept(?SOCK(L), Timeout) ->
    Ref = monitor(process, L),
    L ! {self(), Ref, accept},
    receive
        {Ref, {ok, S, _Remote}} ->
            demonitor(Ref, [flush]),
            call(S, {owner, self()}),
            {ok, ?SOCK(S)};
        {Ref, Error} -> demonitor(Ref, [flush]), Error;
        {'DOWN', Ref, _, _, _} -> {error, closed}
    after Timeout ->
        call(L, {cancel, Ref}),
        demonitor(Ref, [flush]),
        %% An accept that raced with the timeout: close the connection it would have returned.
        receive {Ref, {ok, S, _}} -> exit(S, kill) after 0 -> ok end,
        {error, timeout}
    end.

%% ---- connected sockets ----

-record(s, {owner, peer, local, remote,
            active = true, mode = list, packet = raw,   % gen_tcp's defaults
            buf = <<>>, closed = false, recv = none,
            sent = 0, received = 0}).

socket_init(Owner, Opts, Local, Remote) ->
    receive {peer, Peer} -> ok end,
    monitor(process, Peer),
    St = apply_opts(Opts, #s{owner = Owner, peer = Peer, local = Local, remote = Remote}),
    socket_loop(St).

apply_opts(Opts, St) -> lists:foldl(fun opt/2, St, Opts).

opt(binary, St) -> St#s{mode = binary};
opt(list, St) -> St#s{mode = list};
opt({mode, M}, St) when M =:= binary; M =:= list -> St#s{mode = M};
opt({active, A}, St) when is_boolean(A); A =:= once; is_integer(A) -> St#s{active = A};
opt({packet, P}, St) -> St#s{packet = packet_type(P)};
opt(_, St) -> St.  % tuning options (nodelay, buffers, ...) mean nothing here

packet_type(0) -> raw;
packet_type(raw) -> raw;
packet_type(line) -> line;
packet_type(N) when N =:= 1; N =:= 2; N =:= 4 -> N;
packet_type(_) -> raw.

socket_loop(St) ->
    receive
        {From, Ref, Req} -> socket_loop(deliver(handle(Req, From, Ref, St)));
        {data, Bin} ->
            socket_loop(deliver(St#s{buf = <<(St#s.buf)/binary, Bin/binary>>, received = St#s.received + byte_size(Bin)}));
        peer_closed -> socket_loop(deliver(St#s{closed = true}));
        {'DOWN', _, process, Peer, _} when Peer =:= St#s.peer -> socket_loop(deliver(St#s{closed = true}));
        {recv_timeout, Ref} -> socket_loop(recv_timeout(Ref, St));
        stop -> ok
    end.

handle({send, _}, From, Ref, #s{closed = true} = St) -> From ! {Ref, {error, closed}}, St;
handle({send, Bin}, From, Ref, St) ->
    St#s.peer ! {data, frame(Bin, St#s.packet)},
    From ! {Ref, ok},
    St#s{sent = St#s.sent + byte_size(Bin)};
handle({recv, Len, Timeout}, From, Ref, St) ->
    case Timeout of
        infinity -> ok;
        T -> erlang:send_after(T, self(), {recv_timeout, Ref})
    end,
    St#s{recv = {From, Ref, Len}};
handle({unrecv, Bin}, From, Ref, St) -> From ! {Ref, ok}, St#s{buf = <<Bin/binary, (St#s.buf)/binary>>};
handle({setopts, Opts}, From, Ref, St) -> From ! {Ref, ok}, apply_opts(Opts, St);
handle({getopts, Opts}, From, Ref, St) -> From ! {Ref, {ok, [{O, getopt(O, St)} || O <- Opts, getopt(O, St) =/= undefined]}}, St;
handle({owner, Pid}, From, Ref, St) -> From ! {Ref, ok}, St#s{owner = Pid};
handle(peername, From, Ref, St) -> From ! {Ref, {ok, {{127, 0, 0, 1}, St#s.remote}}}, St;
handle(sockname, From, Ref, St) -> From ! {Ref, {ok, {{127, 0, 0, 1}, St#s.local}}}, St;
handle({getstat, What}, From, Ref, St) ->
    Stats = #{recv_oct => St#s.received, send_oct => St#s.sent},
    From ! {Ref, {ok, [{W, maps:get(W, Stats, 0)} || W <- What]}}, St;
handle({shutdown, How}, From, Ref, St) when How =:= write; How =:= read_write ->
    St#s.peer ! peer_closed, From ! {Ref, ok}, St;
handle({shutdown, read}, From, Ref, St) -> From ! {Ref, ok}, St;
handle(close, From, Ref, St) ->
    St#s.peer ! peer_closed, From ! {Ref, ok}, self() ! stop,
    St#s{owner = none}.

getopt(active, St) -> St#s.active;
getopt(mode, St) -> St#s.mode;
getopt(packet, St) -> case St#s.packet of raw -> 0; P -> P end;
getopt(header, _) -> 0;
getopt(packet_size, _) -> 0;
getopt(deliver, _) -> term;
getopt(exit_on_close, _) -> true;
getopt(nodelay, _) -> true;
getopt(keepalive, _) -> false;
getopt(reuseaddr, _) -> true;
getopt(send_timeout, _) -> infinity;
getopt(send_timeout_close, _) -> false;
getopt(delay_send, _) -> false;
getopt(recbuf, _) -> 65536;
getopt(sndbuf, _) -> 65536;
getopt(buffer, _) -> 65536;
getopt(_, _) -> undefined.

%% Packets with a length header are framed by the sender and unframed by the receiver.
frame(Bin, N) when is_integer(N) -> <<(byte_size(Bin)):N/unit:8, Bin/binary>>;
frame(Bin, _) -> Bin.

%% The next packet in `Buf`, or `more`. `Len` only matters in raw mode (0 = whatever is there).
take(<<>>, _, _) -> more;
take(Buf, raw, 0) -> {Buf, <<>>};
take(Buf, raw, Len) when byte_size(Buf) >= Len -> <<P:Len/binary, R/binary>> = Buf, {P, R};
take(_, raw, _) -> more;
take(Buf, line, _) ->
    case binary:match(Buf, <<"\n">>) of
        {I, 1} -> <<P:(I + 1)/binary, R/binary>> = Buf, {P, R};
        nomatch -> more
    end;
take(Buf, N, _) when byte_size(Buf) >= N ->
    <<Size:N/unit:8, Rest/binary>> = Buf,
    case Rest of
        <<P:Size/binary, R/binary>> -> {P, R};
        _ -> more
    end;
take(_, _, _) -> more.

out(Bin, #s{mode = binary}) -> Bin;
out(Bin, #s{mode = list}) -> binary_to_list(Bin).

deliver(#s{recv = {From, Ref, Len}} = St) ->
    case take(St#s.buf, St#s.packet, Len) of
        {P, Rest} -> From ! {Ref, {ok, out(P, St)}}, deliver(St#s{buf = Rest, recv = none});
        more when St#s.closed -> From ! {Ref, {error, closed}}, St#s{recv = none};
        more -> St
    end;
deliver(#s{active = false} = St) -> St;
deliver(#s{owner = none} = St) -> St;
deliver(St) ->
    Sock = ?SOCK(self()),
    case take(St#s.buf, St#s.packet, 0) of
        {P, Rest} ->
            St#s.owner ! {tcp, Sock, out(P, St)},
            deliver(next_active(St#s{buf = Rest}));
        more when St#s.closed ->
            St#s.owner ! {tcp_closed, Sock},
            St#s{owner = none};
        more -> St
    end.

next_active(#s{active = true} = St) -> St;
next_active(#s{active = once} = St) -> St#s{active = false};
next_active(#s{active = 1} = St) -> St#s.owner ! {tcp_passive, ?SOCK(self())}, St#s{active = false};
next_active(#s{active = N} = St) -> St#s{active = N - 1}.

recv_timeout(Ref, #s{recv = {From, Ref, _}} = St) -> From ! {Ref, {error, timeout}}, St#s{recv = none};
recv_timeout(_, St) -> St.

%% ---- the gen_tcp and inet entry points ----

send(?SOCK(S), Data) -> call(S, {send, iolist_to_binary(Data)}).
recv(?SOCK(S), Len, Timeout) -> call(S, {recv, Len, Timeout}).
unrecv(?SOCK(S), Data) -> call(S, {unrecv, iolist_to_binary(Data)}).
close(?SOCK(S)) -> _ = call(S, close), ok.
shutdown(?SOCK(S), How) -> call(S, {shutdown, How}).
controlling_process(?SOCK(S), Pid) -> call(S, {owner, Pid}).
setopts(?SOCK(S), Opts) -> call(S, {setopts, Opts}).
getopts(?SOCK(S), Opts) -> call(S, {getopts, Opts}).
peername(?SOCK(S)) -> call(S, peername).
sockname(?SOCK(S)) -> call(S, sockname).
socknames(Sock) -> case sockname(Sock) of {ok, A} -> {ok, [A]}; E -> E end.
getstat(?SOCK(S), What) -> call(S, {getstat, What}).
