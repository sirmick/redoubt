-module(re_test).
-export([start/0]).
%% re: compile and run with every capture spec and type, global matching (including empty
%% matches), offsets, flags, unicode, named groups, replace and split. PCRE is the oracle.
start() ->
    {ok, MP} = re:compile("a(b)?(?<n>c)", [unicode]),
    Runs = [
        re:run("xabcac", MP), re:run("xabcac", MP, [global]),
        re:run("xac", MP, [{capture, all, list}]), re:run(<<"xac">>, MP, [{capture, all_but_first, binary}]),
        re:run("xac", MP, [{capture, first, index}]), re:run("xac", MP, [{capture, none}]),
        re:run("xac", MP, [{capture, [n, 1, 0], list}]), re:run("xac", MP, [{capture, all_names, binary}]),
        re:run("zzz", MP), re:run("abcabc", MP, [{offset, 2}]),
        re:run(<<"xac">>, "(q)?a", [{capture, all, binary}]),
        re:run("aaa", "a*", [global]), re:run("abc", "", [global]), re:run("a1b22c333", "[0-9]+", [global, {capture, all, list}]),
        re:run("Hello World", "world", [caseless, {capture, all, list}]),
        re:run("one\ntwo", "^two$", [multiline, {capture, all, list}]), re:run("one\ntwo", "^two$"),
        re:run("a\nb", "a.b", [dotall]), re:run("a\nb", "a.b"),
        re:run("abc", "a b c", [extended]), re:run("aaa", "a+?", [ungreedy, {capture, all, list}]),
        re:run("xab", "ab", [anchored]), re:run("abx", "ab", [anchored]),
        re:run("héllo wörld", "\\w+", [unicode, global, {capture, all, list}]),
        re:run(<<"héllo"/utf8>>, <<"é"/utf8>>, [unicode]),
        re:run("2024-09-18", "(?<y>\\d{4})-(?<m>\\d\\d)-(?<d>\\d\\d)", [{capture, [d, m, y], list}]),
        re:inspect(element(2, re:compile("(?<b>x)(?<a>y)")), namelist)
    ],
    Other = [
        re:replace("hello world", "o", "0", [global, {return, list}]),
        re:replace("hello world", "(l+)", "[\\1]", [{return, binary}]),
        re:replace("abc", "", "-", [global, {return, list}]),
        re:split("a,b,,c", ",", [{return, list}]), re:split("a1b22c", "[0-9]+", [{return, binary}]),
        re:split("abc", "", [{return, list}]), re:split("a b  c", " +", [{return, list}, trim]),
        element(1, re:compile("(unclosed")), element(1, re:compile("a{2,1}")),
        try re:run("abc", "(", []) catch error:badarg -> badarg end
    ],
    {Runs, Other}.

%% Not part of start/0's comparison: run by re_hostile:start/0.
