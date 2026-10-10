# B27 report

Branch wp-B27, one commit 5eeae0c8b on main 91256b4d0, never pushed.

## Cause: two shapes, one rule

K23's init made the steward's entry a fresh connection (`new_connection`) at every endpoint it is
handed, and refused the boot if one could not be made. Two shapes make none.

1. **A tester in the steward's place, handed a server that mints no connections.** The BEAM4
   cases' manifests (tests/data/beamlet/{launch,natives,natives-attack,serve}.json) hand the
   tester `keyd` (badge 1) as well as bootfsd, erofsd:system and littlefsd:data. `keyd` answers
   no `new_connection`, so beamlet-launch, -natives, -serve and -natives-attack were refused at
   boot.
2. **The real steward, at a server that refuses.** verity-wrong-root and verity-flipped-tree
   break the system volume on purpose. erofsd serves it as corrupt and refuses the steward's
   connection, and init refused the boot ("steward: could not make a fresh connection for"). The
   cases expect the steward to run and its sessions' Connect to fail.

## Decision: a failed fresh connection does not refuse the boot

The fresh connection is not a condition of the steward's start. It exists only so that a dead
steward's session connections, minted under it, can be released at its exit.

A server that makes none is handed the badge, as every server is, and the steward meets that
server's answers exactly as it did before K23. That covers three cases: it mints none (`keyd`),
it refuses (a corrupt volume), or it does not answer within RELEASE_TIMEOUT (1 s).

The cost: that one server is the only place a dead steward's session connections could outlive
it, and only if the server later serves sessions. init.md says so.

The manifests were not wrong: any endpoint may be handed to the steward's entry. A tester in the
steward's place gets fresh connections where its servers make them, as the steward does.

An earlier version of this commit kept a hand-written list of the programs that speak
ninep_common. It is dropped: it covered only shape 1, and a future 9P server left off it would
have leaked again. Now the server's own answer decides.

## Pages

init.md, "Fresh connections per child": the badge fallback, its three triggers, and its one cost.

## Gates (through q, logs in /var/tmp/redoubt/K23/B27)

- `cargo test -p redoubt-init`: 0.
- prebuilt: 0.
- Each of these exited 0 on both rv64 and rv32: verity-wrong-root, verity-flipped-tree,
  beamlet-launch, beamlet-natives, beamlet-serve, beamlet-natives-attack, steward-restart (13
  restarts), steward-restart-reboot, init-restart.
- docs 0, formatting 0, size-budget 0 after raising servers/init 2,413 → 2,416 (`Size budget:`
  line in the commit).
- No host-clock failures to rerun.

## The red's notes (OK with notes at 5eeae0c8b), folded

The head is wp-B27 38ecfe7b9.

- **P2:** on the fallback, init prints
  `init: <endpoint> made the steward no fresh connection: the badge is handed`.
  - Every case's `forbid` patterns were checked against this line; none matches.
  - init.md and the commit message mention the line.
  - The BEAM4 tester prints it once per boot, for keyd.
- **Nit:** the code comment now says "does not answer within a second", matching the page.

Gates (through q), each exit 0:

- init host tests.
- prebuilt.
- verity-wrong-root on rv64 and rv32; its console now carries the line for erofsd:system.
- beamlet-launch on rv64 and rv32.
- docs, formatting.
- size-budget, after raising servers/init 2,416 → 2,422.
