%% A gen_server that answers ping with pong, for names.erl.
-module(names_echo).
-behaviour(gen_server).
-export([init/1, handle_call/3, handle_cast/2]).

init([]) -> {ok, []}.
handle_call(ping, _From, S) -> {reply, pong, S}.
handle_cast(_, S) -> {noreply, S}.
