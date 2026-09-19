-module(re_hostile).
-export([start/0]).
%% Patterns and subjects that are slow or huge for backtracking engines. Here each must finish
%% promptly with a result or a compile error. PCRE (the oracle) hits its match limit on some,
%% so only "did every call finish" is compared.
start() ->
    Evil = binary:copy(<<"a">>, 30),
    Calls = [
        fun() -> re:run(<<Evil/binary, "!">>, "(a+)+$") end,
        fun() -> re:run(<<Evil/binary, "!">>, "^(a|aa)+$") end,
        fun() -> re:run(binary:copy(<<"ab">>, 5000), "(.*a){12}") end,
        fun() -> re:compile("(a{1000}){1000}") end,
        fun() -> re:compile(lists:duplicate(20000, $()) end,
        fun() -> re:run(binary:copy(<<"x">>, 100000), "x*y") end
    ],
    Done = [begin _ = (catch F()), done end || F <- Calls],
    {length(Done), lists:usort(Done)}.
