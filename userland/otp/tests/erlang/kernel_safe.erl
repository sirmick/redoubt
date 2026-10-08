%% kernel_safe_sup is there once an application with a callback module has started, as on a BEAM
%% node, and OTP's group_history, which waits for it before group serves a line, then loads.
-module(kernel_safe).
-behaviour(application).
-export([start/0, start/2, stop/1]).

start() ->
    ok = application:load({application, kernel_safe_probe,
                           [{description, "a probe"}, {vsn, "1"}, {modules, [kernel_safe]},
                            {registered, []}, {applications, [kernel, stdlib]},
                            {mod, {kernel_safe, []}}]}),
    ok = application:start(kernel_safe_probe),
    Self = self(),
    spawn(fun() -> Self ! {history, group_history:load()} end),
    History = receive {history, H} -> H after 5000 -> waited end,
    {is_pid(whereis(kernel_safe_sup)), History}.

%% The application's callback: a process that waits.
start(normal, []) -> {ok, spawn(fun() -> receive stop -> ok end end)}.

stop(_State) -> ok.
