%% The width OTP's line editor measures a character at, prim_tty:npwcwidth/2: BEAM asks libc,
%% beamlet has none and OTP falls back to its own table. They agree on ASCII and on wide East
%% Asian characters, which is what the shell's prompt and lines hold; a combining mark is where
%% they part (libc says 0, the table 1), so it is not claimed here.
-module(char_width).
-export([start/0]).

start() ->
    [prim_tty:npwcwidth(C, unicode) || C <- [$a, $~, $\s, 16#4E16, 16#AC00, 16#FF21]].
