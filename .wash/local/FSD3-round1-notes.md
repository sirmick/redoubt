# FSD3 round-1 review notes on 260a67022, for the third implementer

Verdicts: red BLOCK (one P1, one P2); simplifier OK with notes; editor OK with notes. Each
change goes into the commit that owns the code, no fix-ups. Red re-reviews the P1 fix.

## Red (BLOCK)

P1 (required) `servers/init/src/check.rs:263 volumes()` and `:327-343`: a `handed` item at
blkd's endpoint, e.g. `{"endpoint":"blkd","badge":"1"}`, passes the check: the badge only needs
to be below FIRST_MINTED_BADGE and not repeat another handed item. blkd resolves badge 1 to GPT
entry 0, the range init mints for the volume's fsd (init.rs:451), so a second server holds the
volume's raw range: breaks init.md's "at most one server attaches it" (R47) and "never used
twice at one endpoint"; with matching labels it reads and writes the raw blocks under a live
fsd, bypassing the attach-root subtree, the quota and the id check. Fix: `volumes()` refuses any
`handed` item at an endpoint a blkd receives on, plus a host refusal test; name the test under
R47 and the init checks.

P2 (required; RULED by the Architect): the code stands (the brief's rule 5 said "no whole-volume
*block* check"; the metadata walk was always meant to run at every start). fsd.md:415-417,
replace the paragraph with (rewrapped to the page's width; keep "**Open:** none." after it if
FSD3 wrote it):
> A restarted `fsd` mounts as it does at boot, with the same checks over every metadata pair,
> then serves; it reads no file's blocks first. A power cut leaves the volume consistent
> ([R50](#r50-power-loss-leaves-before-or-after)), and damage in a file's blocks is `corrupt`
> wherever a request meets it ([R49](#r49-a-hostile-medium-is-corrupt-not-a-crash)).
fsd-restart's description line:
> `fsd` is killed (a test feature triggered once) and `init` restarts it; the restarted `fsd`
> mounts with a boot's checks; an old connection's call gets `Dead`, and a fresh one reads the
> files.
P1's page line: append to init.md's Volumes bullet ("at most one server attaches it"): "and no
entry is handed a badge at the endpoint a `blkd` receives on."

Notes: fsd-reboot's capture groups are inert (the client compares /qids): drop them or use
them; a labelled volume naming an absent GPT entry stops blkd for every volume (BAD_ARGS), as
args.rs documents: fine.

## The predecessor's pending question a34b1956 (the tests the new count flips), answered

Granted: (a) drop host confined_refuses_a_driver_serving_two_label_sets (Device stays covered
by confined_refuses_two_label_sets_on_one_disk); (b) re-aim bench init-refuses-confined-server
(tests/data/init/confined-server.json + its toml) at a labelled server handed an unlabelled
endpoint, refused as Endpoint, with its description saying what it refuses now; host
confined_refuses_a_server_instance_serving_two_label_sets is NOT dropped: it is re-aimed per the
Architect's Sharing::Server ruling below (or deleted with the kind if unreachable). The dropped
and re-aimed names leave the confinement check's and R34's lists; the counts follow. Each commit
passes its tests; the WIP (95f93d935) is folded into final commits.

## Architect: Sharing::Server (after the confined-users ruling)

Keep it, with a test, if a server can still reach it: a confined manifest with a `blkd` entry
that has no `devices`, labelled {}, and a {7} fsd attaching a {7} volume passes Endpoint (the
range badge is not a handed item), Volume (the sets agree) and Device (no devices), and is
refused as Server. Re-aim host:redoubt-init::confined_refuses_a_server_instance_serving_two_label_sets
at that manifest, expecting Server at blkd's servers[i] (so that test is NOT dropped after all;
the other dropped host test and the re-aimed bench case stand). No page change.
If init refuses a device-less blkd earlier for another reason, the kind is unreachable: delete
it with its test and refusal.rs's string, and in init.md: delete at :165 "- a **server
instance**: one `servers` entry serving both."; after "The kinds are checked in the order
listed, and the refusal names the kind." insert "A server instance two sets share is refused as
one of these: each of its users holds one of its endpoints or attaches a volume at its device,
and its principal users carry its own set."; R34's text (:680) and the tenets keep "server
instance". The dropped/re-aimed tests' names leave the confinement check's and R34's lists and
the counts follow.

## Orchestrator

- fsd-label-check (d8f38c3d9) orders its verdicts with sleeps (the outsider waits 3 s, the
  writer's read-back 6 s) and ordered expect lines. A timing-ordered case is a flake waiting for
  a slow host: if the writer can be made to wait on an event (the outsider's refusal line on the
  console, or a second start after the outsider exits, as INIT3's cases do with init's restart),
  do that; else state the margin and why it holds under icount.
- The unlabelled client is refused at attach, so walk/open/stat/write cannot be attempted
  separately: the case and the commit say so; the Architect sees it at the merge check.

## Editor (OK with notes)

1. blkd.md's "labels.P=ID,... which init gives blkd" lands in 711330611, but init passes them
   only from c89781164: move the line to the commit that makes it true.
2. docs/testbench.md "Disks and network cards" list: two host: lines were inserted between
   bench: lines (after bench:image-disk, before bench:net-tcp); group by kind like the rest.
3. docs/testbench.md: one line ~103 columns ("server is announced as `init: restarted NAME...",
   26fb424fd): rewrap.
4. Pending (yours anyway): fsd.md's status names bench:fsd-labelled-volume: rename to
   fsd-label-check with the ruled shape; serving.md's R25 status line; init.md's confined-users
   text. The Q1/Q2 ruling file is `.wash/local/FSD2-gaps-ruling.md`? No: FSD3's Q1/Q2 are in my
   instruction 4a8c7af4 (the exact init.md and fsd.md lines); check the init.md example args
   (endpoint=, buckets=4; the handed blkd dropped) against it.
5. For the Architect at the merge: the corrupt-volume text "fsd says so on its console, and it
   stays up" (fsd.md Mounting, 3234a5a8a) is the implementer's own wording.
6. fsd.md's Volumes list carries fsd-reboot and image-disk beyond the brief's five: both exist,
   fine.

## Simplifier (OK with notes)

Take, unless a reason says otherwise in the report:

1. `tests/data/fsd/{boot,corrupt,quota,reboot,restart}.json` are 48 lines each and differ only
   in the client's args (line 45). One manifest if the case file can set the client's args (a
   per-case `args` override on a program entry, if case.rs has or can cheaply gain one); else
   keep them and say why. About -190 lines.
2. `tests/data/fsd/image.json` (194 lines) is `image/manifest.json` plus a 23-line client entry.
   Every change to the image's manifest would be made twice, and `image-disk` stops testing the
   real image. Prefer a case-level addition over `image/manifest.json` (the case names the
   image's manifest and adds its client); else a host test that the two agree minus the client.
   About -170 lines.
3. `fsd-client`: eight modes plus labelled/reader on a second dispatch (`labelled_run`, its own
   `read_all`) beside `read_file`: merge the two readers, about -25 lines. Keep `read`
   (image-disk uses it).

Not duplicates (keep): blkd's `labels.P=` parsing (the check is the skeleton's); `pack.rs` as
the one disk-format path (disk.rs calls `fsd::pack` and blkd's Image; `gpt_disk` moved whole);
the restart on init's generic path; the reporter reset on the loader line. The
`one-volume-probe` feature (test code inside the shipped fsd, off by default) is acceptable.
