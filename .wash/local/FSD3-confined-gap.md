# FSD3: no confined boot with a labelled volume can start its fsd

These candidates were checked against init's own check with a throwaway host test, since removed.
The base is fsd-boot's manifest, confined, with label alice-secrets (7) on the volume, blkd, fsd
and the client.

| Variant | init's check |
| --- | --- |
| v1: alice with no labels and no label sets | refused at servers[3] (fsd), sharing Server |
| v2: alice owns alice-secrets | refused at servers[3] (fsd), sharing Server |
| v3: alice has the label set {alice-secrets} | refused at servers[1].devices (consoled), sharing Device |
| v4: no principals | refused at labels[0].owner, Unknown |
| v5: v1 with fsd's buckets= removed | accepted, but fsd exits BAD_ARGS before serving |

Why:
- fsd requires buckets= (fsd.md, Arguments), so it is a shared server.
- init.md says a shared server's users are every principal domain. Every principal has the base
  domain {}, which differs from fsd's {7}.
- keyd and consoled also require buckets=, so a {7} label set refuses them as well (v3).
- A label needs an owning principal (v4).

Options:
1. init.md: a labelled shared server's users are only the principal domains whose label set equals
   its own. The steward can only grant a connection there to such a domain. This changes init's
   check. Recommended.
2. fsd: buckets= becomes optional; without it, fsd serves only its manifest callers, with fixed
   admission. This reverses fsd.md's "required, never defaulted".
3. The confined case shows init refusing the boot, and the gap is recorded as a residual with a
   follow-up.
