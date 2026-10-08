-module(fun_identity).
-export([start/0]).
%% A fun's identity across term_to_binary and binary_to_term. The module checksum it is
%% encoded with names its code: with the loaded module's, it is that code's fun (equal to it,
%% callable, written back with the loaded code's fields); with any other it is another fun,
%% kept as it came (a call is badfun, and it is written back byte for byte). The OldIndex
%% field is not part of the identity. Left out: the order of two funs that differ only in the
%% checksum, and the OldIndex a second decode of one unknown checksum is written back with,
%% which are BEAM's fun table's own (it keeps the first decode's).
start() ->
    X = 3,
    Funs = [fun(A) -> A * 2 end, fun(A) -> A * X end],
    [identity(F) || F <- Funs].

identity(F) ->
    Bin = term_to_binary(F),
    <<131, 112, Size:32, Ar, Md5:16/binary, Idx:32, NF:32, 119, ML, Mod:ML/binary, 97, Old, Rest/binary>> = Bin,
    Make = fun(Sum, OldIndex) ->
        <<131, 112, Size:32, Ar, Sum/binary, Idx:32, NF:32, 119, ML, Mod/binary, 97, OldIndex, Rest/binary>>
    end,
    Same = binary_to_term(Bin),
    Zero = Make(<<0:128>>, Old),
    Z = binary_to_term(Zero),
    Shifted = Make(Md5, Old bxor 42),
    S = binary_to_term(Shifted),
    B = binary_to_term(Make(<<0:128>>, Old bxor 42)),
    Call = fun(G) -> try G(21) catch error:{E, _} -> E; error:E -> E end end,
    {{Same =:= F, Call(Same), term_to_binary(Same) =:= Bin},
     {Z =:= F, Z =:= binary_to_term(Zero), Call(Z), term_to_binary(Z) =:= Zero,
      erlang:phash2(Z) =:= erlang:phash2(F), erlang:fun_info(Z, new_uniq), erlang:fun_info(Z, index) =:= {index, Old},
      erlang:fun_info(Z, module), erlang:fun_info(Z, arity), erlang:fun_info(Z, env) =:= erlang:fun_info(F, env)},
     {S =:= F, Call(S), term_to_binary(S) =:= Bin},
     {B =:= Z, Call(B)}}.
