%% gen_tcp for beamlet: the kernel's gen_tcp chooses between BEAM's port driver and its socket
%% NIF; here every socket belongs to the one backend, beamlet_tcp, so this module is a thin
%% front. Sockets are `{'$inet', beamlet_tcp, Pid}`, so OTP's `inet` functions on them
%% (`setopts`, `peername`, ...) reach the same backend.
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(gen_tcp).
-export([connect/2, connect/3, connect/4, listen/2, accept/1, accept/2, shutdown/2, close/1,
         send/2, recv/2, recv/3, unrecv/2, controlling_process/2, fdopen/2]).

-define(SOCK(Mod, H), {'$inet', Mod, H}).

connect(#{addr := Addr, port := Port}, Opts) -> connect(Addr, Port, Opts, infinity).
connect(#{addr := Addr, port := Port}, Opts, Timeout) -> connect(Addr, Port, Opts, Timeout);
connect(Address, Port, Opts) -> connect(Address, Port, Opts, infinity).
connect(Address, Port, Opts, Timeout) -> beamlet_tcp:connect(Address, Port, Opts, Timeout).

listen(Port, Opts) -> beamlet_tcp:listen(Port, Opts).

accept(S) -> accept(S, infinity).
accept(?SOCK(Mod, _) = S, Timeout) -> Mod:accept(S, Timeout).

shutdown(?SOCK(Mod, _) = S, How) -> Mod:shutdown(S, How).
close(?SOCK(Mod, _) = S) -> Mod:close(S);
close(_) -> ok.

send(?SOCK(Mod, _) = S, Data) -> Mod:send(S, Data).
recv(S, Len) -> recv(S, Len, infinity).
recv(?SOCK(Mod, _) = S, Len, Timeout) -> Mod:recv(S, Len, Timeout).
unrecv(?SOCK(Mod, _) = S, Data) -> Mod:unrecv(S, Data).

controlling_process(?SOCK(Mod, _) = S, Pid) -> Mod:controlling_process(S, Pid).

%% There are no host file descriptors to adopt.
fdopen(_Fd, _Opts) -> {error, enotsup}.
