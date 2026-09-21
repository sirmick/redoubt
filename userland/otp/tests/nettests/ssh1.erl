-module(ssh1).
-export([start/0]).
%% An SSH daemon and client in one VM: key exchange, host key signature, password
%% authentication, a session channel and an exec request; then the algorithms negotiated.
start() ->
    {ok, _} = application:ensure_all_started(ssh),
    HostKeys = [{'ssh-ed25519', public_key:generate_key({namedCurve, ed25519})},
                {'ecdsa-sha2-nistp256', public_key:generate_key({namedCurve, secp256r1})}],
    {ok, D} = ssh:daemon(0, [{key_cb, {sshkeys, HostKeys}},
                             {user_passwords, [{"alice", "secret"}]},
                             {exec, {direct, fun(Cmd) -> {ok, "echo: " ++ Cmd} end}},
                             {subsystems, []}]),
    [{port, Port}] = ssh:daemon_info(D, [port]),
    [session(Port, Prefs) || Prefs <- [[], [{public_key, ['ecdsa-sha2-nistp256']}, {cipher, ['aes256-ctr']}, {mac, ['hmac-sha2-256']}, {kex, ['ecdh-sha2-nistp256']}], [{cipher, ['chacha20-poly1305@openssh.com']}]]]
        ++ [wrong_password(Port)].

session(Port, Prefs) ->
    {ok, C} = ssh:connect("localhost", Port, [{user, "alice"}, {password, "secret"},
                                              {silently_accept_hosts, true}, {user_interaction, false},
                                              {save_accepted_host, false}, {key_cb, {sshkeys, []}},
                                              {preferred_algorithms, fill(Prefs)}], 10000),
    {ok, Ch} = ssh_connection:session_channel(C, 10000),
    success = ssh_connection:exec(C, Ch, "hello", 10000),
    Out = receive {ssh_cm, C, {data, Ch, 0, Data}} -> Data after 10000 -> timeout end,
    [{algorithms, Algs}] = ssh:connection_info(C, [algorithms]),
    ssh:close(C),
    {Out, lists:sort([{K, V} || {K, V} <- Algs, lists:member(K, [kex, hkey, encrypt, send_mac])])}.

%% Unset preference classes keep their defaults.
fill(Prefs) ->
    Defaults = ssh:default_algorithms(),
    [case lists:keyfind(Class, 1, Prefs) of
         {Class, [A]} when Class =:= cipher; Class =:= mac -> {Class, [{client2server, [A]}, {server2client, [A]}]};
         {Class, L} -> {Class, L};
         false -> {Class, V}
     end || {Class, V} <- Defaults].

wrong_password(Port) ->
    ssh:connect("localhost", Port, [{user, "alice"}, {password, "wrong"}, {silently_accept_hosts, true},
                                    {user_interaction, false}, {save_accepted_host, false},
                                    {key_cb, {sshkeys, []}}], 10000).
