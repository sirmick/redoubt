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
use redoubt_net_tests::SELF_ALWAYS;

fn prefix(text: &str) -> Prefix {
    let (addr, len) = text.split_once('/').unwrap();
    let octets: Vec<u8> = addr.split('.').map(|o| o.parse().unwrap()).collect();
    Prefix::new(u32::from_be_bytes(octets.try_into().unwrap()), len.parse().unwrap()).unwrap()
}

/// A net case: its file name, its `[net]` table, its manifest and what it must fail on, if it
/// must.
struct NetCase {
    name: String,
    net: toml::Value,
    manifest: Manifest,
    must_fail: Option<String>,
}

/// Every net case: one whose manifest starts the judge.
fn net_cases() -> Vec<NetCase> {
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
        let must_fail = case.get("must_fail").and_then(|m| m.as_str()).map(String::from);
        if let Some(manifest) = manifest(&workspace, &case) {
            if manifest.servers.iter().any(|s| s.program == "net-judge") {
                cases.push(NetCase { name, net, manifest, must_fail });
            }
        }
    }
    cases.sort_by(|a, b| a.name.cmp(&b.name));
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
    assert!(cases.len() >= 9, "found only {:?}", cases.iter().map(|c| &c.name).collect::<Vec<_>>());
    for NetCase { name, net, manifest, must_fail } in cases {
        let forbidden: Vec<Prefix> = net
            .get("self_forbidden")
            .unwrap_or_else(|| panic!("{name}: no self_forbidden"))
            .as_array()
            .unwrap()
            .iter()
            .map(|v| prefix(v.as_str().unwrap()))
            .collect();
        let ipd = the(&manifest, "ipd");
        let own = redoubt_ipd::args::parse(ipd.args.iter().map(String::as_str))
            .unwrap_or_else(|e| panic!("{name}: ipd's arguments: {e:?}"))
            .selfs;
        let expected: Vec<Prefix> = own.into_iter().chain(SELF_ALWAYS.iter().map(|p| prefix(p))).collect();
        // A self-check that its ipd misses one of the box's own addresses forbids that one too,
        // so the capture's catching the SYN is the failure it must show.
        if must_fail.is_some_and(|m| m.contains("one of the box.s own addresses")) {
            let missing: Vec<&Prefix> = forbidden.iter().filter(|p| !expected.contains(p)).collect();
            assert!(
                missing.len() == 1 && expected.iter().all(|p| forbidden.contains(p)),
                "{name}: {missing:?}"
            );
        } else {
            assert_eq!(forbidden, expected, "{name}");
        }
    }
}

/// Every client parses its arguments strictly, and holds a badge at `ipd` that `ipd`'s arguments
/// scope and the same badge at the judge, which is how the judge knows it.
#[test]
fn every_client_is_scoped_and_known_to_the_judge() {
    for NetCase { name, manifest, .. } in net_cases() {
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

/// The case that pokes `netd` sends exactly what `netd`'s test-only feature faults on.
#[test]
fn the_poke_is_netds() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../netd-restart.toml");
    let case: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let poke = &case["net"]["poke"];
    assert_eq!(poke["port"].as_integer(), Some(i64::from(redoubt_netd::restart_probe::PORT)));
    assert_eq!(poke["payload"].as_str().map(str::as_bytes), Some(redoubt_netd::restart_probe::PAYLOAD));
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
}
