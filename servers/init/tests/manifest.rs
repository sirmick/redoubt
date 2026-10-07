//! The manifest's checks, one refusal at a time (servers/init.md, "The boot manifest", "The
//! confinement check", "Starting the servers", "The key-separation check"): each test changes the
//! image's own manifest (`image/manifest.json`) in one way and expects the refusal for that rule
//! and no other.

use redoubt_init::check::{MANIFEST, Machine, Plan, STEWARD_SLOTS, args, is_steward};
use redoubt_init::fuzz::{BUNDLE_KEY, ENTRIES, machine, virt_devices};
use redoubt_init::manifest::{
    Budget, Device, DeviceUse, Handed, Label, LabelSet, Net, Principal, Server, Sizes, Steward, Verity,
    Volume,
};
use redoubt_init::refusal::{Refusal, Sharing, Why};
use redoubt_init::{ARENA_PAGES, Manifest, check, read};
use redoubt_rt::abi::MAX_START_HANDLES;
use redoubt_rt::wire::json::{SchemaError, SchemaKind};
use stub::MAX_STACK_PAGES;

const IMAGE: &str = include_str!("../../../image/manifest.json");

/// An OpenSSH Ed25519 key whose 32 bytes are 1 to 32.
const LOGIN_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8g";

/// The image's manifest whole: its servers, the steward and its principals.
fn full_image() -> Manifest { read(IMAGE.as_bytes(), ARENA_PAGES).expect("the image's manifest decodes") }

/// The image's servers and devices without the steward and `sshd`, which calls it, its
/// principals, labels, alice's labelled volume and its `littlefsd`, and console: the base each test
/// below changes in one way.
fn image() -> Manifest {
    let mut m = full_image();
    m.principals.clear();
    m.labels.clear();
    m.steward = None;
    m.console = None;
    m.volumes.retain(|v| v.labels.is_empty());
    m.servers.retain(|s| !["steward", "sshd", "littlefsd:alice-secrets"].contains(&s.name.as_str()));
    m
}

/// The image's manifest with one disk: without the userland disk (`disk1`, its `blkd`, its
/// volume, its verifier and `erofsd`, and `beamlet`, which reads it).
fn without_userland() -> Manifest {
    let mut m = image();
    m.volumes.retain(|v| v.name != "system");
    m.servers
        .retain(|s| !["beamlet", "blkd:system", "verity:system", "erofsd:system"].contains(&s.name.as_str()));
    m.devices.retain(|d| d.name != "disk1");
    m
}

/// The image's manifest with one disk and no volume, for tests that lay out volumes of their own.
fn without_volumes() -> Manifest {
    let mut m = without_userland();
    m.volumes.clear();
    m.servers.retain(|s| s.volume.is_none());
    m
}

fn on_virt(m: &Manifest) -> Result<Plan, Refusal> {
    let devices = virt_devices();
    check(m, &machine(&devices, &ENTRIES), BUNDLE_KEY)
}

fn on(m: &Manifest, machine: &Machine) -> Result<Plan, Refusal> { check(m, machine, BUNDLE_KEY) }

fn refused_at(m: &Manifest, path: &str, why: Why) {
    assert_eq!(on_virt(m).unwrap_err(), Refusal::At { at: path.into(), why });
}

fn refused_at_on(m: &Manifest, machine: &Machine, path: &str, why: Why) {
    assert_eq!(on(m, machine).unwrap_err(), Refusal::At { at: path.into(), why });
}

fn server<'a>(m: &'a mut Manifest, name: &str) -> &'a mut Server {
    m.servers.iter_mut().find(|s| s.name == name).unwrap()
}

fn budget(pages: u64) -> Budget { Budget { pages, processes: 1, weight: 100 } }

fn alice() -> Principal {
    Principal {
        name: "alice".into(),
        account: 1001,
        budget: budget(4096),
        ssh_keys: vec![LOGIN_KEY.into()],
        approval_keys: vec![],
        labels: vec![],
        label_sets: vec![],
        home: None,
        net: vec![],
    }
}

fn secrets(m: &mut Manifest) {
    m.labels.push(Label { name: "alice-secrets".into(), owner: "alice".into(), id: 7 });
}

#[test]
fn the_image_manifest_passes_and_its_plan_is_what_the_boot_follows() {
    let plan = on_virt(&full_image()).unwrap();
    let h = |n| redoubt_rt::abi::Handle::new(n).unwrap();
    // virt's handles: the console in 5 and 6, slots 0x1000_1000 on from 7, their interrupts
    // (1 to 8) from 15.
    assert_eq!(plan.placements[1], vec![("uart".into(), h(5)), ("uart-irq".into(), h(6))]);
    assert_eq!(plan.placements[3], vec![("disk".into(), h(14)), ("disk-irq".into(), h(22))]);
    assert_eq!(plan.placements[4], vec![("net".into(), h(13)), ("net-irq".into(), h(21))]);
    // The userland disk's blkd: slot 0x1000_6000, interrupt 6.
    assert_eq!(plan.placements[8], vec![("disk".into(), h(12)), ("disk-irq".into(), h(20))]);
    assert!(plan.placements[0].is_empty());
    // Alice's and bob's login keys, then the bundle key.
    let asked: Vec<&str> = plan.keys.iter().map(|(at, _)| at.as_str()).collect();
    assert_eq!(asked, ["principals[0].ssh_keys[0]", "principals[1].ssh_keys[0]", "bundle key"]);
    // Three domains (alice's {} and {alice-secrets}, bob's {}) at every shared server, beside its
    // system callers: init and sshd at keyd; init at consoled; init and the steward at bootfsd;
    // netd, the steward and sshd at ipd; the steward at walfsd:data, littlefsd:alice-secrets and
    // erofsd.
    assert_eq!(plan.buckets, vec![(0, 5), (1, 4), (2, 5), (5, 6), (6, 4), (7, 4), (10, 4)]);
    // No handed item names consoled or bootfsd: init's own badge at each is 1. sshd is handed
    // keyd's 1, the host key's, so init's there is 2.
    assert_eq!(plan.init_badges, vec![(0, 2), (1, 1), (2, 1)]);
}

#[test]
fn a_server_stack_defaults_and_is_checked_against_its_budget() {
    let absent =
        br#"{"servers":[{"name":"x","program":"x","budget":{"pages":"32","processes":1,"weight":1}}]}"#;
    assert_eq!(read(absent, ARENA_PAGES).unwrap().servers[0].stack_pages, 16);
    let mut m = image();
    for pages in [0, MAX_STACK_PAGES as u64 + 1, m.servers[0].budget.pages] {
        m.servers[0].stack_pages = pages;
        refused_at(&m, "servers[0].stack_pages", Why::Stack);
    }
    m.servers[0].stack_pages = MAX_STACK_PAGES as u64;
    assert!(on_virt(&m).is_ok());
    m.servers[0].budget.pages = 32;
    m.servers[0].stack_pages = 32;
    refused_at(&m, "servers[0].stack_pages", Why::Stack);
}

