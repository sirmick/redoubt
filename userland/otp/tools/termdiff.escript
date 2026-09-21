#!/usr/bin/env escript
%% Usage: termdiff.escript WANT_FILE GOT_FILE
%% Both files hold one term printed with ~w (the last line of GOT). Prints the path to each
%% differing sub-term, e.g. "3.2.1: want <<1>> got <<2>>", up to 10 differences.
main([Want, Got]) ->
    W = read(Want), G = read(Got),
    case {W, G} of
        {{ok, A}, {ok, B}} -> diff([], A, B), ok;
        _ -> io:format("  (could not parse: ~p / ~p)~n", [element(1, W), element(1, G)])
    end.

read(File) ->
    {ok, Bin} = file:read_file(File),
    Lines = string:split(string:trim(Bin), "\n", all),
    Last = lists:last(Lines),
    try
        {ok, Toks, _} = erl_scan:string(unicode:characters_to_list(Last) ++ "."),
        erl_parse:parse_term(Toks)
    catch _:_ -> {error, unparsable}
    end.

diff(Path, A, A) -> Path;
diff(Path, A, B) when is_tuple(A), is_tuple(B), tuple_size(A) =:= tuple_size(B) ->
    walk(Path, tuple_to_list(A), tuple_to_list(B), 1);
diff(Path, A, B) when is_list(A), is_list(B), length(A) =:= length(B) ->
    walk(Path, A, B, 1);
diff(Path, A, B) ->
    case get(count) of
        N when is_integer(N), N >= 10 -> ok;
        N ->
            put(count, (case N of undefined -> 0; _ -> N end) + 1),
            io:format("  ~s: want ~P~n  ~*s  got  ~P~n", [path(Path), A, 12, length(path(Path)), "", B, 12])
    end.

walk(_, [], [], _) -> ok;
walk(Path, [X | Xs], [Y | Ys], I) -> diff([I | Path], X, Y), walk(Path, Xs, Ys, I + 1).

path([]) -> "(top)";
path(P) -> lists:join(".", [integer_to_list(I) || I <- lists:reverse(P)]).
