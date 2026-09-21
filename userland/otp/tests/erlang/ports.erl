%% Ports to programs: open_port/2 (spawn, spawn_executable), the messages a port sends and
%% takes, framing, exit status, port_info, links and monitors, and os:cmd/1.
-module(ports).
-export([start/0]).

start() ->
    [spawned(), executable(), packets(), lines(), eof(), commands(), errors(),
     monitors(), links(), info(), cmd()].

%% Everything a port sends until it closes (or 3 s pass). BEAM may deliver the exit status and
%% eof before the last output, so those two are put last, in a fixed order.
collect(P) -> collect(P, [], 150).
collect(_P, Acc, 0) -> finish([timeout | Acc]);
collect(P, Acc, N) ->
    receive
        {P, {data, D}} -> collect(P, [D | Acc], N);
        {P, M} -> collect(P, [M | Acc], N);
        {'EXIT', P, R} -> finish([{exit, R} | Acc])
    after 20 ->
        case erlang:port_info(P) of
            undefined -> finish(Acc);
            _ -> collect(P, Acc, N - 1)
        end
    end.
finish(Acc) ->
    Last = fun(eof) -> true; ({exit_status, _}) -> true; (_) -> false end,
    {End, Data} = lists:partition(Last, lists:reverse(Acc)),
    Data ++ lists:sort(End).

spawned() ->
    P = open_port({spawn, "echo hello"}, [exit_status]),
    A = collect(P),
    Q = open_port({spawn, "sh -c 'exit 7'"}, [exit_status, binary]),
    B = collect(Q),
    R = open_port({spawn, "sh -c 'echo out; echo err 1>&2'"}, [stderr_to_stdout, binary, exit_status, {line, 10}]),
    C = lists:sort(collect(R)),
    E = open_port({spawn, "sh -c 'echo $FOO; echo ${HOMEX-unset}'"}, [{env, [{"FOO", "bar"}, {"HOMEX", false}]}, exit_status]),
    D = collect(E),
    F = open_port({spawn, "pwd -P"}, [{cd, "/usr"}, exit_status]),
    {A, B, C, D, collect(F)}.

executable() ->
    P = open_port({spawn_executable, "/bin/echo"}, [{args, ["a b", <<"c">>]}, binary, exit_status]),
    A = collect(P),
    Q = open_port({spawn_executable, "/bin/sh"}, [{args, ["-c", "echo $0"]}, {arg0, "zero"}, exit_status]),
    {A, collect(Q)}.

packets() ->
    P = open_port({spawn_executable, "/bin/cat"}, [{packet, 2}, binary]),
    port_command(P, <<"one">>),
    port_command(P, ["t", [<<"w">>], $o]),
    A = [receive {P, {data, D}} -> D after 3000 -> timeout end || _ <- [1, 2]],
    port_close(P),
    A.

lines() ->
    P = open_port({spawn, "printf 'short\\nthis is a long line\\nend'"}, [{line, 8}, exit_status]),
    collect(P).

eof() ->
    P = open_port({spawn, "echo x"}, [eof, exit_status, binary]),
    A = collect(P, [], 10),
    B = erlang:port_info(P, connected) =:= {connected, self()},
    port_close(P),
    {A, B, erlang:port_info(P)}.

commands() ->
    P = open_port({spawn_executable, "/bin/cat"}, [binary]),
    P ! {self(), {command, <<"via message">>}},
    A = receive {P, {data, D}} -> D after 3000 -> timeout end,
    P ! {self(), close},
    B = receive {P, closed} -> closed after 3000 -> timeout end,
    {A, B, erlang:port_info(P)}.

errors() ->
    Try = fun(F) -> try F() catch C:R -> {C, R} end end,
    [Try(fun() -> open_port({spawn_executable, "/no/such/file"}, []) end),
     Try(fun() -> open_port({spawn, "echo"}, [nonsense]) end),
     Try(fun() -> open_port({spawn, "echo"}, [{packet, 3}]) end),
     Try(fun() -> open_port(nonsense, []) end),
     Try(fun() -> open_port({spawn_driver, "efile"}, []) end),
     Try(fun() -> port_command(self(), <<"x">>) end),
     Try(fun() -> port_close(list_to_port("#Port<0.99999>")) end),
     Try(fun() -> erlang:port_info(self()) end)].

monitors() ->
    P = open_port({spawn_executable, "/bin/cat"}, []),
    Ref = erlang:monitor(port, P),
    A = try erlang:monitor(process, P) catch error:badarg -> badarg end,
    port_close(P),
    B = receive {'DOWN', Ref, Type, P, Reason} -> {down, Type, Reason} after 3000 -> timeout end,
    Ref2 = erlang:monitor(port, P),
    C = receive {'DOWN', Ref2, port, P, R2} -> R2 after 3000 -> timeout end,
    {A, B, C}.

links() ->
    process_flag(trap_exit, true),
    P = open_port({spawn_executable, "/bin/cat"}, []),
    {links, [Me]} = erlang:port_info(P, links),
    A = Me =:= self(),
    port_close(P),
    B = receive {'EXIT', P, R} -> R after 3000 -> timeout end,
    %% A port whose owner exits closes.
    Self = self(),
    Owner = spawn(fun() -> Q = open_port({spawn_executable, "/bin/cat"}, []), Self ! {port, Q}, receive stop -> ok end end),
    Q = receive {port, Q0} -> Q0 end,
    M = erlang:monitor(port, Q),
    Owner ! stop,
    C = receive {'DOWN', M, port, Q, R2} -> R2 after 3000 -> timeout end,
    %% exit/2 with a reason other than normal ends it.
    S = open_port({spawn_executable, "/bin/cat"}, []),
    exit(S, bye),
    D = receive {'EXIT', S, R3} -> R3 after 3000 -> timeout end,
    process_flag(trap_exit, false),
    {A, B, C, D}.

info() ->
    P = open_port({spawn_executable, "/bin/cat"}, [binary]),
    true = register(my_port, P),
    port_command(my_port, <<"12345">>),
    receive {P, {data, _}} -> ok after 3000 -> timeout end,
    Items = [I || {I, _} <- erlang:port_info(P)],
    Vals = [erlang:port_info(P, I) || I <- [name, input, output, registered_name]],
    Connected = erlang:port_info(P, connected) =:= {connected, self()},
    A = lists:member(P, erlang:ports()),
    L = erlang:port_to_list(P),
    B = list_to_port(L) =:= P,
    port_close(P),
    {Items -- [os_pid], lists:member(os_pid, Items), Vals, Connected, A, B, is_port(P), is_pid(P),
     lists:member(P, erlang:ports()), whereis(my_port)}.

cmd() ->
    [os:cmd("echo one; echo two"), os:cmd("exit 3"), os:cmd("printf 'caf\\303\\251'"),
     try os:cmd("false", #{exception_on_failure => true}) catch C:R -> {C, R} end, os:cmd("yes | head -c 100000", #{max_size => 10})].