#[test]
fn a_server_heap_cap_is_optional_and_fits_its_budget_beside_its_stack() {
    let limits = r#""budget":{"pages":"32","processes":1,"weight":1}"#;
    let read_one = |rest: &str| {
        let json = format!(r#"{{"servers":[{{"name":"x","program":"x",{limits}{rest}}}]}}"#);
        read(json.as_bytes(), ARENA_PAGES).map(|m| m.servers[0].heap_pages)
    };
    assert_eq!(read_one(""), Ok(None));
    assert_eq!(read_one(r#","heap_pages":"12""#), Ok(Some(12)));
    assert!(read_one(r#","heap_pages":12"#).is_err(), "a decimal string, as stack_pages");
    // The startup block carries the cap as a `u32`: past it the manifest does not decode.
    assert_eq!(read_one(r#","heap_pages":"4294967295""#), Ok(Some(u32::MAX)));
    assert_eq!(
        read_one(r#","heap_pages":"4294967296""#),
        Err(Refusal::Schema(SchemaError {
            path: "servers[0].heap_pages".into(),
            kind: SchemaKind::WrongType
        }))
    );
    let mut m = image();
    let (budget, stack) = (m.servers[0].budget.pages as u32, m.servers[0].stack_pages as u32);
    for pages in [0, budget - stack, budget] {
        m.servers[0].heap_pages = Some(pages);
        refused_at(&m, "servers[0].heap_pages", Why::Heap);
    }
    m.servers[0].heap_pages = Some(budget - stack - 1);
    assert!(on_virt(&m).is_ok());
    m.servers[0].heap_pages = None;
    assert!(on_virt(&m).is_ok());
}

#[test]
fn the_bound_uses_the_largest_declared_stack_batch() {
    let mut m = image();
    for server in &mut m.servers {
        server.stack_pages = 16;
    }
    let base = on_virt(&m).unwrap().bound;
    server(&mut m, "keyd").stack_pages = 32;
    assert_eq!(on_virt(&m).unwrap().bound, base + 16);
    server(&mut m, "consoled").stack_pages = 48;
    assert_eq!(on_virt(&m).unwrap().bound, base + 32);
    server(&mut m, "keyd").stack_pages = MAX_STACK_PAGES as u64;
    assert_eq!(on_virt(&m).unwrap().bound, base + 48);
}

#[test]
fn init_s_own_badge_is_the_smallest_no_handed_item_uses_there() {
    let mut m = without_volumes();
    for (holder, badge) in [("ipd", 1), ("netd", 2), ("blkd", 4)] {
        server(&mut m, holder).handed.push(Handed { endpoint: "bootfsd".into(), badge });
    }
    server(&mut m, "consoled").handed.push(Handed { endpoint: "keyd".into(), badge: 1 });
    let plan = on_virt(&m).unwrap();
    assert_eq!(plan.init_badges, vec![(0, 2), (1, 1), (2, 3)]);
}

#[test]
fn the_image_manifest_s_bound() {
    let plan = on_virt(&image()).unwrap();
    // The arena (256 + 3 tables), 10 receive and 10 exit endpoints and init's reports endpoint,
    // 10 process objects, 10 blocks with 3 tables each, 10 watching threads (an IPC page, 4 stack
    // pages and 3 tables each), one launch (stub 4 + 3, one 64-page batch of ipd's image + 3,
    // the largest stack, walfsd:data's 14 + 3), the lend (2 + 3), and one handle-table page: 22
    // handles at the start (3 budgets, the Reset right, 18 devices) and 10 + 5 + 40 + 3 + 1 = 59
    // added (the three volume ranges, walfsd:data's, erofsd:system's at verity:system and
    // verity:system's at blkd:system, among the 5 badges) pass page 0's 64.
    let devices = virt_devices();
    let m = machine(&devices, &ENTRIES);
    assert_eq!(m.handles_at_start, 22);
    assert_eq!(plan.bound, 259 + 21 + 10 + 40 + 80 + (4 + 3 + 64 + 3 + 14 + 3) + 1 + 5);
}

/// A volume's range badge is a handle `init` mints, as a `handed` item is: with the handle table
/// full to a page's edge, either one opens the next page.
#[test]
fn a_volume_s_range_badge_counts_in_the_bound_as_a_handed_item_does() {
    let mut m = without_volumes();
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: vec![], disk: None, verity: None });
    let bound = |m: &Manifest| on_virt(m).unwrap().bound;
    let mut handed = m.clone();
    server(&mut handed, "ipd").handed.push(Handed { endpoint: "bootfsd".into(), badge: 1 });
    let mut attached = m.clone();
    attached.servers[0].volume = Some("data".into());
    // Each endpoint more takes a handle, until the next one opens a page.
    let mut n = 0;
    while bound(&handed) == bound(&m) {
        for m in [&mut m, &mut handed, &mut attached] {
            server(m, "bootfsd").receives.push(format!("e{n}"));
        }
        n += 1;
    }
    assert_eq!(bound(&attached), bound(&handed));
}

// ---- decoding: strict JSON, types, members ----

#[test]
fn what_is_not_strict_json_is_refused_with_where() {
    assert!(matches!(read(b"{ \"servers\": [ }", ARENA_PAGES), Err(Refusal::Json(_))));
    assert!(matches!(read(b"{ \"public\": [], \"public\": [] }", ARENA_PAGES), Err(Refusal::Json(_))));
    let schema = |text: &str| match read(text.as_bytes(), ARENA_PAGES) {
        Err(Refusal::Schema(e)) => (e.path, e.kind),
        other => panic!("{other:?}"),
    };
    assert_eq!(schema(r#"{ "confined": 1 }"#), ("confined".into(), SchemaKind::WrongType));
    assert_eq!(schema(r#"{ "system": {} }"#), ("system".into(), SchemaKind::Unknown));
    assert_eq!(
        schema(
            r#"{ "servers": [ { "name": "a", "program": "a", "budget": { "pages": 4, "processes": 1, "weight": 1 } } ] }"#
        ),
        ("servers[0].budget.pages".into(), SchemaKind::WrongType)
    );
    assert_eq!(
        schema(r#"{ "devices": [ { "name": "a", "dma": false } ] }"#),
        ("devices[0].base".into(), SchemaKind::Missing)
    );
    assert_eq!(
        schema(
            r#"{ "servers": [ { "name": "a", "program": "a", "budget": { "pages": "4", "processes": 1, "weight": 1 }, "budgets": ["system"] } ] }"#
        ),
        ("servers[0].budgets".into(), SchemaKind::Unknown)
    );
}

#[test]
fn a_manifest_the_arena_cannot_parse_is_refused_before_parsing() {
    let longest = redoubt_init::max_manifest(ARENA_PAGES);
    let mut text = vec![b' '; longest + 1];
    text[0] = b'{';
    text[longest] = b'}';
    assert_eq!(read(&text, ARENA_PAGES), Err(Refusal::Arena { len: longest + 1 }));
    text.pop();
    text[longest - 1] = b'}';
    assert!(read(&text, ARENA_PAGES).is_ok());
}

// ---- names and references ----

#[test]
fn names_follow_the_rule_and_differ() {
    let mut m = image();
    m.servers[2].name = "Bootfsd".into();
    refused_at(&m, "servers[2].name", Why::NotAName);
    let mut m = image();
    m.servers[2].name = "keyd".into();
    refused_at(&m, "servers[2].name", Why::Twice);
    let mut m = image();
    server(&mut m, "ipd").receives.push("consoled".into());
    refused_at(&m, "servers[5].receives[1]", Why::ReceivedTwice);
    let mut m = image();
    server(&mut m, "netd").handed.push(Handed { endpoint: "netd".into(), badge: 9 });
    refused_at(&m, "servers[4].handed[1].endpoint", Why::Twice);
}

#[test]
fn references_name_what_the_manifest_and_bundle_hold() {
    let mut m = image();
    m.servers[0].program = "nosuch".into();
    refused_at(&m, "servers[0].program", Why::Unknown);
    let mut m = image();
    m.servers[0].program = MANIFEST.into();
    refused_at(&m, "servers[0].program", Why::Unknown);
    let mut m = image();
    server(&mut m, "ipd").handed[0].endpoint = "littlefsd".into();
    refused_at(&m, "servers[5].handed[0].endpoint", Why::Unknown);
    let mut m = image();
    m.servers[0].volume = Some("nowhere".into());
    refused_at(&m, "servers[0].volume", Why::Unknown);
    let mut m = image();
    m.servers[0].labels = vec!["alice-secrets".into()];
    refused_at(&m, "servers[0].labels[0]", Why::Unknown);
    let mut m = image();
    secrets(&mut m);
    refused_at(&m, "labels[0].owner", Why::Unknown);
}

#[test]
fn a_handed_badge_is_a_root_badge_given_once_at_its_endpoint() {
    for badge in [0, 1 << 63] {
        let mut m = image();
        server(&mut m, "netd").handed[0].badge = badge;
        refused_at(&m, "servers[4].handed[0].badge", Why::Badge);
    }
    // netd's badge at ipd, given again to keyd first: the later is refused.
    let mut m = image();
    server(&mut m, "keyd").handed.push(Handed { endpoint: "ipd".into(), badge: 3 });
    refused_at(&m, "servers[4].handed[0].badge", Why::Badge);
    // The same badge at another endpoint is another root.
    let mut m = image();
    server(&mut m, "keyd").handed.push(Handed { endpoint: "netd".into(), badge: 3 });
    assert!(on_virt(&m).is_ok());
}

#[test]
fn principals_values_are_checked() {
    let mut m = image();
    m.principals.push(Principal { account: 0, ..alice() });
    refused_at(&m, "principals[0].account", Why::Value);
    let mut m = image();
    m.principals.push(alice());
    m.principals.push(Principal { name: "bob".into(), ..alice() });
    refused_at(&m, "principals[1].account", Why::Twice);
    let mut m = image();
    m.principals.push(Principal { home: Some("nowhere:/home/alice".into()), ..alice() });
    refused_at(&m, "principals[0].home", Why::Value);
    m.principals[0].home = Some("data:home/../x".into());
    refused_at(&m, "principals[0].home", Why::Value);
    m.principals[0].home = Some("data:/home/alice".into());
    assert!(on_virt(&m).is_ok());
    for (prefix, port) in [("10.0.0.0/33", 22), ("10.0.0.0/08", 22), ("10.0.0/8", 22), ("::/0", 0)] {
        let mut m = image();
        m.principals
            .push(Principal { net: vec![Net { prefix: prefix.into(), ports: vec![port] }], ..alice() });
        let path = if port == 0 { "principals[0].net[0].ports[0]" } else { "principals[0].net[0].prefix" };
        refused_at(&m, path, Why::Value);
    }
    let mut m = image();
    m.principals.push(Principal { labels: vec!["alice-secrets".into()], ..alice() });
    m.principals.push(Principal { name: "bob".into(), account: 1002, ..alice() });
    m.labels.push(Label { name: "alice-secrets".into(), owner: "bob".into(), id: 7 });
    refused_at(&m, "principals[0].labels[0]", Why::Unknown);
}

// ---- devices ----

#[test]
fn a_device_name_is_at_most_60_bytes_and_never_ends_in_irq() {
    let mut m = image();
    m.devices[0].name = "uart-irq".into();
    refused_at(&m, "devices[0].name", Why::DeviceName);
    let mut m = image();
    m.devices[0].name = "u".repeat(61);
    refused_at(&m, "devices[0].name", Why::DeviceName);
    let mut m = image();
    m.devices[0].name = "u".repeat(60);
    server(&mut m, "consoled").devices[0].device = "u".repeat(60);
    assert!(on_virt(&m).is_ok());
    let mut m = image();
    server(&mut m, "consoled").devices[0].name = "uart-irq".into();
    refused_at(&m, "servers[1].devices[0].as", Why::DeviceName);
}

#[test]
fn a_device_no_handle_names_is_refused() {
    let mut m = image();
    m.devices[1].base = Some(0x2000_0000);
    refused_at(&m, "devices[1].base", Why::Unmatched);
    let mut m = image();
    m.devices[1].irq = Some(11);
    refused_at(&m, "devices[1].irq", Why::Unmatched);
    // The Reset right is never a device a manifest can name.
    let mut m = image();
    m.devices[1].base = Some(0);
    refused_at(&m, "devices[1].base", Why::Unmatched);
}

#[test]
fn a_device_split_between_two_entries_is_refused() {
    let mut m = image();
    m.devices[2].base = m.devices[1].base;
    refused_at(&m, "devices[2].base", Why::SplitDevice);
    // One entry for the registers, another for the interrupt.
    let mut m = image();
    m.devices[1].irq = None;
    m.devices.push(redoubt_init::manifest::Device {
        name: "disk0i".into(),
        base: None,
        irq: Some(8),
        dma: false,
    });
    assert!(on_virt(&m).is_ok(), "the interrupt alone is a device no other entry names");
    m.devices[1].irq = Some(8);
    refused_at(&m, "devices[4].irq", Why::SplitDevice);
}

#[test]
fn the_dma_flag_must_be_the_kernel_s() {
    let mut m = image();
    m.devices[0].dma = true;
    refused_at(&m, "devices[0].dma", Why::DmaMismatch);
    let mut m = image();
    m.devices[1].dma = false;
    refused_at(&m, "devices[1].dma", Why::DmaMismatch);
    let mut m = image();
    m.devices[1].base = None;
    refused_at(&m, "devices[1].dma", Why::DmaMismatch);
}

#[test]
fn one_device_has_one_holder() {
    let mut m = image();
    let uart = server(&mut m, "consoled").devices[0].clone();
    server(&mut m, "keyd").devices.push(uart);
    refused_at(&m, "servers[1].devices[0].device", Why::HeldTwice);
    let mut m = image();
    server(&mut m, "keyd")
        .devices
        .push(redoubt_init::manifest::DeviceUse { device: "gpu".into(), name: "gpu".into() });
    refused_at(&m, "servers[0].devices[0].device", Why::Unknown);
}

// ---- R33, budgets and the fit in system ----

#[test]
fn a_server_handed_a_budget_is_refused() {
    for budget in ["root", "system", "users"] {
        let mut m = image();
        server(&mut m, "keyd").handed.push(Handed { endpoint: budget.into(), badge: 1 });
        refused_at(&m, "servers[0].handed[0].endpoint", Why::BudgetHandle);
        let mut m = image();
        server(&mut m, "keyd").receives.push(budget.into());
        refused_at(&m, "servers[0].receives[1]", Why::BudgetHandle);
    }
}

#[test]
fn a_budget_no_process_could_run_in_is_refused() {
    for (processes, weight) in [(0, 100), (1, 0), (1, 1 << 32)] {
        let mut m = image();
        m.servers[3].budget = Budget { pages: 1, processes, weight };
        refused_at(&m, "servers[3].budget", Why::Budget);
    }
}

#[test]
fn servers_that_do_not_fit_in_system_are_refused() {
    let devices = virt_devices();
    let mut machine = machine(&devices, &ENTRIES);
    let m = image();
    // keyd 256, consoled 1024, bootfsd 4096, the two blkds 512 each, verity:system 256, netd
    // 1024, ipd 4096, walfsd:data and erofsd:system 1024 each, and a page each for the budgets.
    let pages = 256 + 1024 + 4096 + 512 * 2 + 256 + 1024 + 4096 + 1024 * 2 + 10;
    machine.system.pages_limit = machine.system.pages_usage + pages - 1;
    assert_eq!(
        on(&m, &machine).unwrap_err(),
        Refusal::SystemFit { what: "pages", need: pages, free: pages - 1 }
    );
    machine.system.pages_limit += 1;
    assert!(on(&m, &machine).is_ok());
    machine.system.processes_usage = machine.system.processes_limit - 9;
    assert_eq!(on(&m, &machine).unwrap_err(), Refusal::SystemFit { what: "processes", need: 10, free: 9 });
    machine.system.processes_usage = 0;
    machine.system.weight_carved = machine.system.weight_limit - 4599;
    assert_eq!(on(&m, &machine).unwrap_err(), Refusal::SystemFit { what: "weight", need: 4600, free: 4599 });
}

// ---- public ----

#[test]
fn public_names_entries_the_bundle_holds_never_the_manifest() {
    let mut m = image();
    m.public = vec!["trace".into()];
    assert!(on_virt(&m).is_ok());
    m.public = vec![MANIFEST.into()];
    refused_at(&m, "public[0]", Why::PublicManifest);
    m.public = vec!["absent".into()];
    refused_at(&m, "public[0]", Why::Unknown);
    m.public = vec!["trace".into(), "trace".into()];
    refused_at(&m, "public[1]", Why::Twice);
    m.public = vec!["trace".into()];
    // And beamlet, its client, with it.
    m.servers.retain(|s| s.program != "bootfsd" && s.program != "beamlet");
    refused_at(&m, "public", Why::NoBootfsd);
}

#[test]
fn bootfsd_is_given_the_public_list_after_its_buckets() {
    let mut m = image();
    m.public = vec!["trace".into()];
    let bootfsd = &m.servers[2];
    assert_eq!(redoubt_init::check::args(&m, bootfsd, &BUNDLE_KEY), ["buckets=5", "trace"]);
    assert_eq!(redoubt_init::check::args(&m, &m.servers[0], &BUNDLE_KEY).len(), 3, "only bootfsd gets it");
    // The names bootfsd serves come from public alone.
    server(&mut m, "bootfsd").args.push("trace".into());
    refused_at(&m, "servers[2].args[1]", Why::Argument);
    // A public list too long for bootfsd's startup block is refused before the boot.
    let names: Vec<String> = (0..80).map(|n| format!("{n:0>60}")).collect();
    let mut entries = ENTRIES.to_vec();
    entries.extend(names.iter().map(|n| (n.as_str(), 1)));
    let devices = virt_devices();
    let mut m = image();
    m.public = names.clone();
    refused_at_on(&m, &machine(&devices, &entries), "servers[2]", Why::Block);
    m.public.truncate(40);
    assert!(on(&m, &machine(&devices, &entries)).is_ok());
}

// ---- startup blocks ----

/// A volume's server gets `labels=` its volume's ids, and `blkd` one `labels.P=` per labelled
/// volume, P its GPT entry; an unlabelled volume gets neither (servers/init.md, "The boot
/// manifest"; servers/blkd.md, "Ranges and badges").
#[test]
fn a_volume_s_labels_go_to_its_server_and_to_blkd() {
    let mut m = without_volumes();
    secrets(&mut m);
    m.principals.push(alice());
    m.volumes.push(Volume { name: "scratch".into(), partition: 0, labels: vec![], disk: None, verity: None });
    m.volumes.push(Volume {
        name: "vault".into(),
        partition: 2,
        labels: vec!["alice-secrets".into()],
        disk: None,
        verity: None,
    });
    server(&mut m, "keyd").volume = Some("vault".into());
    server(&mut m, "bootfsd").volume = Some("scratch".into());
    assert!(on_virt(&m).is_ok());
    let args = |m: &Manifest, name: &str| {
        redoubt_init::check::args(m, m.servers.iter().find(|s| s.name == name).unwrap(), &BUNDLE_KEY)
    };
    assert_eq!(args(&m, "keyd").last().unwrap(), "labels=7");
    assert_eq!(args(&m, "bootfsd"), ["buckets=5", "beamlet"]);
    assert_eq!(args(&m, "blkd"), ["endpoint=blkd", "labels.2=7"]);
}

/// R47 (one volume per instance): a volume is one GPT entry, attached by one server, whose range
/// is minted at its disk's `blkd`; and no entry carries an argument `init` passes itself.
#[test]
fn a_volume_is_one_entry_for_one_server_at_one_blkd() {
    let data = || Volume { name: "data".into(), partition: 0, labels: vec![], disk: None, verity: None };
    let mut m = without_volumes();
    m.volumes = vec![data(), Volume { name: "other".into(), ..data() }];
    refused_at(&m, "volumes[1].partition", Why::Twice);
    let mut m = without_volumes();
    m.volumes.push(data());
    m.servers[0].volume = Some("data".into());
    m.servers[2].volume = Some("data".into());
    refused_at(&m, "servers[2].volume", Why::Twice);
    let mut m = without_volumes();
    m.volumes.push(data());
    m.servers[0].volume = Some("data".into());
    m.servers.retain(|s| s.program != "blkd");
    refused_at(&m, "servers[0].volume", Why::NoBlkd);
    let mut m = without_volumes();
    m.volumes.push(data());
    m.servers[0].volume = Some("data".into());
    let mut second = server(&mut m, "blkd").clone();
    second.name = "blkd2".into();
    second.receives = vec!["blkd2".into()];
    second.devices.clear();
    m.servers.push(second);
    refused_at(&m, "volumes[0].disk", Why::NoDisk);
    let mut m = without_volumes();
    m.volumes.push(data());
    m.servers[0].volume = Some("data".into());
    m.servers[0].args.push("labels=1".into());
    refused_at(&m, "servers[0].args[3]", Why::Argument);
    let mut m = without_volumes();
    server(&mut m, "blkd").args.push("labels.0=1".into());
    refused_at(&m, "servers[3].args[1]", Why::Argument);
}

/// R47 (one volume per instance): `blkd` resolves a badge at its endpoint to a volume's range, so
/// a server handed one would hold the raw blocks of a volume another server attaches.
#[test]
fn no_server_is_handed_a_badge_at_blkd() {
    let mut m = without_volumes();
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: vec![], disk: None, verity: None });
    m.servers[0].volume = Some("data".into());
    assert!(on_virt(&m).is_ok());
    let blkd = server(&mut m, "blkd").receives[0].clone();
    server(&mut m, "netd").handed.push(Handed { endpoint: blkd.clone(), badge: 1 });
    let netd = m.servers.iter().position(|s| s.program == "netd").unwrap();
    let k = m.servers[netd].handed.len() - 1;
    refused_at(&m, &format!("servers[{netd}].handed[{k}].endpoint"), Why::BlkdHanded);
    // With no volume at all, and at any badge.
    let mut m = without_volumes();
    server(&mut m, "keyd").handed.push(Handed { endpoint: blkd, badge: 9 });
    refused_at(&m, "servers[0].handed[0].endpoint", Why::BlkdHanded);
}

/// The userland disk's device: QEMU `virt`'s virtio-mmio bus 5 (docs/testbench.md, "Disks and
/// network cards").
fn disk1(m: &mut Manifest) {
    m.devices.push(Device { name: "disk1".into(), base: Some(0x1000_6000), irq: Some(6), dma: true });
}

/// A second disk: its own `blkd` on `device`, and `littlefsd:system` attaching its volume `system`,
/// which names that `blkd` as its disk.
fn second_disk(m: &mut Manifest, device: &str, suffix: &str, labels: Vec<String>) {
    let blkd = format!("blkd:system{suffix}");
    let littlefsd = format!("littlefsd:system{suffix}");
    let volume = format!("system{suffix}");
    m.volumes.push(Volume {
        name: volume.clone(),
        partition: 0,
        labels: labels.clone(),
        disk: Some(blkd.clone()),
        verity: None,
    });
    let base = server(&mut image(), "blkd").clone();
    m.servers.push(Server {
        name: blkd.clone(),
        labels: labels.clone(),
        receives: vec![blkd.clone()],
        devices: vec![DeviceUse { device: device.into(), name: "disk".into() }],
        args: vec![format!("endpoint={blkd}")],
        ..base
    });
    let base = server(&mut image(), "walfsd:data").clone();
    m.servers.push(Server {
        name: littlefsd.clone(),
        program: "littlefsd".into(),
        labels,
        volume: Some(volume),
        receives: vec![littlefsd.clone()],
        args: vec![format!("endpoint={littlefsd}"), "buckets=4".into()],
        ..base
    });
}

/// Two disks (servers/init.md, "The boot manifest", Volumes): each volume names the `blkd` of its
/// disk, its range is minted there and only that `blkd` is given its labels; one GPT entry on two
/// disks is two volumes. With two `blkd`s a volume without `disk` is refused, and so is a `disk`
/// naming no `blkd`, one entry twice on one disk, and a badge handed at the second `blkd`.
#[test]
fn each_volume_s_range_is_minted_at_its_own_disk_s_blkd() {
    let mut m = without_userland();
    secrets(&mut m);
    m.principals.push(alice());
    disk1(&mut m);
    m.volumes[0].disk = Some("blkd".into());
    second_disk(&mut m, "disk1", "", vec!["alice-secrets".into()]);
    assert!(on_virt(&m).is_ok());
    let at = |m: &Manifest, volume: &str| {
        let v = m.volumes.iter().find(|v| v.name == volume).unwrap();
        redoubt_init::check::blkd(m, v).map(|b| b.name.clone())
    };
    assert_eq!(at(&m, "data").as_deref(), Some("blkd"));
    assert_eq!(at(&m, "system").as_deref(), Some("blkd:system"));
    let args = |m: &Manifest, name: &str| {
        redoubt_init::check::args(m, m.servers.iter().find(|s| s.name == name).unwrap(), &BUNDLE_KEY)
    };
    assert_eq!(args(&m, "blkd"), ["endpoint=blkd"]);
    assert_eq!(args(&m, "blkd:system"), ["endpoint=blkd:system", "labels.0=7"]);
    // A volume without its disk, and a disk naming a server that is not a blkd.
    let mut missing = m.clone();
    missing.volumes[0].disk = None;
    refused_at(&missing, "volumes[0].disk", Why::NoDisk);
    let mut unknown = m.clone();
    unknown.volumes[1].disk = Some("netd".into());
    refused_at(&unknown, "volumes[1].disk", Why::Unknown);
    // One GPT entry twice on one disk.
    let mut twice = m.clone();
    twice.volumes.push(Volume { name: "more".into(), ..twice.volumes[1].clone() });
    refused_at(&twice, "volumes[2].partition", Why::Twice);
    // A badge at the second blkd's endpoint.
    server(&mut m, "netd").handed.push(Handed { endpoint: "blkd:system".into(), badge: 9 });
    let netd = m.servers.iter().position(|s| s.program == "netd").unwrap();
    let k = m.servers[netd].handed.len() - 1;
    refused_at(&m, &format!("servers[{netd}].handed[{k}].endpoint"), Why::BlkdHanded);
}

/// A `blkd` receives on the endpoint its one `endpoint=` names, and `init` mints its volumes'
/// ranges at its first: the two must be one, or a range would be minted where no `blkd` serves.
#[test]
fn a_blkd_receives_where_init_mints_its_ranges() {
    let mut m = image();
    server(&mut m, "blkd").args = vec!["endpoint=blkd:system".into()];
    refused_at(&m, "servers[3].args[0]", Why::BlkdEndpoint);
    server(&mut m, "blkd").args.clear();
    refused_at(&m, "servers[3].args", Why::BlkdEndpoint);
    server(&mut m, "blkd").args = vec!["endpoint=blkd".into(), "endpoint=blkd".into()];
    refused_at(&m, "servers[3].args[1]", Why::BlkdEndpoint);
    server(&mut m, "blkd").receives.push("blkd2".into());
    server(&mut m, "blkd").args = vec!["endpoint=blkd2".into()];
    refused_at(&m, "servers[3].args[0]", Why::BlkdEndpoint);
    server(&mut m, "blkd").args = vec!["endpoint=blkd".into()];
    assert!(on_virt(&m).is_ok());
}

#[test]
fn handles_and_arguments_must_fit_one_startup_block() {
    let mut m = image();
    server(&mut m, "keyd").args.push("x".repeat(4000));
    refused_at(&m, "servers[0]", Why::Block);
    let mut m = image();
    server(&mut m, "keyd").receives = (0..MAX_START_HANDLES).map(|n| format!("keyd{n}")).collect();
    refused_at(&m, "servers[0]", Why::Block);
    let mut m = image();
    server(&mut m, "keyd").args.push("a\0b".into());
    refused_at(&m, "servers[0].args[3]", Why::Argument);
}

// ---- R35: the keys keyd is asked about ----

#[test]
fn a_manifest_without_keyd_is_refused_and_init_calls_each_server_at_an_endpoint() {
    let mut m = image();
    m.servers.retain(|s| s.program != "keyd");
    refused_at(&m, "servers", Why::NoKeyd);
    for (i, name) in ["keyd", "consoled", "bootfsd"].into_iter().enumerate() {
        // Without beamlet, which is handed bootfsd.
        let mut m = without_userland();
        server(&mut m, name).receives.clear();
        refused_at(&m, &format!("servers[{i}].receives"), Why::Unknown);
    }
}

/// Only `init` holds a root badge at `consoled`: a server handed one would write bare lines,
/// which only `init`'s may be (servers/consoled.md, "Started by `init`").
#[test]
fn no_server_is_handed_a_root_badge_at_consoled() {
    let mut m = image();
    server(&mut m, "netd").handed.push(Handed { endpoint: "consoled".into(), badge: 7 });
    refused_at(&m, "servers[4].handed[1].endpoint", Why::ConsoledRoot);
    // Whatever the endpoint is called, and whichever server is handed it.
    let mut m = image();
    server(&mut m, "consoled").receives = vec!["cons".into()];
    server(&mut m, "keyd").handed.push(Handed { endpoint: "cons".into(), badge: 2 });
    refused_at(&m, "servers[0].handed[0].endpoint", Why::ConsoledRoot);
    // Another server's endpoint is still handed as before.
    let mut m = image();
    server(&mut m, "keyd").handed.push(Handed { endpoint: "bootfsd".into(), badge: 9 });
    assert!(on_virt(&m).is_ok());
}

#[test]
fn init_calls_one_of_each_server_it_calls() {
    for name in ["keyd", "consoled", "bootfsd"] {
        let mut m = image();
        let mut second = server(&mut m, name).clone();
        second.name = format!("{name}2");
        second.receives = vec![format!("{name}2")];
        m.servers.push(second);
        let at = format!("servers[{}].program", m.servers.len() - 1);
        refused_at(&m, &at, Why::Second(name));
    }
}

#[test]
fn every_login_and_approval_key_then_the_bundle_key_is_asked_about() {
    let mut m = image();
    let mut approval = [0u8; 32];
    approval[0] = 0xaa;
    m.principals.push(Principal {
        approval_keys: vec![
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
        ],
        ..alice()
    });
    let counting: [u8; 32] = core::array::from_fn(|i| i as u8 + 1);
    assert_eq!(
        on_virt(&m).unwrap().keys,
        vec![
            ("principals[0].ssh_keys[0]".into(), counting),
            ("principals[0].approval_keys[0]".into(), approval),
            ("bundle key".into(), BUNDLE_KEY)
        ]
    );
    m.principals[0].ssh_keys.push("ssh-rsa AAAA".into());
    refused_at(&m, "principals[0].ssh_keys[1]", Why::Key);
}

/// Every key once across all principals' lists: a login key of one principal that is another's
/// approval key, or one listed twice, refuses the boot before any server runs.
#[test]
fn a_key_in_two_roles_across_principals_is_refused() {
    let mut m = image();
    m.principals.push(alice());
    m.principals.push(Principal {
        name: "bob".into(),
        account: 1002,
        ssh_keys: vec![],
        approval_keys: vec![LOGIN_KEY.into()],
        ..alice()
    });
    refused_at(&m, "principals[1].approval_keys[0]", Why::Twice);
    let mut m = image();
    m.principals.push(Principal { ssh_keys: vec![LOGIN_KEY.into(), LOGIN_KEY.into()], ..alice() });
    refused_at(&m, "principals[0].ssh_keys[1]", Why::Twice);
    let mut m = image();
    m.principals.push(Principal { approval_keys: vec![LOGIN_KEY.into()], ..alice() });
    refused_at(&m, "principals[0].approval_keys[0]", Why::Twice);
}

// ---- buckets ----

#[test]
fn a_shared_server_needs_a_bucket_per_declared_domain_and_root_badge() {
    // Without beamlet, a fifth caller at bootfsd beside the three domains and init; keyd at the
    // four buckets this counts against.
    let mut m = without_userland();
    *server(&mut m, "keyd").args.last_mut().unwrap() = "buckets=4".into();
    secrets(&mut m);
    m.principals.push(Principal {
        label_sets: vec![
            LabelSet { labels: vec!["alice-secrets".into()] },
            // The same set again is the same domain.
            LabelSet { labels: vec!["alice-secrets".into()] },
        ],
        ..alice()
    });
    // Without alice's login key, which may appear only once.
    m.principals.push(Principal { name: "bob".into(), account: 1002, ssh_keys: vec![], ..alice() });
    // alice {} and {alice-secrets}, bob {}: 3, and init at keyd: 4, which buckets=4 holds.
    let plan = on_virt(&m).unwrap();
    assert_eq!(plan.buckets[0], (0, 4));
    // ipd: the 3 domains and netd's root badge.
    assert_eq!(plan.buckets[3], (5, 4));
    m.principals.push(Principal { name: "carol".into(), account: 1003, ssh_keys: vec![], ..alice() });
    assert_eq!(on_virt(&m).unwrap_err(), Refusal::Buckets { at: "servers[0]".into(), have: 4, need: 5 });
    let mut m = image();
    server(&mut m, "keyd").args.push("buckets=5".into());
    refused_at(&m, "servers[0].args", Why::BucketsArgument);
    let mut m = image();
    *server(&mut m, "keyd").args.last_mut().unwrap() = "buckets=33".into();
    refused_at(&m, "servers[0].args", Why::BucketsArgument);
}

// ---- R34: each sharing kind, each with its own reason ----

fn confined() -> Manifest {
    let mut m = image();
    m.confined = true;
    m
}

fn sharing(m: &Manifest) -> Sharing {
    match on_virt(m) {
        Err(Refusal::Confined { sharing, .. }) => sharing,
        other => panic!("{other:?}"),
    }
}

#[test]
fn confined_refuses_two_label_sets_on_one_endpoint() {
    let mut m = confined();
    secrets(&mut m);
    m.principals.push(alice());
    server(&mut m, "netd").labels = vec!["alice-secrets".into()];
    assert_eq!(sharing(&m), Sharing::Endpoint);
}

#[test]
fn confined_refuses_two_label_sets_on_one_volume() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals.push(alice());
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: vec![], disk: None, verity: None });
    // A labelled server attaching an unlabelled volume.
    m.servers[0].volume = Some("data".into());
    m.servers[0].labels = vec!["alice-secrets".into()];
    m.servers[0].args.pop();
    assert_eq!(sharing(&m), Sharing::Volume);
}

#[test]
fn confined_gives_a_labelled_domain_no_network() {
    let mut m = confined();
    secrets(&mut m);
    m.principals.push(Principal {
        label_sets: vec![LabelSet { labels: vec!["alice-secrets".into()] }],
        net: vec![Net { prefix: "0.0.0.0/0".into(), ports: vec![443] }],
        ..alice()
    });
    // Without a shared server between them, the network is what the two sets share first.
    for s in &mut m.servers {
        s.args.retain(|a| !a.starts_with("buckets="));
    }
    assert_eq!(sharing(&m), Sharing::Network);
}

/// A volume's range is a handle at `blkd`'s endpoint, so its server is one of `blkd`'s users: two
/// volumes with differing label sets on one disk share `blkd` and its device, and are refused; a
/// disk whose `blkd` carries its one volume's set boots.
#[test]
fn confined_refuses_two_label_sets_on_one_disk() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.labels.push(Label { name: "alice-other".into(), owner: "alice".into(), id: 8 });
    m.principals.push(alice());
    for s in &mut m.servers {
        s.args.retain(|a| !a.starts_with("buckets="));
    }
    // A littlefsd for each volume, each carrying its volume's set.
    for (name, partition, label) in [("vault", 0, "alice-secrets"), ("other", 1, "alice-other")] {
        m.volumes.push(Volume {
            name: name.into(),
            partition,
            labels: vec![label.into()],
            disk: None,
            verity: None,
        });
        let endpoint = format!("littlefsd:{name}");
        m.servers.push(Server {
            name: endpoint.clone(),
            program: "littlefsd".into(),
            volume: Some(name.into()),
            labels: vec![label.into()],
            receives: vec![endpoint.clone()],
            handed: vec![],
            devices: vec![],
            args: vec![format!("endpoint={endpoint}")],
            ..server(&mut image(), "walfsd:data").clone()
        });
    }
    assert_eq!(sharing(&m), Sharing::Device);
    // One label set on the disk, and blkd carrying it.
    m.servers.pop();
    m.volumes.pop();
    assert_eq!(sharing(&m), Sharing::Device, "an unlabelled blkd serving a labelled volume");
    server(&mut m, "blkd").labels = vec!["alice-secrets".into()];
    assert!(on_virt(&m).is_ok());
}

/// The userland disk in a confined boot (docs/userland/beamlet.md, "beamlet on Redoubt"): each
/// label set running beamlet gets its own read-only attachment of the userland image, its own
/// device, `blkd`, volume and `littlefsd`, all carrying that set, so nothing is shared and R34 passes as
/// it stands. The {alice-secrets} beamlet handed the unlabelled `littlefsd:system` instead shares its
/// endpoint across two sets and is refused.
#[test]
fn confined_gives_each_label_set_its_own_userland_disk() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals.push(alice());
    let secret = || vec![String::from("alice-secrets")];
    disk1(&mut m);
    m.devices.push(Device { name: "disk2".into(), base: Some(0x1000_5000), irq: Some(5), dma: true });
    second_disk(&mut m, "disk1", "", vec![]);
    second_disk(&mut m, "disk2", "-l", secret());
    // Each beamlet is handed its own set's `littlefsd:system`.
    let beamlet = |name: &str, labels: Vec<String>, littlefsd: &str| Server {
        name: name.into(),
        program: "beamlet".into(),
        budget: budget(256),
        stack_pages: 16,
        heap_pages: None,
        labels,
        devices: vec![],
        volume: None,
        receives: vec![],
        handed: vec![Handed { endpoint: littlefsd.into(), badge: 7 }],
        args: vec!["budget_pages=256".into()],
    };
    m.servers.push(beamlet("beamlet", vec![], "littlefsd:system"));
    m.servers.push(beamlet("beamlet-l", secret(), "littlefsd:system-l"));
    let devices = virt_devices();
    let machine = machine(&devices, &ENTRIES);
    assert!(on(&m, &machine).is_ok());
    m.servers.last_mut().unwrap().handed[0] = Handed { endpoint: "littlefsd:system".into(), badge: 8 };
    let littlefsd = m.servers.iter().position(|s| s.name == "littlefsd:system").unwrap();
    assert_eq!(
        on(&m, &machine).unwrap_err(),
        Refusal::Confined { at: format!("servers[{littlefsd}].receives[0]"), sharing: Sharing::Endpoint }
    );
}

/// A `blkd` with no devices serving a labelled volume shares no endpoint, volume or device with
/// its `littlefsd`, yet the volume's range at it puts two sets on the one instance.
#[test]
fn confined_refuses_a_server_instance_serving_two_label_sets() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals.push(alice());
    let secret = || vec![String::from("alice-secrets")];
    m.volumes.push(Volume { name: "vault".into(), partition: 0, labels: secret(), disk: None, verity: None });
    server(&mut m, "blkd").devices.clear();
    let base = server(&mut image(), "walfsd:data").clone();
    m.servers.push(Server { labels: secret(), volume: Some("vault".into()), ..base });
    let blkd = m.servers.iter().position(|s| s.program == "blkd").unwrap();
    assert_eq!(
        on_virt(&m).unwrap_err(),
        Refusal::Confined { at: format!("servers[{blkd}]"), sharing: Sharing::Server }
    );
    m.servers[blkd].labels = secret();
    assert!(on_virt(&m).is_ok());
}

/// A shared server's users are the principal domains with its own label set, since only those
/// may later be granted a connection there: alice working under {alice-secrets} beside unlabelled
/// shared keyd and consoled, with a labelled blkd, walfsd and client, boots; the client unlabelled
/// shares walfsd's endpoint across two sets and is refused.
#[test]
fn confined_counts_only_a_shared_servers_own_label_set() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals
        .push(Principal { label_sets: vec![LabelSet { labels: vec!["alice-secrets".into()] }], ..alice() });
    let secret = || vec![String::from("alice-secrets")];
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: secret(), disk: None, verity: None });
    server(&mut m, "blkd").labels = secret();
    let base = server(&mut image(), "walfsd:data").clone();
    m.servers.push(Server { labels: secret(), ..base.clone() });
    m.servers.push(Server {
        name: "client".into(),
        volume: None,
        labels: secret(),
        receives: vec!["client".into()],
        handed: vec![Handed { endpoint: "walfsd:data".into(), badge: 7 }],
        args: vec!["endpoint=client".into(), "buckets=4".into()],
        ..base
    });
    assert!(on_virt(&m).is_ok());
    m.servers.last_mut().unwrap().labels.clear();
    let walfsd = m.servers.len() - 2;
    assert_eq!(
        on_virt(&m).unwrap_err(),
        Refusal::Confined { at: format!("servers[{walfsd}].receives[0]"), sharing: Sharing::Endpoint }
    );
}

