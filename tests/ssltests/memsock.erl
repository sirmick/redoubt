%% An in-memory, connected socket pair with gen_tcp's interface, for use as an ssl transport
%% (the `cb_info` option). Each socket is a process; data written to one arrives at the other.
%% Messages to the owner: {memsock, S, Data}, {memsock_closed, S}, {memsock_passive, S}.
-module(memsock).
-export([pair/0, send/2, recv/2, recv/3, setopts/2, getopts/2, controlling_process/2,
         peername/1, sockname/1, port/1, close/1, shutdown/2, getstat/2, cb_info/0]).

-record(st, {owner, peer, active = false, buf = <<>>, closed = false, waiting = none, port}).

cb_info() -> {?MODULE, memsock, memsock_closed, memsock_error, memsock_passive}.

pair() ->
    Owner = self(),
    A = spawn(fun() -> init(Owner, 1) end),
    B = spawn(fun() -> init(Owner, 2) end),
    A ! {peer, B}, B ! {peer, A},
    {A, B}.

init(Owner, Port) -> receive {peer, P} -> loop(#st{owner = Owner, peer = P, port = Port}) end.

call(S, Req) ->
    Ref = monitor(process, S),
    S ! {call, self(), Ref, Req},
    receive
        {Ref, Reply} -> demonitor(Ref, [flush]), Reply;
        {'DOWN', Ref, _, _, _} -> {error, closed}
    end.

send(S, Data) -> call(S, {send, iolist_to_binary(Data)}).
recv(S, Len) -> recv(S, Len, infinity).
recv(S, Len, Timeout) -> call(S, {recv, Len, Timeout}).
setopts(S, Opts) -> call(S, {setopts, Opts}).
getopts(S, Opts) -> call(S, {getopts, Opts}).
controlling_process(S, Pid) -> call(S, {owner, Pid}).
peername(S) -> call(S, peername).
sockname(S) -> call(S, sockname).
port(S) -> call(S, port).
close(S) -> call(S, close).
shutdown(S, _How) -> call(S, close).
getstat(_S, Opts) -> {ok, [{O, 0} || O <- Opts]}.

loop(St) ->
    receive
        {call, From, Ref, Req} -> {Reply, St1} = handle(Req, From, Ref, St),
                                  case Reply of noreply -> ok; _ -> From ! {Ref, Reply} end,
                                  loop(deliver(St1));
        {data, Bin} -> loop(deliver(St#st{buf = <<(St#st.buf)/binary, Bin/binary>>}));
        peer_closed -> loop(deliver(St#st{closed = true}));
        {timeout, Ref} -> loop(timeout(Ref, St))
    end.

handle({send, _}, _, _, #st{closed = true} = St) -> {{error, closed}, St};
handle({send, Bin}, _, _, St) -> St#st.peer ! {data, Bin}, {ok, St};
handle({recv, Len, Timeout}, From, Ref, St) ->
    case Timeout of infinity -> ok; T -> erlang:send_after(T, self(), {timeout, Ref}) end,
    {noreply, St#st{waiting = {From, Ref, Len}}};
handle({setopts, Opts}, _, _, St) ->
    Active = proplists:get_value(active, Opts, St#st.active),
    {ok, St#st{active = Active}};
handle({getopts, Opts}, _, _, St) ->
    Known = #{active => St#st.active, mode => binary, packet => raw, header => 0,
              packet_size => 0, deliver => term, exit_on_close => true, send_timeout => infinity},
    {{ok, [{O, maps:get(O, Known)} || O <- Opts, is_atom(O), maps:is_key(O, Known)]}, St};
handle({owner, Pid}, _, _, St) -> {ok, St#st{owner = Pid}};
handle(peername, _, _, St) -> {{ok, {{127, 0, 0, 1}, 3 - St#st.port}}, St};
handle(sockname, _, _, St) -> {{ok, {{127, 0, 0, 1}, St#st.port}}, St};
handle(port, _, _, St) -> {{ok, St#st.port}, St};
handle(close, _, _, St) -> St#st.peer ! peer_closed, {ok, St#st{closed = true}}.

%% Hand buffered data to a waiting recv, or to the owner in active mode.
deliver(#st{waiting = {From, Ref, Len}, buf = Buf} = St) when byte_size(Buf) > 0, Len =< byte_size(Buf) ->
    N = case Len of 0 -> byte_size(Buf); _ -> Len end,
    <<Out:N/binary, Rest/binary>> = Buf,
    From ! {Ref, {ok, Out}},
    deliver(St#st{buf = Rest, waiting = none});
deliver(#st{waiting = {From, Ref, _}, closed = true, buf = <<>>} = St) ->
    From ! {Ref, {error, closed}},
    St#st{waiting = none};
deliver(#st{active = A, buf = Buf} = St) when A =/= false, byte_size(Buf) > 0 ->
    St#st.owner ! {memsock, self(), Buf},
    deliver(St#st{buf = <<>>, active = next_active(A, St)});
deliver(#st{active = A, closed = true, buf = <<>>, owner = O} = St) when A =/= false, O =/= none ->
    O ! {memsock_closed, self()},
    St#st{owner = none};
deliver(St) -> St.

next_active(true, _) -> true;
next_active(once, _) -> false;
next_active(1, St) -> St#st.owner ! {memsock_passive, self()}, false;
next_active(N, _) when is_integer(N) -> N - 1.

timeout(Ref, #st{waiting = {From, Ref, _}} = St) -> From ! {Ref, {error, timeout}}, St#st{waiting = none};
timeout(_, St) -> St.
