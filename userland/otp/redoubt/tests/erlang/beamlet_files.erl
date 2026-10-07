%% beamlet-files (docs/userland/files.md, "Files over 9P"): files over 9P on the home volume
%% bound at /home/alice, through OTP's own prim_file, whose natives are beamlet's. It writes a
%% file, reads it back, lists the directory, removes the file, reads one larger than one 9P answer
%% with eight processes at once, renames a directory into itself, which the volume refuses, and
%% reads a path no binding holds.
-module(beamlet_files).
-export([start/0]).

start() ->
    Notes = "/home/alice/notes.txt",
    say("wrote: ~s", [prim_file:write_file(Notes, <<"buy milk\n">>)]),
    {ok, Read} = prim_file:read_file(Notes),
    say("read back: ~s", [Read]),
    say("listed: ~s", [prim_file:list_dir("/home/alice")]),
    say("removed: ~s", [prim_file:delete(Notes)]),
    say("listed after: ~s", [prim_file:list_dir("/home/alice")]),
    % More than one 9P answer (64 KiB less its header), built small: a heap is a sixteenth of the
    % VM's budget, and a list of every byte would not fit one.
    Big = list_to_binary(lists:duplicate(280, list_to_binary(lists:seq(0, 249)))),
    ok = prim_file:write_file("/home/alice/big", Big),
    Self = self(),
    Readers = [spawn(fun() -> Self ! {self(), prim_file:read_file("/home/alice/big")} end) || _ <- lists:seq(1, 8)],
    Same = [receive {P, {ok, B}} -> B =:= Big; {P, _} -> false end || P <- Readers],
    say("eight readers of ~s bytes at once: ~s", [byte_size(Big), lists:all(fun(X) -> X end, Same)]),
    % A rename the volume will not do, a directory into itself: refused by name, not as a
    % connection refused.
    ok = prim_file:make_dir("/home/alice/d"),
    ok = prim_file:make_dir("/home/alice/d/inner"),
    say("directory into itself: ~s", [prim_file:rename("/home/alice/d", "/home/alice/d/inner/d")]),
    say("bob's file: ~s", [prim_file:read_file("/home/bob/x")]),
    say("mode: ~s", [element(8, element(2, prim_file:read_file_info("/home/alice/big")))]),
    done.

%% Printed with ~s alone, as io_lib's ~p would need modules this disk does not carry. Each line
%% is what ~p would print for these shapes: atoms, integers, binaries of text, lists of strings,
%% and tuples of them.
say(Format, Args) -> io:format("files: " ++ Format ++ "~n", [show(A) || A <- Args]).

show(A) when is_atom(A) -> atom_to_list(A);
show(I) when is_integer(I) -> integer_to_list(I);
show(B) when is_binary(B) -> "<<\"" ++ escape(binary_to_list(B)) ++ "\">>";
show(T) when is_tuple(T) -> "{" ++ join([show(E) || E <- tuple_to_list(T)]) ++ "}";
show(L) when is_list(L) -> "[" ++ join(["\"" ++ S ++ "\"" || S <- L]) ++ "]".

escape(Cs) -> lists:flatmap(fun($\n) -> "\\n"; (C) -> [C] end, Cs).

join([]) -> "";
join([H | T]) -> H ++ lists:flatmap(fun(E) -> "," ++ E end, T).
