-module(compare).
-export([start/0]).
%% The standard term order across every type, and sorting with it.
start() ->
    Terms = [<<"b">>, [a], [], #{a => 1}, {1, 2}, {1}, self_pid, make_ref_x, fun_x, atom, 2.5, 3, -1, 1.0, 1,
             <<1:3>>, "abc", [1 | 2], #{}, {}, 16#FFFFFFFFFFFFFFFFFF],
    Clean = [T || T <- Terms, not is_atom(T) orelse T =:= atom],
    {lists:sort(Clean), lists:usort([1, 1.0, 2, 2.0]), [X < Y || X <- [1, a, {}], Y <- [1.0, b, []]],
     [1] < [1, 2], [2] > [1, 2], {2} < {1, 1}, #{b => 1} < #{a => 1, c => 2},
     #{1 => a} < #{1.0 => a}, <<1:1>> < <<1>>}.
