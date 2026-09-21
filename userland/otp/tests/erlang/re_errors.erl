%% re:compile errors carry PCRE's messages and positions for the common mistakes.
-module(re_errors).
-export([start/0]).

start() ->
    [re:compile(P) || P <- ["[invalid", "(abc", "abc)", "*a", "a{3,2}", "x\\", "[z-a]", "(?<n>a)(?<n>b)"]].
