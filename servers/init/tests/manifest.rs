//! The manifest's checks, one refusal at a time (servers/init.md, "The boot manifest", "The
//! confinement check", "Starting the servers", "The key-separation check"): each test changes the
//! image's own manifest (`image/manifest.json`) in one way and expects the refusal for that rule
//! and no other.

use redoubt_init::check::{MANIFEST, Machine, Plan};
use redoubt_init::fuzz::{BUNDLE_KEY, ENTRIES, machine, virt_devices};
use redoubt_init::manifest::{Budget, Handed, Label, LabelSet, Net, Principal, Server, Volume};
use redoubt_init::refusal::{Refusal, Sharing, Why};
use redoubt_init::{ARENA_PAGES, Manifest, check, read};
use redoubt_rt::abi::MAX_START_HANDLES;
use redoubt_rt::wire::json::SchemaKind;

const IMAGE: &str = include_str!("../../../image/manifest.json");

/// An OpenSSH Ed25519 key whose 32 bytes are 1 to 32.
const LOGIN_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8g";

fn image() -> Manifest { read(IMAGE.as_bytes(), ARENA_PAGES).expect("the image's manifest decodes") }

/// The image's manifest without its volume and the `fsd` serving it, for tests that lay out
/// volumes of their own.
fn without_volumes() -> Manifest {
    let mut m = image();
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
    let plan = on_virt(&image()).unwrap();
    let h = |n| redoubt_rt::abi::Handle::new(n).unwrap();
    // virt's handles: the console in 5 and 6, slots 0x1000_1000 on from 7, their interrupts
    // (1 to 8) from 15.
    assert_eq!(plan.placements[1], vec![("uart".into(), h(5)), ("uart-irq".into(), h(6))]);
    assert_eq!(plan.placements[3], vec![("disk".into(), h(14)), ("disk-irq".into(), h(22))]);
    assert_eq!(plan.placements[4], vec![("net".into(), h(13)), ("net-irq".into(), h(21))]);
    assert!(plan.placements[0].is_empty());
    // No principals: only the bundle key is asked about.
    assert_eq!(plan.keys, vec![("bundle key".into(), BUNDLE_KEY)]);
    // keyd, consoled and bootfsd: init alone calls them; ipd: netd's badge; fsd:data: nobody's
    // yet, a principal's connection being the steward's to grant.
    assert_eq!(plan.buckets, vec![(0, 1), (1, 1), (2, 1), (5, 1), (6, 0)]);
    // No handed item names keyd, consoled or bootfsd: init's own badge at each is 1.
    assert_eq!(plan.init_badges, vec![(0, 1), (1, 1), (2, 1)]);
}

#[test]
fn init_s_own_badge_is_the_smallest_no_handed_item_uses_there() {
    let mut m = image();
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
    // The arena (256 + 3 tables), 7 receive and 7 exit endpoints and init's reports endpoint, 7
    // process objects, 7 blocks with 3 tables each, 7 watching threads (an IPC page, 4 stack pages
    // and 3 tables each), one launch (stub 4 + 3, one 64-page batch of ipd's 147-page image + 3,
    // stack 16 + 3), the lend (2 + 3), and no handle-table page: 22 handles at the start (3
    // budgets, the Reset right, 18 devices) and 7 + 3 + 28 + 3 + 1 = 42 added (fsd:data's range
    // among the 3 badges) still fit page 0.
    let devices = virt_devices();
    let m = machine(&devices, &ENTRIES);
    assert_eq!(m.handles_at_start, 22);
    assert_eq!(plan.bound, 259 + 15 + 7 + 28 + 56 + (4 + 3 + 64 + 3 + 16 + 3) + 5);
}

