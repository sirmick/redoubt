%% Run ExUnit test files on beamlet (or BEAM) the way Elixir's `make test` does: from DIR
%% (e.g. /lib/elixir), requiring HELPER and then each FILE, all relative to DIR.
%%   beamlet --root ROOT ... exunit run DIR HELPER FILE...
%% Starts `logger` first, as the `elixir` command does. Returns ExUnit's summary map; the
%% report goes to the console.
-module(exunit).
-export([run/1]).

run([Dir, Helper | Files]) ->
    {ok, _} = application:ensure_all_started(logger),
    {ok, _} = application:ensure_all_started(ex_unit),
    ok = file:set_cwd(Dir),
    'Elixir.Code':require_file(unicode:characters_to_binary(Helper)),
    'Elixir.ExUnit':configure([{autorun, false}]),
    %% FILE:LINE runs only the test at that line, as `mix test FILE:LINE` does.
    Lines = [L || F <- Files, [_, L] <- [string:split(F, ":", trailing)]],
    case Lines of
        [] -> ok;
        _ -> 'Elixir.ExUnit':configure([{exclude, [test]}, {include, [{line, list_to_binary(L)} || L <- Lines]}])
    end,
    [begin
         io:format("== ~ts~n", [F]),
         'Elixir.Code':require_file(unicode:characters_to_binary(F))
     end || F0 <- Files, F <- [hd(string:split(F0, ":", trailing))]],
    'Elixir.ExUnit':run().
