%% The parts of the kernel application's start that every VM needs, run at boot: OTP's own
%% logger, set up as kernel:start/2 sets it up (its configuration, logger_sup, and the default
%% handler), so that handlers, filters and formatters (Elixir's Logger, ExUnit's capture_log)
%% work as on BEAM. Used only when the platform provides the kernel's logger; otherwise the
%% embedded stand-ins in logger.erl and error_logger.erl are loaded instead.
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(beamlet_kernel).
-export([start/0]).

start() ->
    %% The kernel's environment (logger_level => notice, ...), which configures the logger.
    _ = application:load(kernel),
    %% A release's boot script starts logger_server before the kernel application.
    {ok, Server} = logger_server:start_link(),
    unlink(Server),
    ok = logger:internal_init_logger(),
    {ok, Sup} = logger_sup:start_link(),
    %% The boot process ends now; the supervisor must outlive it.
    unlink(Sup),
    ok = logger:add_handlers(kernel).
