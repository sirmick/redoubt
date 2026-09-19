-module(exceptions).
-export([start/0]).
%% try/catch, catch, throw, exit, error, after, nested handlers and rethrow.
start() ->
    {catch throw(t), catch exit(e), element(1, catch error(r)),
     try throw(x) catch throw:X -> {caught, X} end,
     try error(badarg) catch error:badarg:St -> is_list(St) end,
     try 1 / zero() catch error:badarith -> badarith end,
     try exit(bye) catch exit:R -> R end,
     try ok of ok -> yes after put(k, cleaned) end, get(k),
     try try throw(inner) after put(k2, done) end catch throw:I -> I end, get(k2),
     try try error(a) catch error:a -> throw(b) end catch throw:B -> B end,
     try erlang:raise(throw, reraised, []) catch throw:Rr -> Rr end,
     try case two() of 1 -> one end catch error:{case_clause, 2} -> case_clause end,
     try bad_match() catch error:{badmatch, V} -> {badmatch, V} end,
     try no_clause(3) catch error:function_clause -> function_clause end,
     try if_test(zero()) catch error:if_clause -> if_clause end,
     try undefined_mod:f() catch error:undef -> undef end,
     deep(1000)}.
zero() -> 0.
if_test(Z) -> if Z == 1 -> x end.
two() -> 2.
bad_match() -> {a, _} = {b, two()}.
no_clause(1) -> one.
deep(0) -> throw(bottom);
deep(N) -> try deep(N - 1) catch throw:bottom when N == 1000 -> reached_top end.