#[test]
fn confined_lets_label_sets_that_share_nothing_share_the_cores() {
    let mut m = confined();
    secrets(&mut m);
    m.principals.push(alice());
    // Two label sets that share nothing, on one core: every set shares the kernel and its cores.
    m.servers[0].labels = vec!["alice-secrets".into()];
    m.servers[0].args.pop();
    assert!(on_virt(&m).is_ok());
}

// ---- the steward: its entry, sizes, console and manifest lines ----

/// The image without its userland, with alice (owning alice-secrets and working under it) and
/// bob, and a steward entry the `steward` object names.
fn with_steward() -> Manifest {
    let mut m = without_userland();
    secrets(&mut m);
    let top = Budget { pages: 4096, processes: 4, weight: 100 };
    m.principals.push(Principal {
        labels: vec!["alice-secrets".into()],
        label_sets: vec![LabelSet { labels: vec!["alice-secrets".into()] }],
        budget: top,
        ..alice()
    });
    m.principals.push(Principal {
        name: "bob".into(),
        account: 1002,
        ssh_keys: vec![],
        budget: top,
        ..alice()
    });
    let base = server(&mut image(), "keyd").clone();
    m.servers.push(Server {
        name: "steward".into(),
        program: "steward".into(),
        budget: budget(256),
        receives: vec!["steward".into()],
        args: vec![],
        ..base
    });
    let size = |pages| Budget { pages, processes: 1, weight: 10 };
    m.steward = Some(Steward {
        server: "steward".into(),
        sizes: Sizes {
            session: size(512),
            agent: size(256),
            sub_agent: size(64),
            crossing: size(32),
            cost: 1,
        },
    });
    m
}

