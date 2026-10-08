%% Fixture for vm/tests/sized.rs: resources that declare their size, through the test's natives
%% probe:sized/1 and probe:resize/2. Only natives are used: the tests load no OTP modules.
%% Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/sized.erl
-module(sized).
-export([small/0, many/0, grown/0]).

-define(LIMIT, #{size => 100000, kill => true, error_logger => false}).

%% A few small resources within the process's limit: it ends normally.
small() -> limited(fun() -> hold(100, 10, []) end).

%% A thousand of 100 kB each, 100 MB against a limit of 100,000 words: the limit ends it.
many() -> limited(fun() -> hold(1000, 100000, []) end).

%% One resource of no size, then declared 10 MB: the limit ends it.
grown() ->
    limited(fun() ->
                    R = probe:sized(0),
                    ok = probe:resize(R, 10000000),
                    spin(1000),
                    R
            end).

limited(F) ->
    {Pid, Ref} = spawn_opt(F, [monitor, {max_heap_size, ?LIMIT}]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason end.

hold(0, _Bytes, Acc) -> length(Acc);
hold(N, Bytes, Acc) -> hold(N - 1, Bytes, [probe:sized(Bytes) | Acc]).

spin(0) -> ok;
spin(N) -> erlang:yield(), spin(N - 1).
