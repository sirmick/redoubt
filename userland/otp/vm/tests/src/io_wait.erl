%% Fixture for vm/tests/io_wait.rs: file operations the platform finishes later, each waited for
%% by the process that asked, which gets its own answer. Only natives are used: the tests load no
%% OTP modules. Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/io_wait.erl
-module(io_wait).
-export([two/0, local/0, killed/0, messages/0]).

%% Two processes read at once; each answer reaches the one that asked.
two() ->
    Self = self(),
    spawn(fun() -> Self ! {a, prim_file:read_file_nif(<<"/a">>)} end),
    spawn(fun() -> Self ! {b, prim_file:read_file_nif(<<"/b">>)} end),
    A = receive {a, RA} -> RA end,
    B = receive {b, RB} -> RB end,
    {A, B}.

%% The same through a NIF stub called locally, as OTP's prim_file calls its NIFs.
local() ->
    Self = self(),
    spawn(fun() -> Self ! {a, prim_file:read_file(<<"/a">>)} end),
    spawn(fun() -> Self ! {b, prim_file:read_file(<<"/b">>)} end),
    A = receive {a, RA} -> RA end,
    B = receive {b, RB} -> RB end,
    {A, B}.

%% A process killed while it waits: its operation is dropped, and the VM goes on.
killed() ->
    P = spawn(fun() -> prim_file:read_file_nif(<<"/never">>) end),
    receive after 10 -> ok end,
    exit(P, kill),
    receive after 10 -> ok end,
    is_process_alive(P).

%% A message to a process that waits for a file does not end its wait.
messages() ->
    Self = self(),
    P = spawn(fun() -> Self ! {read, prim_file:read_file_nif(<<"/a">>)} end),
    P ! hello,
    receive {read, R} -> R end.