/// The lines carry each principal with its keys' ids, its owned labels' ids and its domains, its
/// unlabelled set first; `keyd []`, since the live key-separation checks are init's and sshd's;
/// the steward's slot count; and the sizes. Only the steward's entry gets them, and `users`.
#[test]
fn the_steward_s_entry_alone_is_given_the_manifest_lines() {
    let m = with_steward();
    on_virt(&m).unwrap();
    let key: [u8; 32] = core::array::from_fn(|i| i as u8 + 1);
    let id = redoubt_steward::hash::key_id(&key);
    let steward = m.servers.iter().find(|s| s.name == "steward").unwrap();
    assert_eq!(
        args(&m, steward, &BUNDLE_KEY),
        [
            format!(
                "principal \"alice\" account=1001 login=[{id}] approval=[] owned=[7] sets=[[],[7]] top=4096,4,100"
            ),
            "principal \"bob\" account=1002 login=[] approval=[] owned=[] sets=[[]] top=4096,4,100".into(),
            "keyd []".into(),
            format!("servers {STEWARD_SLOTS}"),
            "sizes session=512,1,10 agent=256,1,10 sub_agent=64,1,10 crossing=32,1,10 cost=1".into(),
            "label \"alice-secrets\" id=7".into(),
        ]
    );
    // The policy core reads its own back as the manifest they came from.
    let lines = args(&m, steward, &BUNDLE_KEY);
    let core = lines.iter().map(String::as_str).filter(|l| !l.starts_with("label "));
    let read = redoubt_steward::manifest::parse_lines(core).unwrap();
    assert_eq!(read.principals[0].login_keys, [id]);
    for s in m.servers.iter().filter(|s| s.name != "steward") {
        assert!(!is_steward(&m, s));
        assert!(!args(&m, s, &BUNDLE_KEY).iter().any(|a| a.starts_with("principal ")), "{}", s.name);
    }
    assert!(is_steward(&m, steward));
}