/// A volume's range badge is a handle `init` mints, as a `handed` item is: with the handle table
/// full to a page's edge, either one opens the next page.
#[test]
fn a_volume_s_range_badge_counts_in_the_bound_as_a_handed_item_does() {
    let mut m = without_volumes();
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: vec![] });
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
    m.servers[0].program = "sshd".into();
    refused_at(&m, "servers[0].program", Why::Unknown);
    let mut m = image();
    m.servers[0].program = MANIFEST.into();
    refused_at(&m, "servers[0].program", Why::Unknown);
    let mut m = image();
    server(&mut m, "ipd").handed[0].endpoint = "fsd".into();
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
    refused_at(&m, "devices[3].irq", Why::SplitDevice);
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
    // keyd 256, consoled 1024, bootfsd and blkd 512 each, netd 1024, ipd 4096 and fsd:data 1024
    // pages, and a page each for the budgets.
    let pages = 256 + 1024 + 512 * 2 + 1024 + 4096 + 1024 + 7;
    machine.system.pages_limit = machine.system.pages_usage + pages - 1;
    assert_eq!(
        on(&m, &machine).unwrap_err(),
        Refusal::SystemFit { what: "pages", need: pages, free: pages - 1 }
    );
    machine.system.pages_limit += 1;
    assert!(on(&m, &machine).is_ok());
    machine.system.processes_usage = machine.system.processes_limit - 6;
    assert_eq!(on(&m, &machine).unwrap_err(), Refusal::SystemFit { what: "processes", need: 7, free: 6 });
    machine.system.processes_usage = 0;
    machine.system.weight_carved = machine.system.weight_limit - 3399;
    assert_eq!(on(&m, &machine).unwrap_err(), Refusal::SystemFit { what: "weight", need: 3400, free: 3399 });
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
    m.servers.retain(|s| s.program != "bootfsd");
    refused_at(&m, "public", Why::NoBootfsd);
}

#[test]
fn bootfsd_is_given_the_public_list_after_its_buckets() {
    let mut m = image();
    m.public = vec!["trace".into()];
    let bootfsd = &m.servers[2];
    assert_eq!(redoubt_init::check::args(&m, bootfsd), ["buckets=4", "trace"]);
    assert_eq!(redoubt_init::check::args(&m, &m.servers[0]).len(), 3, "only bootfsd gets it");
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
    m.volumes.push(Volume { name: "scratch".into(), partition: 0, labels: vec![] });
    m.volumes.push(Volume { name: "vault".into(), partition: 2, labels: vec!["alice-secrets".into()] });
    server(&mut m, "keyd").volume = Some("vault".into());
    server(&mut m, "bootfsd").volume = Some("scratch".into());
    assert!(on_virt(&m).is_ok());
    let args = |m: &Manifest, name: &str| {
        redoubt_init::check::args(m, m.servers.iter().find(|s| s.name == name).unwrap())
    };
    assert_eq!(args(&m, "keyd").last().unwrap(), "labels=7");
    assert_eq!(args(&m, "bootfsd"), ["buckets=4"]);
    assert_eq!(args(&m, "blkd"), ["labels.2=7"]);
}

/// R47 (one volume per instance): a volume is one GPT entry, attached by one server, whose range
/// is minted at the one `blkd`; and no entry carries an argument `init` passes itself.
#[test]
fn a_volume_is_one_entry_for_one_server_at_one_blkd() {
    let data = || Volume { name: "data".into(), partition: 0, labels: vec![] };
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
    refused_at(&m, "servers[0].volume", Why::NoBlkd);
    let mut m = without_volumes();
    m.volumes.push(data());
    m.servers[0].volume = Some("data".into());
    m.servers[0].args.push("labels=1".into());
    refused_at(&m, "servers[0].args[3]", Why::Argument);
    let mut m = without_volumes();
    server(&mut m, "blkd").args.push("labels.0=1".into());
    refused_at(&m, "servers[3].args[0]", Why::Argument);
}

/// R47 (one volume per instance): `blkd` resolves a badge at its endpoint to a volume's range, so
/// a server handed one would hold the raw blocks of a volume another server attaches.
#[test]
fn no_server_is_handed_a_badge_at_blkd() {
    let mut m = without_volumes();
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: vec![] });
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
        let mut m = image();
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
    server(&mut m, "keyd").handed.push(Handed { endpoint: "bootfsd".into(), badge: 7 });
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

// ---- buckets ----

