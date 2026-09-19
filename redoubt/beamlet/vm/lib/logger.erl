%% A small stand-in for OTP's `logger` (which lives in the kernel application, with handler
%% processes, configuration and formatters this VM does not need). It keeps the API that OTP
%% and Elixir code call, filters by level, and prints events to `standard_error`.
%%
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(logger).
-export([allow/2, macro_log/3, macro_log/4, macro_log/5,
         log/2, log/3, log/4,
         emergency/1, emergency/2, emergency/3, alert/1, alert/2, alert/3,
         critical/1, critical/2, critical/3, error/1, error/2, error/3,
         warning/1, warning/2, warning/3, notice/1, notice/2, notice/3,
         info/1, info/2, info/3, debug/1, debug/2, debug/3,
         compare_levels/2, get_primary_config/0, set_primary_config/1, set_primary_config/2,
         update_primary_config/1, get_config/0, add_handler/3, remove_handler/1,
         get_handler_config/0, get_handler_config/1, set_handler_config/2, set_handler_config/3,
         update_handler_config/2, update_handler_config/3,
         set_module_level/2, unset_module_level/1, set_application_level/2,
         get_process_metadata/0, set_process_metadata/1, update_process_metadata/1,
         unset_process_metadata/0, add_primary_filter/2, remove_primary_filter/1,
         add_handler_filter/3, remove_handler_filter/2, i/0, timestamp/0,
         get_module_level/0, get_module_level/1, unset_application_level/1, get_handler_ids/0,
         update_formatter_config/2, update_formatter_config/3, get_process_metadata/1]).

-define(LEVELS, [emergency, alert, critical, error, warning, notice, info, debug]).
-define(KEY, '$beamlet_logger_level').

%% ---- levels ----

%% `none` logs nothing, `all` everything, as in OTP's logger.
level_number(none) -> -1;
level_number(all) -> 8;
level_number(Level) -> index(Level, ?LEVELS, 0).

index(X, [X | _], N) -> N;
index(X, [_ | T], N) -> index(X, T, N + 1);
index(_, [], _) -> erlang:error(badarg).

primary_level() -> persistent_term:get(?KEY, notice).

allow(Level, _Module) ->
    level_number(Level) =< level_number(primary_level()).

compare_levels(A, B) ->
    NA = level_number(A), NB = level_number(B),
    if NA < NB -> gt; NA > NB -> lt; true -> eq end.

%% ---- logging ----

