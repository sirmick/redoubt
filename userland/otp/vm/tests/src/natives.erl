%% Fixture for vm/tests/system.rs: the system's natives, module `redoubt`, over a test platform's
%% `System`. Only natives are used: the tests load no OTP modules. Rebuild:
%% erlc +deterministic -o vm/tests/fixtures vm/tests/src/natives.erl
-module(natives).
-export([lookup/0, bind/0, table/0, call/0, send/0, budgets/0, labels/0, identity/0, launch/0, serve/0,
         decoded/0, wrong_types/0, oversize/0, wrong_kind/0, dropped/0, unsupported/0]).

%% The longest prefix's connection and the rest; a named handle; refusals by name.
lookup() ->
    {ok, H, Rest} = redoubt:ns_lookup(<<"/home/alice/notes.txt">>),
    {ok, B, <<>>} = redoubt:ns_lookup(<<"budget">>),
    {is_reference(H), Rest, is_reference(B),
     redoubt:ns_lookup(<<"/nowhere">>), redoubt:ns_lookup(<<"/home/../etc">>)}.

%% A bind puts a connection the VM holds at another prefix.
bind() ->
    {ok, H, _} = redoubt:ns_lookup(<<"/home/alice">>),
    {ok, B, _} = redoubt:ns_lookup(<<"budget">>),
    {redoubt:bind(<<"/h">>, H), redoubt:bind(<<"/b">>, B)}.

%% The table: path, name and handle.
table() -> [{Path, Name, is_reference(H)} || {Path, Name, H} <- redoubt:ns()].

%% A call's reply arrives as a message, its handles new resources.
call() ->
    {ok, K, _} = redoubt:ns_lookup(<<"keyd">>),
    {ok, Ref} = redoubt:call(K, {[3, 0, 0, 0], <<"ask">>, [K]}, 1000),
    {ok, Ref2} = redoubt:call(K, {[4, 0, 0, 0], nil, []}, 0),
    R1 = receive {reply, Ref, R} -> R after 1000 -> none end,
    R2 = receive {reply, Ref2, Q} -> Q after 1000 -> none end,
    {ok, {Words, Buffer, [Got]}} = R1,
    {Words, Buffer, is_reference(Got), R2}.

send() ->
    {ok, K, _} = redoubt:ns_lookup(<<"keyd">>),
    redoubt:send(K, {[9, 1, 2, 3], nil, []}).