#[test]
fn the_steward_object_and_console_name_what_the_manifest_holds() {
    let schema = |text: &str| match read(text.as_bytes(), ARENA_PAGES) {
        Err(Refusal::Schema(e)) => (e.path, e.kind),
        other => panic!("{other:?}"),
    };
    assert_eq!(schema(r#"{ "steward": {} }"#), ("steward.server".into(), SchemaKind::Missing));
    // A label set's sub-budget is the steward's equal share, never the manifest's to give.
    let set = r#"{ "principals": [ { "name": "a", "account": "1", "budget": { "pages": "4", "processes": 1, "weight": 1 }, "label_sets": [ { "labels": [], "budget": {} } ] } ] }"#;
    assert_eq!(schema(set), ("principals[0].label_sets[0].budget".into(), SchemaKind::Unknown));
    let mut m = with_steward();
    m.steward.as_mut().unwrap().server = "nobody".into();
    refused_at(&m, "steward.server", Why::Unknown);
    let mut m = with_steward();
    m.console = Some("carol".into());
    refused_at(&m, "console", Why::Unknown);
    m.console = Some("alice".into());
    assert!(on_virt(&m).is_ok());
    m.steward = None;
    refused_at(&m, "console", Why::Unknown); // The lines are init's to write: a principal in the entry's own arguments is refused.
    let mut m = with_steward();
    server(&mut m, "steward").args = vec!["buckets=4".into(), "principal \"eve\"".into()];
    refused_at(&m, "servers[7].args[1]", Why::Argument);
}

/// After the core's lines, the steward's own: each label's name, alice's home at the handle of
/// her volume's server, her vault set's labelled volume, and her network scope; bob has none of
/// them. A home or vault whose server the steward is not handed is refused.
#[test]
fn the_steward_s_own_lines_bind_homes_vaults_and_scopes() {
    let mut m = with_steward();
    m.volumes.push(Volume {
        name: "alice-secrets".into(),
        partition: 1,
        labels: vec!["alice-secrets".into()],
        disk: None,
        verity: None,
    });
    let base = server(&mut m, "walfsd:data").clone();
    m.servers.push(Server {
        name: "littlefsd:alice-secrets".into(),
        program: "littlefsd".into(),
        volume: Some("alice-secrets".into()),
        receives: vec!["littlefsd:alice-secrets".into()],
        args: vec!["endpoint=littlefsd:alice-secrets".into(), "buckets=4".into()],
        ..base
    });
    m.principals[0].home = Some("data:/home/alice".into());
    m.console = Some("alice".into());
    m.principals[0].net = vec![
        Net { prefix: "0.0.0.0/0".into(), ports: vec![22, 443] },
        Net { prefix: "10.0.0.0/8".into(), ports: vec![] },
    ];
    for e in ["walfsd:data", "littlefsd:alice-secrets"] {
        server(&mut m, "steward").handed.push(Handed { endpoint: e.into(), badge: 9 });
    }
    on_virt(&m).unwrap();
    let steward = m.servers.iter().find(|s| s.name == "steward").unwrap();
    let lines = args(&m, steward, &BUNDLE_KEY);
    let own: Vec<&str> = lines.iter().map(String::as_str).skip_while(|l| !l.starts_with("label ")).collect();
    assert_eq!(
        own,
        [
            "label \"alice-secrets\" id=7",
            "home \"alice\" handle=walfsd:data path=/home/alice",
            "vault \"alice\" labels=[7] handle=littlefsd:alice-secrets",
            "net \"alice\" 0.0.0.0/0:22,443 10.0.0.0/8:*",
            "console \"alice\"",
        ]
    );
    // A scope the steward cannot ask ipd for: IPv6, or more than ipd's eight rules.
    let mut v6 = m.clone();
    v6.principals[0].net = vec![Net { prefix: "::/0".into(), ports: vec![22] }];
    refused_at(&v6, "principals[0].net", Why::Value);
    let mut wide = m.clone();
    wide.principals[0].net = vec![Net { prefix: "0.0.0.0/0".into(), ports: (1..=9).collect() }];
    refused_at(&wide, "principals[0].net", Why::Value);
    let mut unhanded = m.clone();
    server(&mut unhanded, "steward").handed.retain(|h| h.endpoint != "walfsd:data");
    refused_at(&unhanded, "principals[0].home", Why::Unknown);
    server(&mut m, "steward").handed.retain(|h| h.endpoint != "littlefsd:alice-secrets");
    refused_at(&m, "principals[0].label_sets[0]", Why::Unknown);
}

/// Every size a process can run in, and each within the smallest sub-budget the steward carves:
/// an equal share of a principal's budget per domain, less a budget's own cost.
#[test]
fn the_steward_s_sizes_fit_every_principal_s_smallest_share() {
    let mut m = with_steward();
    m.steward.as_mut().unwrap().sizes.agent.pages = 0;
    refused_at(&m, "steward.sizes.agent", Why::Budget);
    let mut m = with_steward();
    m.steward.as_mut().unwrap().sizes.crossing.processes = 0;
    refused_at(&m, "steward.sizes.crossing", Why::Budget);
    let mut m = with_steward();
    m.steward.as_mut().unwrap().sizes.cost = 0;
    refused_at(&m, "steward.sizes.cost", Why::Budget);
    // alice's two domains share 4096 pages: 2047 each after the cost; bob's one, 4095.
    let mut m = with_steward();
    m.steward.as_mut().unwrap().sizes.session.pages = 2047;
    assert!(on_virt(&m).is_ok());
    m.steward.as_mut().unwrap().sizes.session.pages = 2048;
    refused_at(&m, "principals[0].budget", Why::Sizes("session"));
    let mut m = with_steward();
    m.principals[1].budget.weight = 9;
    refused_at(&m, "principals[1].budget", Why::Sizes("session"));
}

// ---- the bound on root ----

#[test]
fn a_manifest_that_passes_every_other_check_but_costs_init_too_much_is_refused() {
    let mut m = without_volumes();
    // Endpoints are cheap for system and dear for root: each is a page of root's. Eight more
    // servers receiving on 60 each fit their blocks, system and every other rule.
    for n in 0..8 {
        let receives = (0..60).map(|e| format!("e{n}-{e}")).collect();
        m.servers.push(Server {
            name: format!("s{n}"),
            program: "littlefsd".into(),
            budget: budget(17),
            heap_pages: None,
            receives,
            ..m.servers[3].clone()
        });
        m.servers.last_mut().unwrap().devices.clear();
    }
    let devices = virt_devices();
    let machine = machine(&devices, &ENTRIES);
    let need = match on(&m, &machine) {
        Err(Refusal::Bound { need, free }) => {
            assert_eq!(free, 1000);
            need
        }
        Ok(plan) => panic!("passed with a bound of {}", plan.bound),
        Err(other) => panic!("{other:?}"),
    };
    let mut roomier = machine;
    roomier.root.pages_limit = roomier.root.pages_usage + need;
    assert_eq!(on(&m, &roomier).unwrap().bound, need);
}

#[test]
fn more_servers_than_init_has_threads_to_watch_are_refused() {
    use redoubt_rt::abi::MAX_THREADS;
    let mut m = image();
    // One thread watches each server, beside init's own.
    while m.servers.len() < MAX_THREADS {
        let n = m.servers.len();
        let receives = vec![format!("e{n}")];
        m.servers.push(Server {
            name: format!("s{n}"),
            program: "littlefsd".into(),
            receives,
            budget: budget(17),
            heap_pages: None,
            ..m.servers[3].clone()
        });
        m.servers.last_mut().unwrap().devices.clear();
    }
    let devices = virt_devices();
    let mut machine = machine(&devices, &ENTRIES);
    machine.system.processes_limit = u32::MAX;
    // A watcher's pages are init's: at MAX_THREADS - 1 servers they pass root's free pages, and
    // this test is about the threads.
    machine.root.pages_limit = u64::MAX;
    let most = MAX_THREADS - 1;
    assert_eq!(on(&m, &machine).unwrap_err(), Refusal::Watchers { servers: most + 1, most });
    m.servers.pop();
    assert!(on(&m, &machine).is_ok());
}

#[test]
fn a_refusal_is_one_line_naming_where_and_why() {
    let mut m = image();
    server(&mut m, "keyd").handed.push(Handed { endpoint: "system".into(), badge: 1 });
    let line = format!("init: refused the boot: {}", on_virt(&m).unwrap_err());
    assert_eq!(
        line,
        "init: refused the boot: servers[0].handed[0].endpoint: a server may not hold a budget handle (R33)"
    );
}

/// The fuzz campaign's kept corpus (`fuzz/seeds/check`), rerun.
#[test]
fn the_fuzz_corpus_still_passes() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds/check");
    let mut ran = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        redoubt_init::fuzz::check_one(&std::fs::read(entry.unwrap().path()).unwrap());
        ran += 1;
    }
    assert!(ran >= 1, "the corpus is there: {ran} inputs");
    redoubt_init::fuzz::check_one(IMAGE.as_bytes());
}

