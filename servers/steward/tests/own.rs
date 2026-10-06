//! The steward's own lines (servers/steward.md, "The manifest lines"): what `init` appends after
//! the core's, parsed strictly, and a network scope encoded as `ipd`'s `grant` takes it.

use redoubt_steward_server::own::{Home, Own, Vault, is_own};

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
        "home \"alice\" handle=littlefsd:data path=/home/alice",
        "vault \"alice\" labels=[9,7] handle=littlefsd:alice-secrets",
        "net \"alice\" 0.0.0.0/0:22,443 10.0.0.0/8:*",
    ])
    .unwrap();
    assert_eq!(own.labels, [("alice-secrets".to_string(), 7)]);
    assert_eq!(
        own.home("alice"),
        Some(&Home {
            principal: "alice".into(),
            handle: "littlefsd:data".into(),
            path: "/home/alice".into()
        })
    );
    assert_eq!(
        own.vault("alice", &[7, 9]),
        Some(&Vault {
            principal: "alice".into(),
            labels: vec![7, 9],
            handle: "littlefsd:alice-secrets".into()
        })
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
        "home \"alice\" handle=littlefsd:data path=home/alice",
        "home \"alice\" handle=littlefsd:data path=/home/../etc",
        "home \"alice\" handle=littlefsd:data",
        "home \"alice\" handle=Fsd!data path=/home/alice",
        "home \"alice\" handle=littlefsd:data path=/home/alice extra=1",
        "vault \"alice\" labels=[] handle=littlefsd:alice-secrets",
        "vault \"alice\" labels=7 handle=littlefsd:alice-secrets",
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
        ["label \"a\" id=1", "home \"alice\" handle=littlefsd:data path=/a", "net \"alice\" 0.0.0.0/0:*"]
    {
        assert!(read(&[twice, twice]).is_err(), "{twice}");
    }
    assert!(read(&["vault \"alice\" labels=[7] handle=a", "vault \"alice\" labels=[7] handle=b"]).is_err());
    assert!(!is_own("principal \"alice\" account=1"));
}
