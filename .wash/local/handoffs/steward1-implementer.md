STEWARD1 handoff, full text: /home/mcloonan/redoubt/.wash/local/STEWARD1-handoff.md, including the 'Additions' section.

Branch wp-steward1, tip af9387c8b, on main ca5a6437b. The tree is clean; the private tag is removed.

Commits:
- 117eeb1b3: steward-gen Elixir backend (clean).
- 34f931bcb (WIP): trace crate and Elixir reference.
- c71328da8 (WIP): 13 hand traces, every row taken.
- af9387c8b (WIP): model recording.
Fold the three WIPs before acceptance.

Deliverables:
- Done: 1 toolchain (on main), 3 reference and skeletons, 4 hand and model traces, the host test of the comparison, the negative run (by hand).
- In progress: 2 trace format (the code is done, the steward.md '####' section is not).
- Not started: 5 bench case kind, run-traces, run-vectors fix, the pages, size budget, gates.

Architect's conditions: 1, 3, 4, 5 met; 2 (the case FAILS, never SKIPs, on a toolchain mismatch) still to do in the kind.

Encoding: the decisions and why are in the file.

First step: write libs/steward/elixir/run-traces from target/steward-dev.sh and target/steward-beamlet.sh, then the bench kind and the case.