// ---- verified volumes ----

/// A root of the right form: the tests here never boot it.
const ROOT: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

/// A verified volume `name` (servers/verityd.md): the volume on `disk`'s GPT entry `partition`,
/// its `verityd` `verity:NAME` and its `littlefsd` `littlefsd:NAME`, all under `labels`.
fn verified_volume(m: &mut Manifest, name: &str, partition: i64, labels: Vec<String>, disk: Option<&str>) {
    let verifier = format!("verity:{name}");
    m.volumes.push(Volume {
        name: name.into(),
        partition,
        labels: labels.clone(),
        disk: disk.map(Into::into),
        verity: Some(Verity {
            server: verifier.clone(),
            root: Some(ROOT.into()),
            blocks: Some(100),
            ..Verity::default()
        }),
    });
    let declared = server(&mut image(), "verity:system").clone();
    m.servers.push(Server {
        name: verifier.clone(),
        labels: labels.clone(),
        receives: vec![verifier],
        ..declared
    });
    let base = server(&mut image(), "walfsd:data").clone();
    let littlefsd = format!("littlefsd:{name}");
    m.servers.push(Server {
        name: littlefsd.clone(),
        program: "littlefsd".into(),
        labels,
        volume: Some(name.into()),
        receives: vec![littlefsd.clone()],
        args: vec![format!("endpoint={littlefsd}"), "buckets=4".into()],
        ..base
    });
}

