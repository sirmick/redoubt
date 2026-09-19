-module(tuples).
-export([start/0]).
-record(point, {x = 0, y = 0, z = 0}).
%% Tuples and records (records compile to tuples plus update_record).
start() ->
    T = {a, b, c},
    P = #point{x = 1},
    P2 = P#point{y = 2, z = 3},
    {element(2, T), setelement(1, T, z), tuple_size(T), tuple_to_list(T), list_to_tuple([1, 2]),
     erlang:make_tuple(3, x), erlang:append_element(T, d), P, P2, P2#point.z,
     is_record(P, point), case P2 of #point{y = Y} -> Y end}.
