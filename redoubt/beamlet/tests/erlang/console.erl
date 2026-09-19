%% Console input through the `user` I/O server: get_line, read, get_chars, fread, and eof.
-module(console).
-export([start/0]).
start() ->
    L1 = io:get_line("name> "),
    {ok, T} = io:read("term> "),
    C = io:get_chars('', 3),
    {ok, [N]} = io:fread("num> ", "~d"),
    L2 = io:get_line(""),
    L3 = io:get_line(""),
    {L1, T, C, N, L2, L3}.
