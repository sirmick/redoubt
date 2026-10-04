# FSD3: the tests the old principal-domain count was holding up

I applied the ruling in `servers/init/src/confine.rs`. A shared server's principal users are now
only the domains with its own label set. The confinement check's line and both test listings in
init.md are as ruled. The new test, `confined_counts_only_a_shared_servers_own_label_set`, passes,
and so does `fsd-confined-labelled` on both widths. All of this is uncommitted.

## Tests whose verdict the change flips

All three have the same shape: an unlabelled shared server (keyd or consoled, which takes
`buckets=`) and a principal with the label set {alice-secrets}. The old count refused that shape;
the ruling now accepts it.

| Test | Expected | Now |
| --- | --- | --- |
| `host:redoubt-init::confined_refuses_a_driver_serving_two_label_sets` (only consoled is shared) | `Sharing::Device` | `Ok` |
| `host:redoubt-init::confined_refuses_a_server_instance_serving_two_label_sets` (only keyd is shared) | `Sharing::Server` | `Ok` |
| `bench:init-refuses-confined-server`, `tests/data/init/confined-server.json` (keyd shared) | refused at keyd, server instance | boots: `init: started keyd`, then the forbidden pattern matches (rv64) |

## Sharing::Server looks unreachable now

The check runs in this order: endpoint, volume, network, device, server instance. Under the new
rule, the only users that can differ from a server's own set are:

- servers handed one of its endpoints, which the endpoint check refuses first;
- servers attaching a volume at blkd, which the device check refuses first, since blkd holds the
  disk.

So no manifest seems to reach `Sharing::Server` any more. The same holds for a device whose driver
is used only through principals.

## Options

1. **Drop the two host tests.** Device stays covered by
   `confined_refuses_two_label_sets_on_one_disk`, and the new test covers the accepted shape.
2. **Re-aim init-refuses-confined-server.** Keep its kind of refusal in a boot, but at a sharing
   kind that is still reachable. For example: a labelled server handed an unlabelled server's
   endpoint, refused with "two label sets share an endpoint". Its description, expect line and
   manifest would change. `tests/init-*` is outside FSD3's owned paths, so this needs your grant.
3. **Sharing::Server** stays as a defensive kind with no test, or goes. That is the Architect's
   call.
