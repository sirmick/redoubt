%% A small application controller in place of the kernel's application and
%% application_controller: enough to load .app specifications, start applications in
%% dependency order by calling their `mod` callback, and keep their environments.
%%
%% Specifications come from the platform (`Platform::load_app`) through beamlet:app_spec/1.
%% There are no application masters, no start types beyond `temporary`, and no takeover.
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(application).
-export([load/1, load/2, start/1, start/2, ensure_started/1, ensure_started/2,
         ensure_all_started/1, ensure_all_started/2, ensure_all_started/3, stop/1, unload/1,
         get_env/1, get_env/2, get_env/3, get_all_env/0, get_all_env/1,
         set_env/1, set_env/2, set_env/3, set_env/4, unset_env/2, unset_env/3,
         get_key/1, get_key/2, get_all_key/0, get_all_key/1,
         get_application/0, get_application/1, which_applications/0, which_applications/1,
         loaded_applications/0, info/0, set_env_defaults/0]).

-define(LOADED, '$beamlet_apps_loaded').    % #{App => Spec}
-define(RUNNING, '$beamlet_apps_running').  % [{App, TopPid}], most recent first
-define(ENV(App), {'$beamlet_app_env', App}).

loaded() -> persistent_term:get(?LOADED, #{}).
%% kernel and stdlib are always running, as on a BEAM node: this VM provides what their
%% processes would (I/O, logger, code loading), so their start callbacks are never run.
running() -> persistent_term:get(?RUNNING, [{stdlib, undefined}, {kernel, undefined}]).

%% ---- loading ----

load(App) -> load(App, []).
load({application, App, Spec}, _) -> do_load(App, Spec);
load(App, _) when is_atom(App) ->
    case maps:is_key(App, loaded()) of
        true -> {error, {already_loaded, App}};
        false ->
            case read_spec(App) of
                {ok, Spec} -> do_load(App, Spec);
                Error -> Error
            end
    end.

read_spec(App) ->
    case beamlet:app_spec(App) of
        Bin when is_binary(Bin) ->
            try
                {ok, Tokens, _} = erl_scan:string(unicode:characters_to_list(Bin)),
                {ok, {application, App, Spec}} = erl_parse:parse_term(Tokens),
                {ok, Spec}
            catch _:_ -> {error, {bad_application, App}}
            end;
        error -> {error, {"no such file or directory", atom_to_list(App) ++ ".app"}}
    end.

do_load(App, Spec) ->
    persistent_term:put(?LOADED, (loaded())#{App => Spec}),
    Env0 = persistent_term:get(?ENV(App), #{}),
    %% Values set before loading win over the specification's defaults.
    Env = maps:merge(maps:from_list(proplists:get_value(env, Spec, [])), Env0),
    persistent_term:put(?ENV(App), Env),
    ok.

unload(App) ->
    persistent_term:put(?LOADED, maps:remove(App, loaded())),
    persistent_term:erase(?ENV(App)),
    ok.

ensure_loaded(App) ->
    case load(App) of
        ok -> ok;
        {error, {already_loaded, App}} -> ok;
        Error -> Error
    end.

%% ---- starting ----

start(App) -> start(App, temporary).
start(App, _Type) ->
    case lists:keymember(App, 1, running()) of
        true -> {error, {already_started, App}};
        false ->
            case ensure_loaded(App) of
                ok -> start_loaded(App);
                Error -> Error
            end
    end.

start_loaded(App) ->
    Spec = maps:get(App, loaded()),
    Deps = proplists:get_value(applications, Spec, []),
    case [D || D <- Deps, not lists:keymember(D, 1, running())] of
        [D | _] -> {error, {not_started, D}};
        [] ->
            case proplists:get_value(mod, Spec) of
                undefined -> started(App, undefined);
                {Mod, Args} ->
                    kernel_services(),
                    try Mod:start(normal, Args) of
                        {ok, Pid} -> started(App, Pid);
                        {ok, Pid, _State} -> started(App, Pid);
                        {error, Reason} -> {error, {Reason, {Mod, start, [normal, Args]}}};
                        Other -> {error, {bad_return, {{Mod, start, [normal, Args]}, Other}}}
                    catch C:R:St -> {error, {{C, R, St}, {Mod, start, [normal, Args]}}}
                    end
            end
    end.

%% The kernel application's servers that other applications call, started the first time an
%% application with a callback module starts (BEAM starts them at boot): erl_signal_server,
%% the event manager for OS signals (there are none here, but Elixir registers handlers), and
%% global_name_server, for {global, Name} registration within this one node.
kernel_services() ->
    case whereis(erl_signal_server) of
        undefined -> {ok, _} = gen_event:start({local, erl_signal_server}), ok;
        _ -> ok
    end,
    case whereis(global_name_server) of
        undefined -> {ok, _} = global:start(), ok;
        _ -> ok
    end.

started(App, Pid) ->
    persistent_term:put(?RUNNING, [{App, Pid} | running()]),
    ok.

ensure_started(App) -> ensure_started(App, temporary).
ensure_started(App, Type) ->
    case start(App, Type) of
        ok -> ok;
        {error, {already_started, App}} -> ok;
        Error -> Error
    end.

ensure_all_started(App) -> ensure_all_started(App, temporary).
ensure_all_started(App, Type) -> ensure_all_started(App, Type, serial).
ensure_all_started(Apps, Type, _Mode) when is_list(Apps) ->
    lists:foldl(fun(A, {ok, Acc}) ->
                        case ensure_all_started(A, Type) of
                            {ok, S} -> {ok, Acc ++ S};
                            E -> E
                        end;
                   (_, E) -> E
                end, {ok, []}, Apps);
ensure_all_started(App, Type, _Mode) ->
    case start_all(App, Type, []) of
        {ok, Started} -> {ok, lists:reverse(Started)};
        Error -> Error
    end.

start_all(App, Type, Started) ->
    case lists:keymember(App, 1, running()) of
        true -> {ok, Started};
        false ->
            case ensure_loaded(App) of
                ok ->
                    Spec = maps:get(App, loaded()),
                    Deps = proplists:get_value(applications, Spec, []) ++
                           proplists:get_value(optional_applications, Spec, []) -- [],
                    case start_deps(Deps, Type, Started, proplists:get_value(optional_applications, Spec, [])) of
                        {ok, S1} ->
                            case start(App, Type) of
                                ok -> {ok, [App | S1]};
                                {error, {already_started, App}} -> {ok, S1};
                                Error -> Error
                            end;
                        Error -> Error
                    end;
                Error -> Error
            end
    end.

start_deps([], _Type, Started, _Optional) -> {ok, Started};
start_deps([D | Ds], Type, Started, Optional) ->
    case start_all(D, Type, Started) of
        {ok, S} -> start_deps(Ds, Type, S, Optional);
        _ when is_list(Optional) ->
            case lists:member(D, Optional) of
                true -> start_deps(Ds, Type, Started, Optional);
                false -> {error, {D, not_started}}
            end
    end.

stop(App) ->
    case lists:keytake(App, 1, running()) of
        {value, {App, Pid}, Rest} ->
            persistent_term:put(?RUNNING, Rest),
            case is_pid(Pid) of
                true ->
                    Ref = monitor(process, Pid),
                    unlink(Pid),
                    exit(Pid, shutdown),
                    receive {'DOWN', Ref, process, Pid, _} -> ok after 5000 -> exit(Pid, kill) end;
                false -> ok
            end,
            ok;
        false -> {error, {not_started, App}}
    end.

%% ---- environment ----

get_env(_Key) -> undefined.
get_env(App, Key) ->
    case maps:find(Key, persistent_term:get(?ENV(App), #{})) of
        {ok, V} -> {ok, V};
        error -> undefined
    end.
get_env(App, Key, Default) ->
    maps:get(Key, persistent_term:get(?ENV(App), #{}), Default).

get_all_env() -> [].
get_all_env(App) -> maps:to_list(persistent_term:get(?ENV(App), #{})).

set_env(Config) ->
    [set_env(App, K, V) || {App, Envs} <- Config, {K, V} <- Envs],
    ok.
set_env(Config, _Opts) -> set_env(Config).
set_env(App, Key, Val) ->
    persistent_term:put(?ENV(App), (persistent_term:get(?ENV(App), #{}))#{Key => Val}),
    ok.
set_env(App, Key, Val, _Opts) -> set_env(App, Key, Val).

unset_env(App, Key) ->
    persistent_term:put(?ENV(App), maps:remove(Key, persistent_term:get(?ENV(App), #{}))),
    ok.
unset_env(App, Key, _Opts) -> unset_env(App, Key).

set_env_defaults() -> ok.

%% ---- keys and queries ----

get_key(_Key) -> undefined.
get_key(App, Key) ->
    _ = ensure_loaded(App),
    case maps:find(App, loaded()) of
        {ok, Spec} ->
            case lists:keyfind(Key, 1, Spec) of
                {Key, V} -> {ok, V};
                false -> {ok, []}
            end;
        error -> undefined
    end.

get_all_key() -> undefined.
get_all_key(App) ->
    case maps:find(App, loaded()) of
        {ok, Spec} -> {ok, Spec};
        error -> undefined
    end.

%% No application masters, so a process's application is unknown.
%% The application a process or module belongs to: for a module, the loaded application whose
%% specification lists it. Processes are not tracked (there are no application masters).
get_application() -> undefined.
get_application(Module) when is_atom(Module) ->
    Owners = [App || {App, Spec} <- maps:to_list(loaded()),
                     lists:member(Module, proplists:get_value(modules, Spec, []))],
    case Owners of
        [App | _] -> {ok, App};
        [] -> undefined
    end;
get_application(_Pid) -> undefined.

which_applications() ->
    [ensure_loaded(A) || {A, _} <- running()],
    [{A, proplists:get_value(description, maps:get(A, loaded(), []), ""),
      proplists:get_value(vsn, maps:get(A, loaded(), []), "")} || {A, _} <- running()].
which_applications(_Timeout) -> which_applications().

loaded_applications() ->
    [{A, proplists:get_value(description, S, ""), proplists:get_value(vsn, S, "")}
     || {A, S} <- maps:to_list(loaded())].

info() -> [{loaded, loaded_applications()}, {running, running()}].
