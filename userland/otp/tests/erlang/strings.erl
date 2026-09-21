-module(strings).
-export([start/0]).
%% Conversions between atoms, lists, binaries and numbers.
start() ->
    {atom_to_list(hello), list_to_atom("world"), atom_to_binary(ok), binary_to_atom(<<"x y">>),
     integer_to_list(255), integer_to_list(255, 16), integer_to_binary(-42), list_to_integer("-123"),
     binary_to_integer(<<"ff">>, 16), list_to_integer("123456789012345678901234567890"),
     float_to_list(1.5), 'quoted atom', 'CamelCase', [], "", "abc",
     try list_to_integer("12x") catch error:badarg -> badarg end,
     try list_to_existing_atom("surely_not_an_existing_atom_xyzzy") catch error:badarg -> badarg end}.
