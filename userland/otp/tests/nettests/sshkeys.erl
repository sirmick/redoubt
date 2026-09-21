%% Host and user keys for the ssh test, from the key_cb option rather than files.
-module(sshkeys).
-behaviour(ssh_server_key_api).
-behaviour(ssh_client_key_api).
-export([host_key/2, is_auth_key/3, add_host_key/3, add_host_key/4, is_host_key/4, is_host_key/5, user_key/2]).

keys(Opts) -> proplists:get_value(key_cb_private, Opts, []).

host_key(Alg, Opts) ->
    case lists:keyfind(Alg, 1, keys(Opts)) of
        {Alg, Key} -> {ok, Key};
        false -> {error, no_host_key}
    end.
is_auth_key(_PublicKey, _User, _Opts) -> false.
add_host_key(_Host, _Key, _Opts) -> ok.
add_host_key(_Host, _Port, _Key, _Opts) -> ok.
is_host_key(_Key, _Host, _Alg, _Opts) -> true.
is_host_key(_Key, _Host, _Port, _Alg, _Opts) -> true.
user_key(_Alg, _Opts) -> {error, no_user_key}.