%% Carve, read and destroy a budget.
budgets() ->
    {ok, B} = redoubt:budget_create(#{pages => 64, processes => 1, weight => 10, deadline => 5000}),
    {ok, U} = redoubt:budget_usage(B),
    {U, redoubt:budget_destroy(B), redoubt:budget_create(#{pages => 1, processes => 1, weight => 1, labels => [7]})}.

%% Fixed: asked twice, the same.
labels() -> {redoubt:labels(), redoubt:labels()}.

%% What the session was told of itself, as the platform has it.
identity() -> redoubt:identity().

%% A launch, and its end as a message naming the job.
launch() ->
    {ok, B} = redoubt:budget_create(#{pages => 64, processes => 1, weight => 10}),
    {ok, H, _} = redoubt:ns_lookup(<<"/home/alice">>),
    {ok, Job} = redoubt:launch(#{image => <<"ELF">>, budget => B, namespace => [{<<"/data">>, H}],
                                handles => [{<<"budget">>, B}], args => [<<"-v">>]}),
    receive {exit, Job, Cause, Code} -> {is_reference(Job), Cause, Code} after 1000 -> none end.

%% Requests on a served endpoint arrive as messages; a reply answers one.
serve() ->
    {ok, E, _} = redoubt:ns_lookup(<<"service">>),
    ok = redoubt:serve(E),
    receive
        {request, Req, Badge, Account, Labels, {Words, Buffer, Handles}} ->
            {Badge, Account, Labels, Words, Buffer, length(Handles),
             redoubt:reply(Req, {[0, 0, 0, 0], nil, []})}
    after 1000 -> none
    end.

%% A handle written out and read back is a plain reference: no native takes it.
decoded() ->
    {ok, H, _} = redoubt:ns_lookup(<<"/home/alice">>),
    Copy = binary_to_term(term_to_binary(H)),
    {Copy =:= H, is_reference(Copy),
     t(fun() -> redoubt:bind(<<"/c">>, Copy) end),
     t(fun() -> redoubt:call(Copy, {[1, 0, 0, 0], nil, []}, 0) end),
     t(fun() -> redoubt:budget_destroy(Copy) end),
     t(fun() -> redoubt:bind(<<"/r">>, make_ref()) end)}.

%% Every argument of the wrong type is badarg.
wrong_types() ->
    {ok, H, _} = redoubt:ns_lookup(<<"/home/alice">>),
    [t(fun() -> redoubt:ns_lookup("/home/alice") end),
     t(fun() -> redoubt:ns_lookup(<<"/home/al", 0, "ice">>) end),
     t(fun() -> redoubt:ns_lookup(<<255, 254>>) end),
     t(fun() -> redoubt:bind(<<"/h">>, self()) end),
     t(fun() -> redoubt:call(H, {[1, 0, 0], nil, []}, 0) end),
     t(fun() -> redoubt:call(H, {[1, 0, 0, -1], nil, []}, 0) end),
     t(fun() -> redoubt:call(H, {[1, 0, 0, 0], "text", []}, 0) end),
     t(fun() -> redoubt:call(H, {[1, 0, 0, 0], nil, [self()]}, 0) end),
     t(fun() -> redoubt:call(H, {[1, 0, 0, 0], nil, []}, 5001) end),
     t(fun() -> redoubt:send(H, [1, 0, 0, 0]) end),
     t(fun() -> redoubt:budget_create(#{pages => 1, processes => 1}) end),
     t(fun() -> redoubt:budget_create(#{pages => 1, processes => 1, weight => 1, colour => red}) end),
     t(fun() -> redoubt:budget_create(#{pages => -1, processes => 1, weight => 1}) end),
     t(fun() -> redoubt:budget_create([]) end),
     t(fun() -> redoubt:launch(#{budget => H}) end),
     t(fun() -> redoubt:launch(#{image => <<>>, budget => H, args => [<<"a", 0>>]}) end)].

%% Lists past their caps are refused without being walked: words, handles, labels, entries.
oversize() ->
    {ok, H, _} = redoubt:ns_lookup(<<"/home/alice">>),
    Long = lists_seq(1, 100000),
    Many = [{<<"/p">>, H} || _ <- lists_seq(1, 129)],
    [t(fun() -> redoubt:call(H, {[1, 0, 0, 0, 0], nil, []}, 0) end),
     t(fun() -> redoubt:call(H, {[1, 0, 0, 0], nil, [H, H, H, H, H]}, 0) end),
     t(fun() -> redoubt:call(H, {Long, nil, []}, 0) end),
     t(fun() -> redoubt:call(H, {[1, 0, 0, 0], binary:copy(<<0>>, 65537), []}, 0) end),
     t(fun() -> redoubt:budget_create(#{pages => 1, processes => 1, weight => 1, labels => lists_seq(1, 17)}) end),
     t(fun() -> redoubt:launch(#{image => <<"ELF">>, budget => H, namespace => Many}) end),
     t(fun() -> redoubt:launch(#{image => <<"ELF">>, budget => H, namespace => lists_seq(1, 64) ++ improper}) end),
     t(fun() -> redoubt:launch(#{image => <<"ELF">>, budget => H,
                                 namespace => [{<<"/p">>, H} || _ <- lists_seq(1, 64)],
                                 handles => [{<<"h">>, H} || _ <- lists_seq(1, 65)]}) end)].

%% A handle of the wrong kind is refused by the platform before any kernel call.
wrong_kind() ->
    {ok, H, _} = redoubt:ns_lookup(<<"/home/alice">>),
    {ok, B, _} = redoubt:ns_lookup(<<"budget">>),
    {ok, Ref} = redoubt:call(B, {[1, 0, 0, 0], nil, []}, 0),
    Called = receive {reply, Ref, R} -> R after 1000 -> none end,
    {redoubt:budget_usage(H), redoubt:budget_destroy(H), Called, redoubt:bind(<<"/x">>, B)}.

%% A handle no process holds is closed at the next collection.
dropped() ->
    Self = self(),
    P = spawn(fun() -> {ok, _, _} = redoubt:ns_lookup(<<"keyd">>), Self ! looked, receive go -> ok end end),
    receive looked -> ok end,
    {ok, _, Before} = redoubt:ns_lookup(<<"drops">>),
    Ref = monitor(process, P),
    P ! go,
    receive {'DOWN', Ref, process, P, _} -> ok end,
    {ok, _, After} = redoubt:ns_lookup(<<"drops">>),
    {Before, After}.

%% A platform with no system calls answers every one by name.
unsupported() ->
    {redoubt:ns_lookup(<<"/x">>), redoubt:ns(), redoubt:labels()}.

%% What `F` returns, or the error it raised, without its stack.
t(F) -> try F() catch error:Reason -> {raised, Reason} end.

lists_seq(N, M) when N > M -> [];
lists_seq(N, M) -> [N | lists_seq(N + 1, M)].