macro_log(Location, Level, StringOrReport) ->
    macro_log(Location, Level, StringOrReport, #{}).
macro_log(Location, Level, Format, Args) when is_list(Args) ->
    macro_log(Location, Level, Format, Args, #{});
macro_log(Location, Level, Report, Meta) ->
    emit(Level, {report, Report}, maps:merge(Location, Meta)).
macro_log(Location, Level, Format, Args, Meta) ->
    emit(Level, {Format, Args}, maps:merge(Location, Meta)).

log(Level, StringOrReport) -> log(Level, StringOrReport, #{}).
log(Level, Format, Args) when is_list(Args) -> log(Level, Format, Args, #{});
log(Level, Report, Meta) -> maybe_emit(Level, {report, Report}, Meta).
log(Level, Format, Args, Meta) -> maybe_emit(Level, {Format, Args}, Meta).

maybe_emit(Level, Msg, Meta) ->
    case allow(Level, undefined) of
        true -> emit(Level, Msg, Meta);
        false -> ok
    end.

emergency(X) -> log(emergency, X).
emergency(X, Y) -> log(emergency, X, Y).
emergency(X, Y, Z) -> log(emergency, X, Y, Z).
alert(X) -> log(alert, X).
alert(X, Y) -> log(alert, X, Y).
alert(X, Y, Z) -> log(alert, X, Y, Z).
critical(X) -> log(critical, X).
critical(X, Y) -> log(critical, X, Y).
critical(X, Y, Z) -> log(critical, X, Y, Z).
error(X) -> log(error, X).
error(X, Y) -> log(error, X, Y).
error(X, Y, Z) -> log(error, X, Y, Z).
warning(X) -> log(warning, X).
warning(X, Y) -> log(warning, X, Y).
warning(X, Y, Z) -> log(warning, X, Y, Z).
notice(X) -> log(notice, X).
notice(X, Y) -> log(notice, X, Y).
notice(X, Y, Z) -> log(notice, X, Y, Z).
info(X) -> log(info, X).
info(X, Y) -> log(info, X, Y).
info(X, Y, Z) -> log(info, X, Y, Z).
debug(X) -> log(debug, X).
debug(X, Y) -> log(debug, X, Y).
debug(X, Y, Z) -> log(debug, X, Y, Z).

%% Print one event. A report with a report_cb is formatted by it, as OTP's formatter does.
emit(Level, Msg, Meta) ->
    Text = try format(Msg, Meta)
           catch _:_ -> io_lib:format("~tp", [Msg])
           end,
    Header = ["=", string:uppercase(atom_to_list(Level)), " REPORT==== "],
    io:put_chars(standard_error, [Header, "\n", Text, "\n"]),
    ok.

format({report, Report}, #{report_cb := Cb}) when is_function(Cb, 1) ->
    {Format, Args} = Cb(Report),
    io_lib:format(Format, Args);
format({report, Report}, #{report_cb := Cb}) when is_function(Cb, 2) ->
    Cb(Report, #{depth => unlimited, chars_limit => unlimited, single_line => false});
format({report, Report}, _Meta) ->
    io_lib:format("~tp", [Report]);
format({Format, Args}, _Meta) ->
    io_lib:format(Format, Args).

timestamp() -> erlang:system_time(microsecond).

%% ---- configuration: only the primary level is kept ----

get_primary_config() -> #{level => primary_level(), filters => [], filter_default => log, metadata => #{}}.
set_primary_config(Config) when is_map(Config) ->
    maps:foreach(fun(K, V) -> set_primary_config(K, V) end, Config).
set_primary_config(level, Level) ->
    _ = level_number(Level),
    persistent_term:put(?KEY, Level),
    ok;
set_primary_config(_, _) -> ok.
update_primary_config(Config) -> set_primary_config(Config).
get_config() -> #{primary => get_primary_config(), handlers => [], module_levels => []}.
add_handler(_, _, _) -> ok.
remove_handler(_) -> ok.
get_handler_config() -> [].
get_handler_config(_) -> {error, {not_found, default}}.
set_handler_config(_, _) -> ok.
set_handler_config(_, _, _) -> ok.
update_handler_config(_, _) -> ok.
update_handler_config(_, _, _) -> ok.
set_module_level(_, _) -> ok.
%% Module levels are not kept: every module logs at the primary level.
get_module_level() -> [].
get_module_level(_) -> [].
unset_application_level(_) -> ok.
get_handler_ids() -> [].
update_formatter_config(_, _) -> ok.
update_formatter_config(_, _, _) -> ok.
unset_module_level(_) -> ok.
set_application_level(_, _) -> ok.
add_primary_filter(_, _) -> ok.
remove_primary_filter(_) -> ok.
add_handler_filter(_, _, _) -> ok.
remove_handler_filter(_, _) -> ok.
i() -> ok.

%% ---- process metadata, in the process dictionary as in OTP ----

-define(META, '$logger_metadata$').
get_process_metadata() -> get(?META).
get_process_metadata(_Pid) -> undefined.
set_process_metadata(Meta) when is_map(Meta) -> put(?META, Meta), ok.
update_process_metadata(Meta) when is_map(Meta) ->
    Old = case get(?META) of undefined -> #{}; M -> M end,
    set_process_metadata(maps:merge(Old, Meta)).
unset_process_metadata() -> erase(?META), ok.