fn named<'m>(m: &'m Manifest, name: &str) -> &'m Server { m.servers.iter().find(|s| s.name == name).unwrap() }

/// A verified volume's server gets its `volume` badge, 1, at its verifier's endpoint, and the
/// verifier the volume's range at the disk's `blkd`, with `endpoint=`, `labels=`, `root=` and
/// `blocks=` from `init` (servers/init.md, Volumes; servers/verityd.md, "Arguments").
#[test]
fn a_verified_volume_s_server_reads_through_its_verifier() {
    use redoubt_init::check::range;
    let args = |m: &Manifest, s: &Server| redoubt_init::check::args(m, s, &BUNDLE_KEY);
    let mut m = without_volumes();
    verified_volume(&mut m, "data", 2, vec![], None);
    assert!(on_virt(&m).is_ok());
    assert_eq!(range(&m, named(&m, "littlefsd:data")), Some(("verity:data", 1)));
    assert_eq!(range(&m, named(&m, "verity:data")), Some(("blkd", 3)));
    assert_eq!(range(&m, named(&m, "blkd")), None);
    let root = format!("root={ROOT}");
    assert_eq!(args(&m, named(&m, "verity:data")), ["endpoint=verity:data", root.as_str(), "blocks=100"]);
    assert_eq!(args(&m, named(&m, "littlefsd:data")), ["endpoint=littlefsd:data", "buckets=4"]);
    // Labelled: the verifier, the server and blkd each get the volume's ids.
    let mut m = without_volumes();
    secrets(&mut m);
    m.principals.push(alice());
    verified_volume(&mut m, "vault", 0, vec!["alice-secrets".into()], None);
    assert!(on_virt(&m).is_ok());
    let args_of = |name: &str| args(&m, named(&m, name));
    assert_eq!(args_of("verity:vault"), ["endpoint=verity:vault", "labels=7", root.as_str(), "blocks=100"]);
    assert_eq!(args_of("littlefsd:vault").last().unwrap(), "labels=7");
    assert_eq!(args_of("blkd"), ["endpoint=blkd", "labels.0=7"]);
    // Beside it, an unverified volume's server keeps its range at blkd.
    m.volumes.push(Volume { name: "plain".into(), partition: 1, labels: vec![], disk: None, verity: None });
    server(&mut m, "keyd").volume = Some("plain".into());
    assert!(on_virt(&m).is_ok());
    assert_eq!(range(&m, named(&m, "keyd")), Some(("blkd", 2)));
}

/// Each refusal of the `verity` key, naming the field (servers/init.md, Volumes).
#[test]
fn a_verified_volume_s_key_and_verifier_are_refused_naming_the_field() {
    let base = || {
        let mut m = without_volumes();
        verified_volume(&mut m, "data", 0, vec![], None);
        assert!(on_virt(&m).is_ok());
        m
    };
    let verity = |m: &mut Manifest| m.volumes[0].verity.as_mut().unwrap().clone();
    let with = |change: &dyn Fn(&mut Verity)| {
        let mut m = base();
        let mut v = verity(&mut m);
        change(&mut v);
        m.volumes[0].verity = Some(v);
        m
    };
    for root in [ROOT.to_uppercase(), ROOT[1..].to_string(), format!("{ROOT}0"), ROOT.replace('a', "g")] {
        let root = root.clone();
        refused_at(&with(&|v| v.root = Some(root.clone())), "volumes[0].verity.root", Why::Value);
    }
    for blocks in [0, u64::MAX] {
        refused_at(&with(&|v| v.blocks = Some(blocks)), "volumes[0].verity.blocks", Why::Value);
    }
    refused_at(&with(&|v| v.server = "nobody".into()), "volumes[0].verity.server", Why::Unknown);
    refused_at(&with(&|v| v.server = "littlefsd:data".into()), "volumes[0].verity.server", Why::NotVerityd);
    // The servers here: keyd, consoled, bootfsd, blkd, netd, ipd, then verity:data and littlefsd:data.
    let verifier = 6;
    assert_eq!(base().servers[verifier].name, "verity:data");
    // A verifier named by two volumes, and one named by none.
    let mut m = base();
    let mut second = m.volumes[0].clone();
    second.name = "more".into();
    second.partition = 1;
    m.volumes.push(second);
    refused_at(&m, "volumes[1].verity.server", Why::Twice);
    let mut m = base();
    let mut spare = m.servers[verifier].clone();
    spare.name = "spare".into();
    spare.receives = vec!["spare".into()];
    m.servers.push(spare);
    refused_at(&m, "servers[8]", Why::NoVolume);
    // A verifier attaching a volume itself, receiving on nothing, or carrying an argument.
    let mut m = base();
    m.volumes.push(Volume { name: "other".into(), partition: 1, labels: vec![], disk: None, verity: None });
    m.servers[verifier].volume = Some("other".into());
    refused_at(&m, &format!("servers[{verifier}].volume"), Why::Verifier);
    let mut m = base();
    m.servers[verifier].receives.clear();
    refused_at(&m, &format!("servers[{verifier}].receives"), Why::Verifier);
    for arg in ["endpoint=verity:data", "labels=7", &format!("root={ROOT}"), "blocks=100", "buckets=4"] {
        let mut m = base();
        m.servers[verifier].args.push(arg.into());
        refused_at(&m, &format!("servers[{verifier}].args[0]"), Why::Argument);
    }
    // A verifier whose labels are not its volume's.
    let mut m = base();
    secrets(&mut m);
    m.principals.push(alice());
    m.servers[verifier].labels = vec!["alice-secrets".into()];
    refused_at(&m, &format!("servers[{verifier}].labels"), Why::VerifierLabels);
    // A badge handed at a verifier's endpoint would be the volume's range in another's hands.
    let mut m = base();
    server(&mut m, "netd").handed.push(Handed { endpoint: "verity:data".into(), badge: 2 });
    let netd = m.servers.iter().position(|s| s.name == "netd").unwrap();
    let k = m.servers[netd].handed.len() - 1;
    refused_at(&m, &format!("servers[{netd}].handed[{k}].endpoint"), Why::VerifierHanded);
}

