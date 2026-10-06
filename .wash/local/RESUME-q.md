# Resume: the machine is scheduled by `q` now (2026-10-06)

The make/fifo jobserver is gone. Never run `jobserver`, never `make -j`, never a bare `cargo`
or `./build` or `cargo testbench`: every build, test and QEMU run goes through `q`, which
leases real cores and pins the command to them, or through `jobs.mk`, which does it for you.

    q=/home/mcloonan/redoubt/scripts/q
    $q run --cores 8 -- ./build --arch rv64 --programs          # a build: 8 cores
    $q run --cores 8 -- cargo build -p <crate>                  # cargo gets -j from the lease
    $q run --cores 4 -- cargo test -p <crate>                   # host tests: 4 (RUST_TEST_THREADS=4)
    $q run --quiet  -- cargo test -p redoubt-rt                 # a host-CLOCK crate (rt, client,
                                                                #  r4 = keyd/consoled/bootfsd, miri):
                                                                #  the quiet core set, one at a time
    make -k -f /home/mcloonan/redoubt/scripts/jobs.mk -C <your worktree> rv64/<case> rv32/<case>
                                                                # a bench case: the class is picked
                                                                #  for you (cores = the case's smp;
                                                                #  [net] cases take a lock; host-clock
                                                                #  cases go quiet); no -j, ever
    $q ls                                                       # the core map, running, waiting
    $q log 30                                                   # recent jobs with waited/ran times

`q run` blocks until the command ends and returns its exit code; it works in the background
(`... &`, setsid, nohup) exactly as before. Killing the `q run` process frees its cores at once.
Your environment exports (RUSTSBI_PROTOTYPER*, BEAMLET_TOOLCHAINS, ~/.cargo/bin) are unchanged.

Verdicts under sharing: a case that fails beside other work is rerun through `--quiet` before
it counts (`make -f jobs.mk ... rv64/<case>` already does that for the host-clock class; for any
other case use `$q run --quiet -- cargo testbench --arch rv64 <case>`), and the report says so.
The whole bench (train 4) is running now; its case phase took minutes, not hours. Resume your
work where your handoff left it.
