//! `ipd`'s arguments (NAMESPACES.md): strict, all or nothing.

use redoubt_ipd::args::{BadArgs, Config, DEFAULT_BUCKETS, Limit, parse};
use redoubt_ipd::scope::{Ports, Prefix, Rule, ip};

fn run(args: &[&str]) -> Result<Config, BadArgs> { parse(args.iter().copied()) }

/// The rig's arguments (tests/net) and the milestone manifest's, as NAMESPACES.md gives them.
const RIG: &[&str] = &[
    "addr=10.0.2.15/24",
    "gateway=10.0.2.2",
    "self=10.0.2.0/24",
    "self=10.0.9.102/32",
    "ingress=3",
    "scope=4:c:0.0.0.0/0:1-65535,l:1-65535",
    "buckets=4",
];

const MILESTONE: &[&str] = &[
    "addr=10.0.2.15/24",
    "gateway=10.0.2.2",
    "self=10.0.2.0/24",
    "ingress=3",
    "scope=4:c:0.0.0.0/0:1-65535",
    "scope=5:l:22",
    "buckets=6",
    "limits=5:23:0:20",
    "limits=4:2:32:0",
];

#[test]
fn the_rig_and_the_milestone_parse() {
    let c = run(RIG).unwrap();
    assert_eq!((c.addr, c.len, c.gateway), (ip(10, 0, 2, 15), 24, Some(ip(10, 0, 2, 2))));
    assert_eq!(
        c.selfs,
        vec![Prefix::new(ip(10, 0, 2, 0), 24).unwrap(), Prefix::new(ip(10, 0, 9, 102), 32).unwrap()]
    );
    assert_eq!(c.ingress, 3);
    assert_eq!(c.buckets, 4);
    let scope = c.scope_of(4).unwrap();
    assert_eq!(scope.rules()[1], Rule::Listen(Ports::new(1, 65535).unwrap()));

    let m = run(MILESTONE).unwrap();
    assert_eq!(m.scope_of(5).unwrap().rules(), &[Rule::Listen(Ports::new(22, 22).unwrap())]);
    assert_eq!(m.limits[0], Limit { badge: 5, in_flight: 23, state: 0, sockets: 20 });
    assert_eq!(run(&["addr=10.0.2.15/24", "ingress=3"]).unwrap().buckets, DEFAULT_BUCKETS);
}

#[test]
fn anything_else_stops_ipd() {
    let bad: &[&[&str]] = &[
        &[],
        &["ingress=3"],
        &["addr=10.0.2.15/24"],
        &["addr=10.0.2.15/24", "ingress=3", "mystery=1"],
        &["addr=10.0.2.15/24", "ingress=3", "noequals"],
        // Not a unicast host address on a network.
        &["addr=10.0.2.15", "ingress=3"],
        &["addr=10.0.2.0/24", "ingress=3"],
        &["addr=10.0.2.255/24", "ingress=3"],
        &["addr=127.0.0.1/8", "ingress=3"],
        &["addr=224.0.0.1/24", "ingress=3"],
        &["addr=0.1.2.3/8", "ingress=3"],
        &["addr=10.0.2.15/31", "ingress=3"],
        &["addr=10.0.2.15/0", "ingress=3"],
        &["addr=10.0.2.15/33", "ingress=3"],
        &["addr=10.0.2.015/24", "ingress=3"],
        &["addr=10.0.2.256/24", "ingress=3"],
        &["addr=10.0.2/24", "ingress=3"],
        &["addr=10.0.2.15/24", "addr=10.0.3.15/24", "ingress=3"],
        // The gateway: unicast, on the network, not the box, not its network or broadcast.
        &["addr=10.0.2.15/24", "gateway=10.0.3.1", "ingress=3"],
        &["addr=10.0.2.15/24", "gateway=10.0.2.15", "ingress=3"],
        &["addr=10.0.2.15/24", "gateway=10.0.2.255", "ingress=3"],
        &["addr=10.0.2.15/24", "gateway=10.0.2.0", "ingress=3"],
        &["addr=10.0.2.15/24", "gateway=10.0.2.2", "gateway=10.0.2.3", "ingress=3"],
        // Badges: nonzero, below 2^63, once each.
        &["addr=10.0.2.15/24", "ingress=0"],
        &["addr=10.0.2.15/24", "ingress=9223372036854775808"],
        &["addr=10.0.2.15/24", "ingress=03"],
        &["addr=10.0.2.15/24", "ingress=3", "ingress=4"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=3:l:22"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:l:22", "scope=4:l:23"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=9223372036854775808:l:22"],
        // Rules and prefixes: canonical, well formed, at most 8.
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:c:10.0.0.1/8:1-2"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:c:10.0.0.0/8:2-1"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:c:10.0.0.0/8:0-1"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:c:10.0.0.0/8:1-65536"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:x:22"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:l:22,l:22,l:22,l:22,l:22,l:22,l:22,l:22,l:22"],
        &["addr=10.0.2.15/24", "ingress=3", "self=10.0.2.1/24"],
        // Buckets and limits.
        &["addr=10.0.2.15/24", "ingress=3", "buckets=0"],
        &["addr=10.0.2.15/24", "ingress=3", "buckets=33"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:l:22", "limits=5:2:2:2"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:l:22", "limits=4:2:2"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:l:22", "limits=4:65:2:2"],
        &["addr=10.0.2.15/24", "ingress=3", "scope=4:l:22", "limits=4:2:2:2", "limits=4:2:2:2"],
    ];
    for args in bad {
        assert!(run(args).is_err(), "{args:?} was accepted");
    }
    // Nine of any repeatable argument is one too many.
    let mut many = vec!["addr=10.0.2.15/24", "ingress=3"];
    many.extend(std::iter::repeat_n("self=10.0.9.0/24", 9));
    assert!(run(&many).is_err());
}
