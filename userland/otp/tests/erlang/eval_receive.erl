-module(eval_receive).
-export([start/0]).
%% A `receive` evaluated by erl_eval, as one typed at a shell's prompt: erl_eval hands each
%% message to a match fun through prim_eval:'receive'/2, which scans the mailbox itself.
start() ->
    {eval(), cursor(), collected()}.

eval() ->
    self() ! {a, 1},
    self() ! {b, 2},
    %% The second message matches; the first stays.
    R1 = ev("receive {b, X} -> X end."),
    %% Nothing matches: the `after` clause, at once and after a wait.
    R2 = ev("receive {c, X} -> X after 0 -> none end."),
    R3 = ev("receive {c, X} -> X after 20 -> late end."),
    %% A guard rejects the first message's clause, then another clause takes it.
    R4 = ev("receive {a, X} when X > 5 -> big; {a, X} -> {small, X} end."),
    %% No `after`: the receive waits for a message another process sends.
    Me = self(),
    spawn(fun() -> receive after 10 -> ok end, Me ! {c, 3} end),
    R5 = ev("receive {c, X} -> X * 10 end."),
    %% A binding from before the receive is matched, not bound again.
    self() ! {d, 1},
    self() ! {d, 2},
    R6 = ev("Y = 2, receive {d, Y} -> {got, Y} end."),
    R7 = ev("receive Any -> Any after 0 -> empty end."),
    R8 = ev("receive Any -> Any after 0 -> empty end."),
    {R1, R2, R3, R4, R5, R6, R7, R8}.

%% prim_eval:'receive'/2 called directly, with funs erl_eval never makes: the scan position
%% after the fun raises, and a fun that receives itself.
cursor() ->
    self() ! a,
    self() ! b,
    C1 = (catch prim_eval:'receive'(fun(a) -> nomatch; (b) -> error(boom) end, 0)),
    C2 = receive X -> X after 0 -> none end,
    C3 = receive Y -> Y after 0 -> none end,
    self() ! p,
    self() ! q,
    self() ! r,
    C4 = prim_eval:'receive'(
        fun(p) -> nomatch;
           (q) -> receive r -> {inner, r} after 0 -> {inner, none} end;
           (M) -> {outer, M}
        end,
        0),
    C5 = receive Z -> Z after 0 -> none end,
    C6 = receive W -> W after 0 -> none end,
    C7 = prim_eval:'receive'(fun(_) -> nomatch end, 0),
    {element(1, C1), C2, C3, C4, C5, C6, C7}.

%% A collection while the scan is open: the fun allocates heavily and collects before it answers
%% nomatch, so the messages it has looked at, and the one it takes, must survive the move.
collected() ->
    [self() ! {big, N, lists:seq(1, 20000 + N)} || N <- [1, 2, 3, 4]],
    Churn = fun(M) ->
        _ = lists:reverse(lists:seq(1, 50000)),
        _ = [{X, [X]} || X <- lists:seq(1, 20000)],
        true = erlang:garbage_collect(),
        case M of
            {big, 3, L} -> {took, 3, length(L), lists:sum(L)};
            _ -> nomatch
        end
    end,
    Took = prim_eval:'receive'(Churn, 0),
    Rest = [receive {big, N, L} -> {N, length(L), lists:sum(L)} after 0 -> none end || _ <- [1, 2, 3]],
    {Took, Rest}.

ev(S) ->
    {ok, Ts, _} = erl_scan:string(S),
    {ok, Es} = erl_parse:parse_exprs(Ts),
    {value, V, _} = erl_eval:exprs(Es, erl_eval:new_bindings()),
    V.