#[test]
fn a_shared_server_needs_a_bucket_per_declared_domain_and_root_badge() {
    let mut m = image();
    secrets(&mut m);
    m.principals.push(Principal {
        label_sets: vec![
            LabelSet { labels: vec!["alice-secrets".into()], budget: budget(64) },
            // The same set again is the same domain.
            LabelSet { labels: vec!["alice-secrets".into()], budget: budget(64) },
        ],
        ..alice()
    });
    m.principals.push(Principal { name: "bob".into(), account: 1002, ..alice() });
    // alice {} and {alice-secrets}, bob {}: 3, and init at keyd: 4, which buckets=4 holds.
    let plan = on_virt(&m).unwrap();
    assert_eq!(plan.buckets[0], (0, 4));
    // ipd: the 3 domains and netd's root badge.
    assert_eq!(plan.buckets[3], (5, 4));
    m.principals.push(Principal { name: "carol".into(), account: 1003, ..alice() });
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
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: vec![] });
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
        label_sets: vec![LabelSet { labels: vec!["alice-secrets".into()], budget: budget(64) }],
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
    // An fsd for each volume, each carrying its volume's set.
    for (name, partition, label) in [("vault", 0, "alice-secrets"), ("other", 1, "alice-other")] {
        m.volumes.push(Volume { name: name.into(), partition, labels: vec![label.into()] });
        let endpoint = format!("fsd:{name}");
        m.servers.push(Server {
            name: endpoint.clone(),
            program: "fsd".into(),
            volume: Some(name.into()),
            labels: vec![label.into()],
            receives: vec![endpoint.clone()],
            handed: vec![],
            devices: vec![],
            args: vec![format!("endpoint={endpoint}")],
            ..server(&mut image(), "fsd:data").clone()
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

/// A `blkd` with no devices serving a labelled volume shares no endpoint, volume or device with
/// its `fsd`, yet the volume's range at it puts two sets on the one instance.
#[test]
fn confined_refuses_a_server_instance_serving_two_label_sets() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals.push(alice());
    let secret = || vec![String::from("alice-secrets")];
    m.volumes.push(Volume { name: "vault".into(), partition: 0, labels: secret() });
    server(&mut m, "blkd").devices.clear();
    let base = server(&mut image(), "fsd:data").clone();
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
/// shared keyd and consoled, with a labelled blkd, fsd and client, boots; the client unlabelled
/// shares fsd's endpoint across two sets and is refused.
#[test]
fn confined_counts_only_a_shared_servers_own_label_set() {
    let mut m = without_volumes();
    m.confined = true;
    secrets(&mut m);
    m.principals.push(Principal {
        label_sets: vec![LabelSet { labels: vec!["alice-secrets".into()], budget: budget(64) }],
        ..alice()
    });
    let secret = || vec![String::from("alice-secrets")];
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: secret() });
    server(&mut m, "blkd").labels = secret();
    let base = server(&mut image(), "fsd:data").clone();
    m.servers.push(Server { labels: secret(), ..base.clone() });
    m.servers.push(Server {
        name: "client".into(),
        volume: None,
        labels: secret(),
        receives: vec!["client".into()],
        handed: vec![Handed { endpoint: "fsd:data".into(), badge: 7 }],
        args: vec!["endpoint=client".into(), "buckets=4".into()],
        ..base
    });
    assert!(on_virt(&m).is_ok());
    m.servers.last_mut().unwrap().labels.clear();
    let fsd = m.servers.len() - 2;
    assert_eq!(
        on_virt(&m).unwrap_err(),
        Refusal::Confined { at: format!("servers[{fsd}].receives[0]"), sharing: Sharing::Endpoint }
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

// ---- the bound on root ----

#[test]
fn a_manifest_that_passes_every_other_check_but_costs_init_too_much_is_refused() {
    let mut m = image();
    // Endpoints are cheap for system and dear for root: each is a page of root's. Eight more
    // servers receiving on 60 each fit their blocks, system and every other rule.
    for n in 0..8 {
        let receives = (0..60).map(|e| format!("e{n}-{e}")).collect();
        m.servers.push(Server {
            name: format!("s{n}"),
            program: "fsd".into(),
            budget: budget(16),
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
            program: "fsd".into(),
            receives,
            budget: budget(1),
            ..m.servers[3].clone()
        });
        m.servers.last_mut().unwrap().devices.clear();
    }
    let devices = virt_devices();
    let mut machine = machine(&devices, &ENTRIES);
    machine.system.processes_limit = u32::MAX;
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
