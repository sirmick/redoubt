%% Fixture for cli/tests/tty.rs: a process that reads the console, which puts a terminal in raw
%% mode, and then ends each way beamlet can end. Only the VM's own BIFs are used: the tests
%% load no OTP modules. Rebuild: erlc +deterministic -o cli/tests/fixtures cli/tests/src/tty_probe.erl
-module(tty_probe).
-export([line/0, crash/0, halt/0]).

%% Reads what is typed and returns it: a normal end.
line() ->
    ok = beamlet:console_subscribe(),
    receive {beamlet_console, Bytes} -> Bytes end.

%% Reads a line, then fails: the run ends with an exception.
crash() ->
    _ = line(),
    erlang:error(boom).

%% Reads a line, then halts with a status: beamlet exits with it.
halt() ->
    _ = line(),
    erlang:halt(3).