/// A verified volume is pinned (`root` and `blocks`) or signed (`key` and `floor`): both modes,
/// neither, or a part of one is refused, and so is a key that is neither 64 lowercase hex digits
/// nor `bundle`, and a floor that is not a decimal string. Signed, the verifier gets `key=` (the
/// bundle's own key, by name, for `bundle`) and `floor=` in place of `root=` and `blocks=`
/// (servers/init.md, Volumes; servers/verityd.md, "The root block, and the two modes").
#[test]
fn a_verified_volume_is_pinned_or_signed_and_never_both() {
    let args = |m: &Manifest, s: &Server| redoubt_init::check::args(m, s, &BUNDLE_KEY);
    let signed = |key: &str| {
        let mut m = without_volumes();
        verified_volume(&mut m, "data", 0, vec![], None);
        m.volumes[0].verity = Some(Verity {
            server: "verity:data".into(),
            key: Some(key.into()),
            floor: Some(2),
            ..Verity::default()
        });
        m
    };
    let bundle = signed("bundle");
    assert_eq!(on_virt(&bundle).unwrap().bundle_key, BUNDLE_KEY, "the key check() was given");
    // `bundle` is the key the loader verified the bundle with, as check() was given it: here the
    // fuzz harness's, not the development key.
    assert_ne!(BUNDLE_KEY, redoubt_signing::DEV_PUBLIC_KEY);
    let dev: String = BUNDLE_KEY.iter().map(|b| format!("{b:02x}")).collect();
    let key = format!("key={dev}");
    assert_eq!(
        args(&bundle, named(&bundle, "verity:data")),
        ["endpoint=verity:data", key.as_str(), "floor=2"]
    );
    let own = signed(ROOT);
    assert!(on_virt(&own).is_ok());
    let key = format!("key={ROOT}");
    assert_eq!(args(&own, named(&own, "verity:data")), ["endpoint=verity:data", key.as_str(), "floor=2"]);
    for key in ["Bundle", "bundle ", "", &ROOT.to_uppercase(), &ROOT[2..]] {
        refused_at(&signed(key), "volumes[0].verity.key", Why::Value);
    }
    let modes = |change: &dyn Fn(&mut Verity)| {
        let mut m = signed("bundle");
        change(m.volumes[0].verity.as_mut().unwrap());
        m
    };
    let mode = Why::VerityMode;
    refused_at(&modes(&|v| (v.root, v.blocks) = (Some(ROOT.into()), Some(100))), "volumes[0].verity", mode);
    refused_at(&modes(&|v| v.blocks = Some(100)), "volumes[0].verity", mode);
    refused_at(&modes(&|v| v.floor = None), "volumes[0].verity", mode);
    refused_at(&modes(&|v| v.key = None), "volumes[0].verity", mode);
    refused_at(&modes(&|v| (v.key, v.floor) = (None, None)), "volumes[0].verity", mode);
    let pinned_half = |v: &mut Verity| (v.key, v.floor, v.root) = (None, None, Some(ROOT.into()));
    refused_at(&modes(&pinned_half), "volumes[0].verity", mode);
    // A floor is a decimal string, as every 64-bit quantity in the manifest; anything else is not
    // decoded.
    let floor = |floor: &str| {
        let text = format!(
            r#"{{ "volumes": [ {{ "name": "data", "partition": 0, "verity": {{ "server": "v", "key": "bundle", "floor": {floor} }} }} ] }}"#
        );
        match read(text.as_bytes(), ARENA_PAGES) {
            Err(Refusal::Schema(e)) => Some((e.path, e.kind)),
            Ok(_) => None,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(floor(r#""2""#), None);
    for bad in ["2", r#""-1""#, r#""two""#, r#""18446744073709551616""#] {
        assert_eq!(floor(bad), Some(("volumes[0].verity.floor".into(), SchemaKind::WrongType)), "{bad}");
    }
}

/// Each verifier is one more server and one more minted range: what it costs `init` in `root`
/// beside the same volume unverified is one server's pages (two endpoints, a process object, a
/// block and its tables, a watching thread) and here the handle-table page its handles open.
#[test]
fn a_verifier_costs_init_one_server_and_its_range() {
    let mut plain = without_volumes();
    plain.volumes.push(Volume {
        name: "data".into(),
        partition: 0,
        labels: vec![],
        disk: None,
        verity: None,
    });
    plain.servers.push(server(&mut image(), "walfsd:data").clone());
    let mut verified = without_volumes();
    verified_volume(&mut verified, "data", 0, vec![], None);
    let bound = |m: &Manifest| on_virt(m).unwrap().bound;
    // 22 handles at the start: 42 added fill page 0 exactly, and the verifier's 6 open page 1.
    assert_eq!(bound(&verified) - bound(&plain), 2 + 1 + 4 + 8 + 1);
}

/// R34 (confined placement) with verified volumes: each label set reads its own verified volume
/// through its own disk, `blkd`, `verityd` and `littlefsd`, and boots; an {alice-secrets} `littlefsd` on the
/// unlabelled verifier shares the verifier's endpoint across two sets, and is refused there.
#[test]
fn confined_gives_each_label_set_its_own_verifier() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals.push(alice());
    let secret = || vec![String::from("alice-secrets")];
    disk1(&mut m);
    m.devices.push(Device { name: "disk2".into(), base: Some(0x1000_5000), irq: Some(5), dma: true });
    for (device, suffix, labels) in [("disk1", "", vec![]), ("disk2", "-l", secret())] {
        let blkd = format!("blkd:system{suffix}");
        let base = server(&mut image(), "blkd").clone();
        m.servers.push(Server {
            name: blkd.clone(),
            labels: labels.clone(),
            receives: vec![blkd.clone()],
            devices: vec![DeviceUse { device: device.into(), name: "disk".into() }],
            args: vec![format!("endpoint={blkd}")],
            ..base
        });
        verified_volume(&mut m, &format!("system{suffix}"), 0, labels, Some(blkd.as_str()));
    }
    let devices = virt_devices();
    let machine = machine(&devices, &ENTRIES);
    assert!(on(&m, &machine).is_ok());
    server(&mut m, "littlefsd:system").labels = secret();
    let verifier = m.servers.iter().position(|s| s.name == "verity:system").unwrap();
    assert_eq!(
        on(&m, &machine).unwrap_err(),
        Refusal::Confined { at: format!("servers[{verifier}].receives[0]"), sharing: Sharing::Endpoint }
    );
}

/// An `erofsd` entry is a volume server as a `littlefsd` one is (servers/init.md, "The boot
/// manifest"): `init` names neither program, so a read-only volume's server gets its range at its
/// verifier, its arguments and its place in confinement exactly as a writable volume's does.
/// Here a confined manifest serves an unlabelled verified volume read-only through `erofsd`, and
/// a labelled one through `littlefsd` on a disk of its own.
#[test]
fn an_erofsd_entry_is_a_volume_server_as_a_littlefsd_one_is() {
    use redoubt_init::check::range;
    let args = |m: &Manifest, s: &Server| redoubt_init::check::args(m, s, &BUNDLE_KEY);
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals.push(alice());
    let secret = || vec![String::from("alice-secrets")];
    disk1(&mut m);
    m.devices.push(Device { name: "disk2".into(), base: Some(0x1000_5000), irq: Some(5), dma: true });
    for (device, suffix, labels) in [("disk1", "", vec![]), ("disk2", "-l", secret())] {
        let blkd = format!("blkd:system{suffix}");
        let base = server(&mut image(), "blkd").clone();
        m.servers.push(Server {
            name: blkd.clone(),
            labels: labels.clone(),
            receives: vec![blkd.clone()],
            devices: vec![DeviceUse { device: device.into(), name: "disk".into() }],
            args: vec![format!("endpoint={blkd}")],
            ..base
        });
        verified_volume(&mut m, &format!("system{suffix}"), 0, labels, Some(blkd.as_str()));
    }
    let erofsd = m.servers.iter_mut().find(|s| s.name == "littlefsd:system").unwrap();
    erofsd.name = "erofsd:system".into();
    erofsd.program = "erofsd".into();
    erofsd.receives = vec!["erofsd:system".into()];
    erofsd.args = vec!["endpoint=erofsd:system".into(), "buckets=4".into()];
    let devices = virt_devices();
    let machine = machine(&devices, &ENTRIES);
    assert!(on(&m, &machine).is_ok());
    assert_eq!(range(&m, named(&m, "erofsd:system")), Some(("verity:system", 1)));
    assert_eq!(args(&m, named(&m, "erofsd:system")), ["endpoint=erofsd:system", "buckets=4"]);
    assert_eq!(args(&m, named(&m, "littlefsd:system-l")).last().unwrap(), "labels=7");
    // Under the labelled set it would read the unlabelled volume's verifier from a labelled
    // domain: confinement refuses it as it would a `littlefsd`.
    server(&mut m, "erofsd:system").labels = secret();
    let verifier = m.servers.iter().position(|s| s.name == "verity:system").unwrap();
    assert_eq!(
        on(&m, &machine).unwrap_err(),
        Refusal::Confined { at: format!("servers[{verifier}].receives[0]"), sharing: Sharing::Endpoint }
    );
}

/// A `walfsd` entry is a volume server as a `littlefsd` one is (servers/init.md, "The boot
/// manifest"): `init` names neither program, so the image's `walfsd:data` gets its range at
/// `blkd`, its arguments, its place among the budgets and its share of the bound exactly as a
/// `littlefsd` in its place would.
#[test]
fn a_walfsd_entry_is_a_volume_server_as_a_littlefsd_one_is() {
    use redoubt_init::check::range;
    let args = |m: &Manifest, s: &Server| redoubt_init::check::args(m, s, &BUNDLE_KEY);
    let walfs = image();
    let mut littlefs = image();
    server(&mut littlefs, "walfsd:data").program = "littlefsd".into();
    let devices = virt_devices();
    let machine = machine(&devices, &ENTRIES);
    let (w, l) = (on(&walfs, &machine).unwrap(), on(&littlefs, &machine).unwrap());
    assert_eq!((&w.placements, &w.buckets, w.bound), (&l.placements, &l.buckets, l.bound));
    let data = |m: &Manifest| range(m, named(m, "walfsd:data")).map(|(at, badge)| (at.to_string(), badge));
    assert_eq!(data(&walfs).as_ref().map(|(at, _)| at.as_str()), Some("blkd"));
    assert_eq!(data(&walfs), data(&littlefs));
    assert_eq!(args(&walfs, named(&walfs, "walfsd:data")), ["endpoint=walfsd:data", "buckets=5"]);
}

/// `boot-profile-unverified` boots the image less its verification
/// (tests/boot-profile-unverified.toml), from copies: the image's manifest without each volume's
/// `verity` and the verifiers it names, and its userland recipe without `verity = true`. Every
/// other entry must stay the image's, so a change to the image either reaches the copies or
/// fails here, rather than the case measuring another image.
#[test]
fn the_boot_profiles_unverified_copies_are_the_image_less_its_verification() {
    let copy = include_str!("../../../tests/data/boot-profile/manifest-unverified.json");
    let copy = read(copy.as_bytes(), ARENA_PAGES).expect("the copy decodes");
    let mut m = full_image();
    let verifiers: Vec<String> =
        m.volumes.iter_mut().filter_map(|v| v.verity.take()).map(|v| v.server).collect();
    assert!(!verifiers.is_empty(), "the image verifies a volume");
    m.servers.retain(|s| !verifiers.contains(&s.name));
    assert_eq!(copy, m);

    // The recipe's own header differs; everything from the disk's size on is the image's.
    let body = |recipe: &'static str| &recipe[recipe.find("# Whole disk").expect("the disk's size")..];
    let image = body(include_str!("../../../image/userland.toml"));
    assert!(image.contains("\nverity = true\n"), "the image's userland volume is verified");
    let copy = body(include_str!("../../../tests/data/boot-profile/userland-unverified.toml"));
    assert_eq!(copy, image.replace("\nverity = true\n", "\n"));
}
