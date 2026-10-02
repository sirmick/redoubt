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

fn on_virt(m: &Manifest) -> Result<Plan, Refusal> {
    let devices = virt_devices();
    check(m, &machine(&devices, &ENTRIES), BUNDLE_KEY)
}

fn on(m: &Manifest, machine: &Machine) -> Result<Plan, Refusal> { check(m, machine, BUNDLE_KEY) }

fn refused_at(m: &Manifest, path: &str, why: Why) {
    assert_eq!(on_virt(m).unwrap_err(), Refusal::At { at: path.into(), why });
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
    assert_eq!(plan.keys, vec![BUNDLE_KEY]);
    // keyd, consoled and bootfsd: init alone calls them; ipd: netd's badge.
    assert_eq!(plan.buckets, vec![(0, 1), (1, 1), (2, 1), (5, 1)]);
    // No handed item names keyd, consoled or bootfsd: init's own badge at each is 1.
    assert_eq!(plan.init_badges, vec![(0, 1), (1, 1), (2, 1)]);
}

#[test]
fn init_s_own_badge_is_the_smallest_no_handed_item_uses_there() {
    let mut m = image();
    for (holder, badge) in [("ipd", 1), ("netd", 2), ("blkd", 4)] {
        server(&mut m, holder).handed.push(Handed { endpoint: "consoled".into(), badge });
    }
    server(&mut m, "bootfsd").handed.push(Handed { endpoint: "keyd".into(), badge: 2 });
    let plan = on_virt(&m).unwrap();
    assert_eq!(plan.init_badges, vec![(0, 1), (1, 3), (2, 1)]);
}

#[test]
fn the_image_manifest_s_bound() {
    let plan = on_virt(&image()).unwrap();
    // The arena (256 + 3 tables), 6 receive and 6 exit endpoints, 6 process objects, 6 blocks
    // with 3 tables each, 6 watching threads (an IPC page, 4 stack pages and 3 tables each), the
    // largest launch (stub 4 + 3, ipd's 147 pages + 3, stack 16 + 3), and no handle-table page: 22 handles at
    // the start (3 budgets, the Reset right, 18 devices) and 6 + 2 + 24 + 3 = 35 added still fit page 0.
    let devices = virt_devices();
    let m = machine(&devices, &ENTRIES);
    assert_eq!(m.handles_at_start, 22);
    assert_eq!(plan.bound, 259 + 12 + 6 + 24 + 48 + (4 + 3 + 147 + 3 + 16 + 3));
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
    m.servers[0].program = "fsd".into();
    refused_at(&m, "servers[0].program", Why::Unknown);
    let mut m = image();
    m.servers[0].program = MANIFEST.into();
    refused_at(&m, "servers[0].program", Why::Unknown);
    let mut m = image();
    server(&mut m, "ipd").handed[0].endpoint = "fsd".into();
    refused_at(&m, "servers[5].handed[0].endpoint", Why::Unknown);
    let mut m = image();
    m.servers[0].volume = Some("data".into());
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
    m.principals.push(Principal { home: Some("data:/home/alice".into()), ..alice() });
    refused_at(&m, "principals[0].home", Why::Value);
    m.volumes.push(Volume { name: "data".into(), partition: 0, labels: vec![] });
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
    // 256 + 512 * 3 + 1024 + 4096 pages, and a page each for the budgets.
    let pages = 256 + 512 * 3 + 1024 + 4096 + 6;
    machine.system.pages_limit = machine.system.pages_usage + pages - 1;
    assert_eq!(
        on(&m, &machine).unwrap_err(),
        Refusal::SystemFit { what: "pages", need: pages, free: pages - 1 }
    );
    machine.system.pages_limit += 1;
    assert!(on(&m, &machine).is_ok());
    machine.system.processes_usage = machine.system.processes_limit - 5;
    assert_eq!(on(&m, &machine).unwrap_err(), Refusal::SystemFit { what: "processes", need: 6, free: 5 });
    machine.system.processes_usage = 0;
    machine.system.weight_carved = machine.system.weight_limit - 3299;
    assert_eq!(on(&m, &machine).unwrap_err(), Refusal::SystemFit { what: "weight", need: 3300, free: 3299 });
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

// ---- startup blocks ----

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
    assert_eq!(on_virt(&m).unwrap().keys, vec![counting, approval, BUNDLE_KEY]);
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
    let mut m = confined();
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

#[test]
fn confined_refuses_a_driver_serving_two_label_sets() {
    let mut m = confined();
    secrets(&mut m);
    m.principals.push(Principal {
        label_sets: vec![LabelSet { labels: vec!["alice-secrets".into()], budget: budget(64) }],
        ..alice()
    });
    // Only consoled is shared: its device reaches both of alice's sets.
    for s in &mut m.servers {
        if s.program != "consoled" {
            s.args.retain(|a| !a.starts_with("buckets="));
        }
    }
    assert_eq!(sharing(&m), Sharing::Device);
}

#[test]
fn confined_refuses_a_server_instance_serving_two_label_sets() {
    let mut m = confined();
    secrets(&mut m);
    m.principals.push(Principal {
        label_sets: vec![LabelSet { labels: vec!["alice-secrets".into()], budget: budget(64) }],
        ..alice()
    });
    for s in &mut m.servers {
        if s.program != "keyd" {
            s.args.retain(|a| !a.starts_with("buckets="));
        }
    }
    assert_eq!(sharing(&m), Sharing::Server);
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
            program: "keyd".into(),
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
