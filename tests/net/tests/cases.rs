//! Each net case's list of the box's own addresses is its `ipd`'s: every case that starts `ipd`
//! from a manifest of its own forbids exactly what that `ipd` refuses (its `self=` prefixes, then
//! the ones `ipd` refuses whatever its arguments), so a SYN the capture finds to one of them is a
//! failure of `ipd`, and one `ipd` refuses is never allowed by the bench. The manifest is read with
//! `init`'s own decoder and `ipd`'s arguments with `ipd`'s own parser, and every client's arguments
//! parse strictly, as they will on the machine.

use std::path::{Path, PathBuf};

use redoubt_init::manifest::{Manifest, Server};
use redoubt_ipd::scope::{Prefix, SelfSet};
use redoubt_net_client::{Args, IPD, JUDGE};
use redoubt_net_tests::{SELF_ALWAYS, SELF_ARGS};

fn prefix(text: &str) -> Prefix {
    let (addr, len) = text.split_once('/').unwrap();
    let octets: Vec<u8> = addr.split('.').map(|o| o.parse().unwrap()).collect();
    Prefix::new(u32::from_be_bytes(octets.try_into().unwrap()), len.parse().unwrap()).unwrap()
}

/// How a net case starts its network.
enum Network {
    /// `init`, from the case's own manifest.
    Manifest(Manifest),
    /// The rig, with its own `self=` list.
    Rig,
}

/// Every net case: its file name, its `[net]` table and how it starts its network. A net case is
/// one whose manifest starts the judge, or one booting the rig.
fn net_cases() -> Vec<(String, toml::Value, Network)> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(workspace.join("tests")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let case: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let net = case.get("net").cloned().unwrap_or(toml::Value::Table(Default::default()));
        let rig = case.get("programs").and_then(|p| p.as_array()).is_some_and(|programs| {
            programs
                .iter()
                .any(|p| p.get("bin").and_then(|b| b.as_str()).is_some_and(|b| b.starts_with("net-rig")))
        });
        if rig {
            cases.push((name, net, Network::Rig));
        } else if let Some(manifest) = manifest(&workspace, &case) {
            if manifest.servers.iter().any(|s| s.program == "net-judge") {
                cases.push((name, net, Network::Manifest(manifest)));
            }
        }
    }
    cases.sort_by(|a, b| a.0.cmp(&b.0));
    cases
}

/// The case's `manifest` file entry, decoded as `init` decodes it.
fn manifest(workspace: &Path, case: &toml::Value) -> Option<Manifest> {
    let files = case.get("file")?.as_array()?;
    let entry = files.iter().find(|f| f.get("name").and_then(|n| n.as_str()) == Some("manifest"))?;
    let path = entry.get("from")?.get("path")?.as_str()?;
    let bytes = std::fs::read(workspace.join(path)).unwrap();
    Some(redoubt_init::manifest::decode(&bytes).unwrap_or_else(|e| panic!("{path}: {e:?}")))
}

fn the<'a>(manifest: &'a Manifest, program: &str) -> &'a Server {
    let mut found = manifest.servers.iter().filter(|s| s.program == program);
    let server = found.next().unwrap_or_else(|| panic!("no {program}"));
    assert!(found.next().is_none(), "two {program}s");
    server
}

fn badge(server: &Server, endpoint: &str) -> Option<u64> {
    server.handed.iter().find(|h| h.endpoint == endpoint).map(|h| h.badge)
}

#[test]
fn every_net_case_forbids_exactly_its_ipds_own_addresses() {
    let cases = net_cases();
    assert!(cases.len() >= 8, "found only {:?}", cases.iter().map(|c| &c.0).collect::<Vec<_>>());
    for (name, net, network) in cases {
        let forbidden: Vec<Prefix> = net
            .get("self_forbidden")
            .unwrap_or_else(|| panic!("{name}: no self_forbidden"))
            .as_array()
            .unwrap()
            .iter()
            .map(|v| prefix(v.as_str().unwrap()))
            .collect();
        let own: Vec<Prefix> = match &network {
            Network::Manifest(manifest) => {
                let ipd = the(manifest, "ipd");
                let args = redoubt_ipd::args::parse(ipd.args.iter().map(String::as_str));
                args.unwrap_or_else(|e| panic!("{name}: ipd's arguments: {e:?}")).selfs
            }
            Network::Rig => SELF_ARGS.iter().map(|p| prefix(p)).collect(),
        };
        let expected: Vec<Prefix> = own.into_iter().chain(SELF_ALWAYS.iter().map(|p| prefix(p))).collect();
        assert_eq!(forbidden, expected, "{name}");
    }
}

/// Every client parses its arguments strictly, and holds a badge at `ipd` that `ipd`'s arguments
/// scope and the same badge at the judge, which is how the judge knows it.
#[test]
fn every_client_is_scoped_and_known_to_the_judge() {
    for (name, _, network) in net_cases() {
        let Network::Manifest(manifest) = network else { continue };
        let ipd = the(&manifest, "ipd");
        let config = redoubt_ipd::args::parse(ipd.args.iter().map(String::as_str)).unwrap();
        let judge = the(&manifest, "net-judge");
        let probe = badge(judge, IPD).unwrap_or_else(|| panic!("{name}: the judge has no badge at ipd"));
        assert!(config.scope_of(probe).is_some(), "{name}: the judge's badge {probe} has no scope");
        let clients: Vec<&Server> = manifest.servers.iter().filter(|s| s.program == "net-client").collect();
        assert!(!clients.is_empty(), "{name}: no client");
        for client in clients {
            let what = format!("{name}: {}", client.name);
            assert!(Args::parse(client.args.iter().map(String::as_str)).is_some(), "{what}: its arguments");
            let at_ipd = badge(client, IPD).unwrap_or_else(|| panic!("{what}: no badge at ipd"));
            assert!(config.scope_of(at_ipd).is_some(), "{what}: badge {at_ipd} has no scope");
            assert_eq!(badge(client, JUDGE), Some(at_ipd), "{what}: its badge at the judge");
        }
    }
}

/// `SELF_ALWAYS` is what `ipd` refuses without being told: each of its prefixes, first and last
/// address, is in the self set of an `ipd` given no `self=` at all, and nothing just outside is.
#[test]
fn the_always_list_is_ipds_own() {
    let addr = u32::from_be_bytes([10, 0, 2, 15]);
    let bare = SelfSet::new(addr, 24, &[]);
    for text in SELF_ALWAYS {
        let (base, len) = text.split_once('/').unwrap();
        let base = u32::from_be_bytes(
            base.split('.').map(|o| o.parse().unwrap()).collect::<Vec<u8>>().try_into().unwrap(),
        );
        let last = base | (u32::MAX.checked_shr(len.parse().unwrap()).unwrap_or(0));
        assert!(bare.contains(base) && bare.contains(last), "{text}");
    }
    assert!(
        !bare.contains(u32::from_be_bytes([1, 0, 0, 0]))
            && !bare.contains(u32::from_be_bytes([126, 255, 255, 255]))
    );
    // And the rig's arguments add exactly SELF_ARGS.
    let extra: Vec<Prefix> = SELF_ARGS.iter().map(|p| prefix(p)).collect();
    let rig = SelfSet::new(addr, 24, &extra);
    assert!(
        rig.contains(u32::from_be_bytes([10, 0, 9, 102]))
            && !bare.contains(u32::from_be_bytes([10, 0, 9, 102]))
    );
}
