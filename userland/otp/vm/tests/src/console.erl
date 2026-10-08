%% Fixture for vm/tests/console.rs: who reads the console. Only the VM's own BIFs are used: the
%% tests load no OTP modules. Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/console.erl
-module(console).
-export([second_reader/0, after_exit/0, size/0, resize/0]).

%% The console's size is the platform's answer, asked at each call.
size() ->
    {beamlet:console_size(), beamlet:console_size()}.

%% While one process reads the console, a second subscription is refused: the input goes to the
%% first, and the second gets nothing.
second_reader() ->
    ok = beamlet:console_subscribe(),
    Self = self(),
    spawn(fun() ->
                  Self ! {second, beamlet:console_subscribe()},
                  receive {beamlet_console, B} -> Self ! {other, B} after 100 -> Self ! {other, none} end
          end),
    Second = receive {second, R} -> R end,
    First = receive {beamlet_console, Bytes} -> Bytes after 1000 -> none end,
    Other = receive {other, O} -> O end,
    {Second, First, Other}.

%% Once the reader has exited, the console is free for the next.
after_exit() ->
    spawn(fun() -> ok = beamlet:console_subscribe() end),
    receive after 10 -> ok end,
    beamlet:console_subscribe().

%% A change of the console's size reaches its reader as a message, ahead of the input that came
%% with it.
resize() ->
    ok = beamlet:console_subscribe(),
    Size = receive {beamlet_console_resize, S} -> S after 1000 -> none end,
    Input = receive {beamlet_console, B} -> B after 1000 -> none end,
    {Size, Input}.
