-module(unicode_rest).
-export([start/0]).
%% What unicode:characters_to_list/2 and characters_to_binary/2 return as the rest when
%% conversion stops: a code point that cannot be converted, a byte sequence that is not UTF-8,
%% and one a binary's end cuts short, at every depth of nesting, with empty lists and binaries
%% around it.
start() ->
    B = <<104, 233>>,
    X = <<160>>,
    Lists = [[$h, -1], [$h, -1 | "ello"], ["foo", [$h, -1 | "ello"], "bar"], [[[$h, -1]]],
             [[[$h, -1], "x"], "y"], [["a", [-1, "z"]], "b"], [[[$h, -1]], []], [[$h, -1], [], "q"],
             [[$h, -1 | []] | "q"], [-1], [[], [-1]], [<<"ab">>, -1], [<<"ab">>, [-1]], [$h, 16#110000]],
    Invalid = [X, [X], [X, "y"], [[X]], [[X], "y"], [[X], []], [<<104, 160, 65>>], ["ab" | <<"cd", 160>>],
               ["a" | X], [<<104, 160>> | "q"], [[X | "q"]], [<<16#A0, 16#A1>>], ["a", X], [<<"a">>, X, <<"b">>],
               [X | <<>>], [X, <<>>], [$a, X], [<<"a">>, [X]], [X, 113]],
    Cut = [[B], B, ["foo", [B, ["bar"]]], ["foo", [B, ["bar"], "foobar"]], [B, "x"], [[B], "x"], [[[B]]],
           [B | "tail"], ["a", B, "c"], <<104, 226, 130>>, [<<104, 226, 130>>, "z"], [[B], []], [[B], [], "z"],
           [[B, []], "z"], [[[B]], "z"], [[B | []] | "z"], [B, [[]], "z"], [B, <<>>, "z"], [B, [[]]], [B, <<>>],
           [B, <<"x">>], [B, [["z"]]], [B, [[], "z"]], [[B], [[[]]], "z"], [B, -1], [B, [-1]], [B | <<"x">>],
           [B | <<>>], [B, 16#110000], [B, [<<>>, "z"]], [[B, <<"x">>], "q"], [B, <<"xy">>, "q"],
           [B, <<128, 128, 65>>], [B, <<128>>, <<128>>, "z"], [B, <<128>>], [B, <<128>>, "z"], [B, [<<"x">>]],
           [[B], <<"x">>], [B, <<128, 65, 160>>, "q"], [B, <<"x">> | "q"], [B, <<"x">>, <<"y">>]],
    {[unicode:characters_to_list(I, latin1) || I <- Lists],
     [unicode:characters_to_list(I, unicode) || I <- Lists ++ Invalid ++ Cut],
     [unicode:characters_to_binary(I, unicode) || I <- Lists ++ Invalid ++ Cut]}.
