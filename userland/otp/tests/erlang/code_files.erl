-module(code_files).
-export([start/0]).
%% A loaded module's file is one answer: code:which/1, code:is_loaded/1, code:all_loaded/0 and
%% code:all_available/0 all give it (a path, or preloaded), whatever it is on this host.
start() ->
    Loaded = lists:sort(code:all_loaded()),
    Available = lists:sort([{list_to_atom(M), F} || {M, F, true} <- code:all_available()]),
    Which = [{M, code:which(M)} || {M, _} <- Loaded],
    IsLoaded = [{M, F} || {M, _} <- Loaded, {file, F} <- [code:is_loaded(M)]],
    {Loaded =:= Available, Loaded =:= lists:sort(Which), Loaded =:= lists:sort(IsLoaded),
     lists:keymember(?MODULE, 1, Loaded), code:is_loaded(no_such_module_here)}.
