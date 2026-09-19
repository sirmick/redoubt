%% Unicode properties and braced escapes in re patterns.
-module(re_unicode).
-export([start/0]).

start() ->
    [{P, S, re:run(S, P, [unicode, {capture, all, binary}])} ||
        {P, S} <- [{"\\p{Latin}$", <<"olá"/utf8>>}, {"\\p{Lu}+", <<"abcDEF"/utf8>>},
                   {"\\P{L}+", <<"ab12cd"/utf8>>}, {"\\x{263A}", <<"x☺y"/utf8>>},
                   {"[[:space:]]", <<"a b">>}]].
