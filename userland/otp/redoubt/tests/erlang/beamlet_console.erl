%% beamlet-console (tests/beamlet-console.toml): the console, the clock and randomness on the
%% machine. It reads a typed line and echoes it, sleeps 200 ms in a receive, prints the
%% microseconds the monotonic clock advanced across it, and prints 32 random bytes in hex.
-module(beamlet_console).
-export([start/0]).

start() ->
    %% The prompt is a whole line, so the bench can wait for it before it types.
    io:put_chars("beamlet-console: type a line\n"),
    Line = io:get_line(""),
    io:put_chars(["echo: ", [C || C <- Line, C =/= $\n, C =/= $\r], "\n"]),
    Before = erlang:monotonic_time(microsecond),
    receive after 200 -> ok end,
    After = erlang:monotonic_time(microsecond),
    io:put_chars(["slept: ", integer_to_list(After - Before), " us\n"]),
    Bytes = crypto:strong_rand_bytes(32),
    io:put_chars(["random: ", hex(Bytes), "\n"]),
    ok.

hex(Bytes) -> [hex_digit(D) || <<D:4>> <= Bytes].

hex_digit(D) when D < 10 -> $0 + D;
hex_digit(D) -> $a + D - 10.
