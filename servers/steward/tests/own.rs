//! The steward's own lines (servers/steward.md, "The manifest lines"): what `init` appends after
//! the core's, parsed strictly, and a network scope encoded as `ipd`'s `grant` takes it.

use redoubt_steward_server::own::{Bound, Home, How, Own, SYSTEM, Vault, binding, is_own, session_args};

fn read(lines: &[&str]) -> Result<Own, String> {
    let mut own = Own::default();
    for l in lines {
        assert!(is_own(l), "{l}");
        own.line(l)?;
    }
    Ok(own)
}

#[test]
fn each_line_reads_as_what_it_binds() {
    let own = read(&[
        "label \"alice-secrets\" id=7",
        "home \"alice\" handle=walfsd:data path=/home/alice quota=8388608",
        "vault \"alice\" labels=[9,7] handle=walfsd:alice-secrets",
        "net \"alice\" 0.0.0.0/0:22,443 10.0.0.0/8:*",
    ])
    .unwrap();
    assert_eq!(own.labels, [("alice-secrets".to_string(), 7)]);
    assert_eq!(
        own.home("alice"),
        Some(&Home {
            principal: "alice".into(),
            handle: "walfsd:data".into(),
            path: "/home/alice".into(),
            quota: 8 << 20,
        })
    );
    assert_eq!(
        own.vault("alice", &[7, 9]),
        Some(&Vault { principal: "alice".into(), labels: vec![7, 9], handle: "walfsd:alice-secrets".into() })
    );
    assert!(own.home("bob").is_none() && own.vault("alice", &[7]).is_none());
    // Three connect rules: ports 22 and 443 of every address, and every port of 10/8.
    let rule = |addr: [u8; 4], len: u8, lo: u16, hi: u16| {
        let mut r = vec![1];
        r.extend_from_slice(&addr);
        r.push(len);
        r.extend_from_slice(&lo.to_le_bytes());
        r.extend_from_slice(&hi.to_le_bytes());
        r
    };
    let mut scope = vec![3];
    scope.extend(rule([0; 4], 0, 22, 22));
    scope.extend(rule([0; 4], 0, 443, 443));
    scope.extend(rule([10, 0, 0, 0], 8, 1, 65535));
    assert_eq!(own.net("alice").unwrap().scope, scope);
}

#[test]
fn a_malformed_line_is_refused() {
    let bad = [
        "label alice-secrets id=7",
        "label \"alice-secrets\" id=7 id=8",
        "label \"alice-secrets\"",
        "label \"Alice Secrets\" id=7",
        "label \"alice-secrets\" id=+7",
        "home \"alice\" handle=walfsd:data path=home/alice",
        "home \"alice\" handle=walfsd:data path=/home/../etc",
        "home \"alice\" handle=walfsd:data",
        "home \"alice\" handle=Fsd!data path=/home/alice",
        "home \"alice\" handle=walfsd:data path=/home/alice quota=1 extra=1",
        "home \"alice\" handle=walfsd:data path=/home/alice",
        "home \"alice\" handle=walfsd:data path=/home/alice quota=0",
        "home \"alice\" handle=walfsd:data path=/home/alice quota=-1",
        "vault \"alice\" labels=[] handle=walfsd:alice-secrets",
        "vault \"alice\" labels=7 handle=walfsd:alice-secrets",
        "net \"alice\"",
        "net \"alice\" 0.0.0.0/0",
        "net \"alice\" 0.0.0.0/33:22",
        "net \"alice\" ::/0:22",
        "net \"alice\" 0.0.0.0/0:0",
        "net \"alice\" 0.0.0.0/0:65536",
        "net \"alice\" 0.0.0.0/0:1,2,3,4,5,6,7,8,9",
    ];
    for l in bad {
        assert!(read(&[l]).is_err(), "{l}");
    }
    for twice in
        ["label \"a\" id=1", "home \"alice\" handle=walfsd:data path=/a quota=1", "net \"alice\" 0.0.0.0/0:*"]
    {
        assert!(read(&[twice, twice]).is_err(), "{twice}");
    }
    assert!(read(&["vault \"alice\" labels=[7] handle=a", "vault \"alice\" labels=[7] handle=b"]).is_err());
    assert!(!is_own("principal \"alice\" account=1"));
}

/// The binding table (servers/steward.md, "Two embedders and a reference"), slot by slot, for an
/// unlabelled session and a vault session of a principal with a home, a vault and a scope, and for
/// one with none of them: what each slot binds, at which path, under which name.
#[test]
fn the_binding_table_binds_each_slot_as_the_page_says() {
    let own = read(&[
        "label \"alice-secrets\" id=7",
        "home \"alice\" handle=walfsd:data path=/home/alice quota=8388608",
        "vault \"alice\" labels=[7] handle=walfsd:alice-secrets",
        "net \"alice\" 0.0.0.0/0:22",
    ])
    .unwrap();
    let fresh = |server: &str, root: &str| How::Fresh { server: server.into(), root: root.into() };
    let at = |p: &str| Some(p.to_string());
    let boot = Some(Bound { how: fresh("bootfsd", ""), at: at("/boot"), name: Some("bootfsd") });
    // The home is minted through the one connection the steward keeps for alice, carved with her quota.
    let carved = How::Carved {
        key: "home alice".into(),
        server: "walfsd:data".into(),
        root: "/home/alice".into(),
        quota: 8 << 20,
    };
    let home = Some(Bound { how: carved, at: at("/home/alice"), name: None });
    let cons = Some(Bound { how: How::Console, at: at("/dev/cons"), name: None });
    let system = Some(Bound { how: fresh(SYSTEM, ""), at: None, name: Some(SYSTEM) });
    let scope = own.net("alice").unwrap().scope.clone();
    let net = Some(Bound { how: How::Grant { scope }, at: at("/net"), name: None });
    let vault = Some(Bound { how: fresh("walfsd:alice-secrets", ""), at: at("/vault"), name: None });
    let table = |labels: &[u64], who: &str| (0..7).map(|s| binding(&own, who, labels, s)).collect::<Vec<_>>();
    assert_eq!(
        table(&[], "alice"),
        [boot.clone(), home.clone(), None, net, cons.clone(), system.clone(), None]
    );
    assert_eq!(table(&[7], "alice"), [boot.clone(), home, vault, None, cons.clone(), system.clone(), None]);
    assert_eq!(table(&[], "bob"), [boot, None, None, None, cons, system, None]);
}

/// What a session's VM is told of itself: its principal, each label of its set by name and id,
/// and a named context; nothing for the default context or the console's session.
#[test]
fn a_sessions_arguments_name_its_principal_labels_and_context() {
    let own = read(&["label \"alice-secrets\" id=7"]).unwrap();
    assert_eq!(
        session_args(&own, "alice", &[7], Some("work")),
        ["principal=alice", "label=alice-secrets:7", "context=work"]
    );
    assert_eq!(session_args(&own, "alice", &[], Some("")), ["principal=alice"]);
    assert_eq!(session_args(&own, "bob", &[], None), ["principal=bob"]);
}
