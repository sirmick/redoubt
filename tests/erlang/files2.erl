%% File times, permissions and links through OTP's file module.
-module(files2).
-export([start/0]).
-include_lib("kernel/include/file.hrl").

start() ->
    ok = file:write_file("a.txt", <<"data">>),
    T = {{2020, 1, 2}, {3, 4, 5}},
    ok = file:change_time("a.txt", T, T),
    {ok, #file_info{mtime = M, atime = A}} = file:read_file_info("a.txt", [{time, universal}]),
    ok = file:change_mode("a.txt", 8#600),
    {ok, #file_info{mode = Mode}} = file:read_file_info("a.txt"),
    ok = file:make_symlink("a.txt", "link"),
    {ok, Target} = file:read_link("link"),
    {ok, #file_info{type = LT}} = file:read_link_info("link"),
    {ok, Via} = file:read_file("link"),
    ok = file:make_link("a.txt", "hard"),
    {ok, #file_info{links = Links}} = file:read_file_info("a.txt"),
    Dangling = file:make_symlink("nowhere", "dangling"),
    DI = file:read_file_info("dangling"),
    {ok, #file_info{type = DT}} = file:read_link_info("dangling"),
    {M, A, Mode band 8#777, Target, LT, Via, Links, Dangling, DI, DT}.
