%% TCP for beamlet: sockets are processes, and `gen_tcp` and `inet` reach them through OTP's
%% module-socket mechanism (a socket is `{'$inet', beamlet_tcp, Pid}`).
%%
%% The transport is the session's `/net` when the namespace has one (docs/servers/ipd.md, "The
%% `/net` tree"): a socket process reads `/net/tcp/clone` for its number, writes `connect` to its
%% `ctl` in the wire encoding (the generated codec, never a frame of its own), reads `ctl` until
%% the state is established, and moves bytes through `data`: a reader process per socket for
%% what arrives, writes from the socket process. Every socket is `ipd`'s then, loopback addresses
%% included: `ipd` refuses the box's own addresses (R59) and a labelled caller (R60), and the
%% refusal comes back by its name (`eacces`); a connect the stack could not complete reads back
%% as the closed state, `econnrefused`. The VM's hub bounds each request at `FILE_WAIT_US`
%% (docs/userland/files.md): `etimedout` on a wait is asked again, 0 bytes is the peer's end.
%%
%% Without a `/net` (a lease, a host test, a VM on the fake kernel) the loopback network inside
%% one VM stands in: `listen/2` registers a port, `connect/4` to a loopback address finds it, and
%% any other address is `enetunreach`: a lease has no network (docs/userland/agents.md).
%%
%% Addresses are IPv4 tuples; a name is `nxdomain` until the resolver (ipd.md, "Name-scoped
%% connections"). Supported: binary and list modes; packet modes raw (0), line, and 1/2/4-byte
%% length headers; active true, false, once and N; recv with a length and a timeout;
%% shutdown/close. Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(beamlet_tcp).
-export([connect/4, listen/2, accept/2, send/2, recv/3, unrecv/2, close/1, shutdown/2,
         controlling_process/2, setopts/2, getopts/2, peername/1, sockname/1, socknames/1,
         getstat/2]).

-define(SOCK(Pid), {'$inet', ?MODULE, Pid}).
-define(REGISTRY, beamlet_tcp_ports).
-define(FIRST_EPHEMERAL, 49152).

%% The `/net` tree and the codec of what its `ctl` files take (libs/wire/tables/net_ctl.md).
-define(NET, "/net/tcp").
-define(CODEC, 'Elixir.Redoubt.Wire.Proto.NetCtl').
%% A socket's states as `ctl` reads them.
-define(ESTABLISHED, 2).
-define(CLOSED, 4).
-define(LISTENING, 5).
%% What one read of `data` asks for: the most one 9P answer carries.
-define(READ_SIZE, 65000).
-define(DEFAULT_BACKLOG, 5).
-define(MAX_BACKLOG, 8).

%% ---- the port registry of the stand-in: Port => listener, and ephemeral port numbers ----

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

%% ---- addresses ----

%% An IPv4 tuple as the four bytes `ipd` takes; a name has no resolver yet.
address({A, B, C, D}) when is_integer(A), is_integer(B), is_integer(C), is_integer(D),
                           (A bor B bor C bor D) band bnot 255 =:= 0 ->
    {ok, <<A, B, C, D>>};
address(T) when is_tuple(T), tuple_size(T) =:= 8 -> {error, eafnosupport};
address(Name) when is_atom(Name); is_list(Name); is_binary(Name) -> {error, nxdomain};
address(_) -> {error, einval}.

loopback(<<127, _, _, _>>) -> true;
loopback(_) -> false.

%% ---- the `/net` files ----

path(Parts) -> unicode:characters_to_binary(lists:concat([?NET | Parts])).

%% A new socket's number, or `no_net` when the namespace holds no `/net`.
clone() ->
    case prim_file:open(path(["/clone"]), [read, binary, raw]) of
        {ok, Fd} ->
            Read = prim_file:pread(Fd, 0, 4),
            prim_file:close(Fd),
            case Read of
                {ok, <<N:32/little>>} -> {ok, N};
                {error, _} = E -> E;
                _ -> {error, eio}
            end;
        {error, enoent} -> no_net;
        {error, _} = E -> E
    end.

open_ctl(N) -> prim_file:open(path(["/", N, "/ctl"]), [read, write, binary, raw]).
open_data(N, Modes) -> prim_file:open(path(["/", N, "/data"]), Modes ++ [binary, raw]).

%% One `net_ctl` operation written to a socket's `ctl`.
ctl(Fd, Op, Fields) ->
    case ?CODEC:encode_file({Op, Fields}) of
        {ok, Bytes} -> prim_file:write(Fd, Bytes);
        {error, _} -> {error, einval}
    end.

%% The socket's state and the number it reports: one read, which `ipd` holds while a connect is
%% in progress or a listener has nothing accepted, and the hub gives up at its bound
%% (`etimedout`), for the caller to ask again.
state(Fd) ->
    case prim_file:pread(Fd, 0, 8) of
        {ok, <<State:32/little, M:32/little>>} -> {ok, State, M};
        {error, _} = E -> E;
        _ -> {error, eio}
    end.

%% Ends socket `N` at `ipd` when its process is gone: the reader's duty, through a ctl of its own.
abort(N) ->
    case open_ctl(N) of
        {ok, Fd} -> _ = ctl(Fd, abort, #{}), prim_file:close(Fd);
        _ -> ok
    end.

%% The remote end of an accepted socket: `addr: bytes[4]`, `port: u16`.
remote(N) ->
    case prim_file:open(path(["/", N, "/remote"]), [read, binary, raw]) of
        {ok, Fd} ->
            Read = prim_file:pread(Fd, 0, 6),
            prim_file:close(Fd),
            case Read of
                {ok, <<A, B, C, D, Port:16/little>>} -> {{A, B, C, D}, Port};
                _ -> {{0, 0, 0, 0}, 0}
            end;
        _ -> {{0, 0, 0, 0}, 0}
    end.

%% ---- listening and connecting ----

listen(Port, Opts) ->
    Owner = self(),
    L = spawn(fun() -> listener_init(Owner, Opts) end),
    case call(L, {listen, Port}) of
        ok -> {ok, ?SOCK(L)};
        Error -> Error
    end.

connect(Address, Port, Opts, Timeout) ->
    case address(Address) of
        {ok, Addr} ->
            Owner = self(),
            S = spawn(fun() -> net_socket_init(Owner, Opts, Addr, Port) end),
            case call(S, connect, Timeout) of
                ok -> {ok, ?SOCK(S)};
                no_net -> loop_connect(Addr, Port, Opts, Timeout);
                {error, timeout} = E -> exit(S, kill), E;
                Error -> Error
            end;
        Error -> Error
    end.

%% The stand-in: a loopback address finds a listener of this VM; there is no other network.
loop_connect(Addr, Port, Opts, Timeout) ->
    case loopback(Addr) of
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

%% A listener takes its port at `ipd` if the namespace has a `/net`, in the registry otherwise,
%% then queues incoming connections until someone accepts them.
listener_init(Owner, Opts) ->
    receive
        {From, Ref, {listen, Port}} ->
            St = #{owner => Owner, opts => Opts, port => Port, queue => [], waiting => []},
            case clone() of
                no_net ->
                    case call(registry(), {bind, Port, self()}) of
                        {ok, P} -> From ! {Ref, ok}, listener_loop(St#{port := P});
                        Error -> From ! {Ref, Error}
                    end;
                {ok, N} ->
                    case net_listen(N, Port, backlog(Opts)) of
                        {ok, Fd} -> From ! {Ref, ok}, net_listener_loop(St#{n => N, ctl => Fd});
                        Error -> From ! {Ref, Error}
                    end;
                Error ->
                    From ! {Ref, Error}
            end
    end.

backlog(Opts) ->
    case proplists:get_value(backlog, Opts, ?DEFAULT_BACKLOG) of
        B when is_integer(B), B > 0 -> min(B, ?MAX_BACKLOG);
        _ -> ?DEFAULT_BACKLOG
    end.

%% `listen(port, backlog)` on socket `N`'s ctl, which `ipd` allows as the scope's listen rule
%% does; a refusal keeps its name, and a socket not listening is given back.
net_listen(N, Port, Backlog) ->
    case open_ctl(N) of
        {ok, Fd} ->
            case ctl(Fd, listen, #{port => Port, backlog => Backlog}) of
                ok -> {ok, Fd};
                Error -> _ = ctl(Fd, abort, #{}), prim_file:close(Fd), Error
            end;
        Error -> Error
    end.

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
            listener_loop(St#{queue := maps:get(queue, St) ++ [{Server, {{127, 0, 0, 1}, ClientPort}}]});
        {From, Ref, Req} ->
            case listener_request(Req, From, Ref, St) of
                closed -> ok;
                St1 -> listener_loop(St1)
            end
    end.

%% A listener over `/net`: while someone waits to accept and nothing is queued, it asks `ctl`,
%% which `ipd` answers with the next accepted socket's number; messages wait for that answer, at
%% most the hub's bound each.
net_listener_loop(#{queue := Q, waiting := W} = St) when Q =/= [], W =/= [] ->
    [{From, Ref} | W1] = W,
    [{S, Remote} | Q1] = Q,
    From ! {Ref, {ok, S, Remote}},
    net_listener_loop(St#{queue := Q1, waiting := W1});
net_listener_loop(#{waiting := []} = St) ->
    receive {From, Ref, Req} -> net_listener_step(Req, From, Ref, St) end;
net_listener_loop(#{waiting := W, ctl := Fd} = St) ->
    receive
        {From, Ref, Req} -> net_listener_step(Req, From, Ref, St)
    after 0 ->
        case state(Fd) of
            {ok, ?LISTENING, M} when M =/= 0 ->
                Opts = maps:get(opts, St),
                S = spawn(fun() -> net_accepted_init(Opts, M) end),
                case call(S, remote) of
                    {ok, Remote} -> net_listener_loop(St#{queue := maps:get(queue, St) ++ [{S, Remote}]});
                    _ -> net_listener_loop(St)
                end;
            {ok, _, _} ->
                net_listener_loop(St);
            {error, etimedout} ->
                net_listener_loop(St);
            {error, _} = Error ->
                [F ! {R, Error} || {F, R} <- W],
                net_listener_loop(St#{waiting := []})
        end
    end.

%% A close ends the listener at `ipd` before its caller is answered, so a VM that ends on the
%% answer leaves nothing behind.
net_listener_step(close, From, Ref, #{ctl := Fd} = St) ->
    _ = ctl(Fd, close, #{}),
    prim_file:close(Fd),
    closed = listener_request(close, From, Ref, St),
    ok;
net_listener_step(Req, From, Ref, St) ->
    case listener_request(Req, From, Ref, St) of
        closed -> ok;
        St1 -> net_listener_loop(St1)
    end.

%% What either listener does for its callers; `closed` ends it.
listener_request(accept, From, Ref, St) ->
    St#{waiting := maps:get(waiting, St) ++ [{From, Ref}]};
listener_request({cancel, AcceptRef}, From, Ref, St) ->
    From ! {Ref, ok},
    St#{waiting := [W || {_, R} = W <- maps:get(waiting, St), R =/= AcceptRef]};
listener_request(sockname, From, Ref, #{port := Port} = St) ->
    From ! {Ref, {ok, {{0, 0, 0, 0}, Port}}}, St;
listener_request({owner, Pid}, From, Ref, St) ->
    From ! {Ref, ok}, St#{owner := Pid};
listener_request(close, From, Ref, St) ->
    From ! {Ref, ok},
    [From2 ! {R2, {error, closed}} || {From2, R2} <- maps:get(waiting, St)],
    [exit(S, kill) || {S, _} <- maps:get(queue, St)],
    closed;
listener_request({getopts, _}, From, Ref, St) ->
    From ! {Ref, {ok, []}}, St;
listener_request(_, From, Ref, St) ->
    From ! {Ref, ok}, St.

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

%% `peer` is the other end: the stand-in's socket process, or `{net, N, Ctl, Data, Reader}` for
%% a socket of `ipd`'s.
-record(s, {owner, peer, local, remote,
            active = true, mode = list, packet = raw,   % gen_tcp's defaults
            buf = <<>>, closed = false, recv = none,
            shut = false,   % our writes are ended: `shutdown(write)` said so, `close` need not
            sent = 0, received = 0}).

socket_init(Owner, Opts, Local, Remote) ->
    receive {peer, Peer} -> ok end,
    monitor(process, Peer),
    St = apply_opts(Opts, #s{owner = Owner, peer = Peer, local = Local, remote = {{127, 0, 0, 1}, Remote}}),
    socket_loop(St).

%% A connecting socket of `ipd`'s: its number from `clone`, `connect` on its ctl, then the state
%% read until established; the first message is its owner's `connect`, answered with the outcome.
net_socket_init(Owner, Opts, Addr, Port) ->
    receive
        {From, Ref, connect} ->
            case clone() of
                {ok, N} ->
                    case net_connect(Opts, N, Addr, Port) of
                        {ok, St} ->
                            <<A, B, C, D>> = Addr,
                            From ! {Ref, ok},
                            socket_loop(St#s{owner = Owner, remote = {{A, B, C, D}, Port}});
                        Error ->
                            From ! {Ref, Error}
                    end;
                Outcome ->
                    From ! {Ref, Outcome}
            end
    end.

%% Socket `N`, fresh from `clone`: its reader is started first, so that a connect killed at its
%% timeout still has its socket aborted; then `connect` on its ctl and the state read until
%% established. `ipd` refuses by name before the stack sees it (the box's own addresses, a
%% labelled caller, the scope); a connect the stack could not complete reads back closed.
net_connect(Opts, N, Addr, Port) ->
    Reader = start_reader(N),
    case open_ctl(N) of
        {ok, Ctl} ->
            case ctl(Ctl, connect, #{addr => Addr, port => Port}) of
                ok ->
                    case established(Ctl) of
                        ok -> net_open(Opts, N, Ctl, Reader);
                        Error -> give_back(Ctl, Reader), Error
                    end;
                Error ->
                    give_back(Ctl, Reader), Error
            end;
        Error ->
            Reader ! stop, _ = abort(N), Error
    end.

established(Ctl) ->
    case state(Ctl) of
        {ok, ?ESTABLISHED, _} -> ok;
        {error, etimedout} -> established(Ctl);
        {ok, _, _} -> {error, econnrefused};
        Error -> Error
    end.

%% A socket that will not connect goes back to `ipd` whole: aborted, its ctl closed, its reader
%% told to stop.
give_back(Ctl, Reader) ->
    _ = ctl(Ctl, abort, #{}),
    prim_file:close(Ctl),
    Reader ! stop.

%% An accepted socket of `ipd`'s, number `M`: its first message asks for its remote end.
net_accepted_init(Opts, M) ->
    receive
        {From, Ref, remote} ->
            Reader = start_reader(M),
            case open_ctl(M) of
                {ok, Ctl} ->
                    case net_open(Opts, M, Ctl, Reader) of
                        {ok, St} ->
                            Remote = remote(M),
                            From ! {Ref, {ok, Remote}},
                            socket_loop(St#s{owner = none, remote = Remote});
                        Error ->
                            From ! {Ref, Error}
                    end;
                Error ->
                    Reader ! stop, _ = abort(M), From ! {Ref, Error}
            end
    end.

%% The data file for writes, and the reader told to read: the socket is established.
net_open(Opts, N, Ctl, Reader) ->
    case open_data(N, [read, write]) of
        {ok, Data} ->
            Reader ! go,
            {ok, apply_opts(Opts, #s{peer = {net, N, Ctl, Data, Reader}})};
        Error ->
            give_back(Ctl, Reader), Error
    end.

start_reader(N) ->
    Self = self(),
    Reader = spawn(fun() -> reader(Self, N) end),
    monitor(process, Reader),
    Reader.

%% Reads socket `N`'s data for its socket process `S`, once told the socket is established: each
%% read waits at `ipd` until something arrives, the hub's `etimedout` is asked again, 0 bytes is
%% the peer's end. Then, and while it waits to begin, it watches `S`: a socket process that dies
%% aborts its socket; one that closed, or gave the socket back, says `stop`.
reader(S, N) ->
    Ref = monitor(process, S),
    receive
        go ->
            case open_data(N, [read]) of
                {ok, Fd} -> read_loop(S, N, Ref, Fd);
                {error, _} -> S ! peer_closed, watch(N, Ref)
            end;
        stop -> ok;
        {'DOWN', Ref, _, _, _} -> abort(N)
    end.

read_loop(S, N, Ref, Fd) ->
    receive
        {'DOWN', Ref, _, _, _} -> prim_file:close(Fd), abort(N);
        stop -> prim_file:close(Fd)
    after 0 ->
        case prim_file:read(Fd, ?READ_SIZE) of
            {ok, Bin} -> S ! {data, Bin}, read_loop(S, N, Ref, Fd);
            {error, etimedout} -> read_loop(S, N, Ref, Fd);
            _ -> prim_file:close(Fd), S ! peer_closed, watch(N, Ref)
        end
    end.

watch(N, Ref) ->
    receive
        {'DOWN', Ref, _, _, _} -> abort(N);
        stop -> ok
    end.

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
        {'DOWN', _, process, Reader, _} when Reader =:= element(5, St#s.peer) ->
            socket_loop(deliver(St#s{closed = true}));
        {recv_timeout, Ref} -> socket_loop(recv_timeout(Ref, St));
        stop -> ok
    end.

handle({send, _}, From, Ref, #s{closed = true} = St) -> From ! {Ref, {error, closed}}, St;
handle({send, Bin}, From, Ref, St) ->
    case transmit(frame(Bin, St#s.packet), St#s.peer) of
        ok -> From ! {Ref, ok}, St#s{sent = St#s.sent + byte_size(Bin)};
        Error -> From ! {Ref, Error}, St
    end;
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
handle(peername, From, Ref, St) -> From ! {Ref, {ok, St#s.remote}}, St;
handle(sockname, From, Ref, #s{peer = {net, _, _, _, _}} = St) ->
    %% `ipd` does not tell a socket its own address and port, and nothing is invented.
    From ! {Ref, {error, enotsup}}, St;
handle(sockname, From, Ref, St) -> From ! {Ref, {ok, {{127, 0, 0, 1}, St#s.local}}}, St;
handle({getstat, What}, From, Ref, St) ->
    Stats = #{recv_oct => St#s.received, send_oct => St#s.sent},
    From ! {Ref, {ok, [{W, maps:get(W, Stats, 0)} || W <- What]}}, St;
handle({shutdown, How}, From, Ref, St) when How =:= write; How =:= read_write ->
    From ! {Ref, ok}, end_writes(St);
handle({shutdown, read}, From, Ref, St) -> From ! {Ref, ok}, St;
handle(close, From, Ref, St) ->
    St1 = end_writes(St),
    release(St1#s.peer),
    From ! {Ref, ok}, self() ! stop,
    St1#s{owner = none}.

%% Bytes to the other end: the stand-in's peer process, or `ipd`'s `data`, where a write waits
%% while the send buffer is full and the hub's `etimedout` is asked again.
transmit(Bin, {net, _, _, Data, _}) -> write_all(Data, Bin);
transmit(Bin, Peer) -> Peer ! {data, Bin}, ok.

write_all(Fd, Bin) ->
    case prim_file:write(Fd, Bin) of
        {error, etimedout} -> write_all(Fd, Bin);
        Outcome -> Outcome
    end.

%% The graceful end of this side's writes, once: `ctl close` at `ipd`, `peer_closed` to a
%% stand-in peer.
end_writes(#s{shut = true} = St) -> St;
end_writes(#s{peer = {net, _, Ctl, _, _}} = St) -> _ = ctl(Ctl, close, #{}), St#s{shut = true};
end_writes(#s{peer = Peer} = St) -> Peer ! peer_closed, St#s{shut = true}.

%% What a closed socket of `ipd`'s gives back: its files, and its reader told the close was ours.
release({net, _, Ctl, Data, Reader}) ->
    Reader ! stop,
    prim_file:close(Data),
    prim_file:close(Ctl);
release(_) -> ok.

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
