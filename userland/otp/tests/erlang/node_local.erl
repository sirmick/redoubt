-module(node_local).
-export([start/0]).
%% node/0,1 on a VM with no distribution, for this node's own pids and refs: the half of
%% atomvm/test_node that needs no other node (that test is skipped: it decodes another node's
%% pid, ref and port).
start() ->
    Badarg = fun(F) -> try F() catch error:badarg -> badarg end end,
    {node(), node(self()), node(make_ref()), node(spawn(fun() -> ok end)),
     Badarg(fun() -> node(test) end), Badarg(fun() -> node({test, nonode@nohost}) end)}.
