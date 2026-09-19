%% Funs in the external term format: local funs with and without free variables round-trip
%% and can be called; the module checksum that identifies them matches BEAM's.
-module(funser).
-export([start/0]).

start() ->
    X = 10, Y = {tag, "text"},
    F0 = fun() -> ok end,
    F1 = fun(A) -> A + X end,
    F2 = fun(A, B) -> {A, B, Y, X} end,
    Ext = fun lists:reverse/1,
    Back = [binary_to_term(term_to_binary(F)) || F <- [F0, F1, F2, Ext]],
    [B0, B1, B2, BExt] = Back,
    Compressed = binary_to_term(term_to_binary(F2, [compressed])),
    %% The body of a NEW_FUN_EXT up to the creator pid is the same on both VMs.
    <<131, 112, _Size:32, Head:(1 + 16 + 4 + 4)/binary, _/binary>> = term_to_binary(F1),
    {B0(), B1(5), B2(1, 2), BExt([1, 2, 3]), Compressed(a, b),
     Back =:= [F0, F1, F2, Ext],
     erlang:phash2(F1) =:= erlang:phash2(B1),
     Head, funser:module_info(md5)}.
