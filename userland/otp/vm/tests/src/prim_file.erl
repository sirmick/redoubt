%% Fixture for vm/tests/io_wait.rs: OTP's prim_file in miniature. Its NIF stub is called locally,
%% as OTP's own prim_file calls its NIFs, so the VM enters the native through the stub's replaced
%% body, not an external call. Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/prim_file.erl
-module(prim_file).
-export([read_file/1]).

read_file(Name) -> read_file_nif(Name).

read_file_nif(_Name) -> erlang:nif_error(undef).
