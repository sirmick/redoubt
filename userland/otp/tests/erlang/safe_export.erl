-module(safe_export).
-export([start/0, id/1]).
%% binary_to_term(Bin, [safe]) and an export fun (EXPORT_EXT): safe mode decodes one only if
%% it names a function exported now (a loaded module's export, or a BIF), so a binary from
%% outside cannot make the VM look for, or load, code; without safe mode any decodes. An atom
%% that does not exist yet is refused in safe mode whatever it names.
id(X) -> X.

start() ->
    Fresh = list_to_atom("safe_export_" ++ "no_such_module"),
    [{Name, decode(Bin, [safe]), decode(Bin, [])} || {Name, Bin} <- [
        {exported, export(?MODULE, id, 1)},
        {wrong_arity, export(?MODULE, id, 2)},
        {missing_function, export(?MODULE, nope, 1)},
        {local_function, export(?MODULE, decode, 2)},
        {no_such_module, export(Fresh, id, 1)},
        {bif, export(erlang, '+', 2)},
        {bif_wrong_arity, export(erlang, '+', 3)},
        {bif_other, export(erlang, element, 2)},
        {loaded_library, export(lists, map, 2)},
        {library_missing, export(lists, map, 9)},
        {new_atoms, <<131, 113, 119, 12, "zz_safe_mod1", 119, 12, "zz_safe_fun1", 97, 1>>}
    ]].

export(M, F, A) ->
    MB = atom_to_binary(M),
    FB = atom_to_binary(F),
    <<131, 113, 119, (byte_size(MB)), MB/binary, 119, (byte_size(FB)), FB/binary, 97, A>>.

decode(Bin, Opts) ->
    try binary_to_term(Bin, Opts) of
        F when is_function(F) -> {fun_, erlang:fun_info(F, arity)}
    catch
        error:badarg -> badarg
    end.
