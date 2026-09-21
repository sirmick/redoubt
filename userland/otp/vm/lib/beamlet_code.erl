%% Running a module's on_load function after code:load_binary/3, as BEAM's code server does:
%% the module stays loaded if the function returns `ok`, and is unloaded otherwise.
%% (Modules the platform supplies are not run through this: their on_load functions only load
%% NIFs, which this VM provides as natives.)
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(beamlet_code).
-export([run_on_load/2]).

run_on_load(Module, _Function) ->
    Result = try erlang:call_on_load_function(Module) of
                 ok -> ok;
                 Other -> {bad_return, Other}
             catch
                 Class:Reason:Stack -> {Class, Reason, Stack}
             end,
    case Result of
        ok ->
            {module, Module};
        Failure ->
            _ = code:delete(Module),
            error_logger:error_msg("The on_load function for module ~p returned:~n~p~n", [Module, Failure]),
            {error, on_load_failure}
    end.
