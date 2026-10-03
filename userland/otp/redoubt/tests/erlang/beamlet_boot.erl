%% beamlet-boot (tests/beamlet-boot.toml): the VM, started by init, writes one line to its
%% console and returns ok.
-module(beamlet_boot).
-export([start/0]).

start() ->
    io:put_chars("beamlet-boot: hello from the VM\n"),
    ok.
