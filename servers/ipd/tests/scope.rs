//! The capability (servers/ipd.md, "Scopes and grants"): containment, canonical encoding, grants
//! that only narrow, and the box's own addresses. Property sweeps with a seeded generator, as
//! netd's tests use (no property-testing crate in the tree): each property is checked on tens of
//! thousands of cases.

use redoubt_ipd::scope::{
    MAX_RULES, Ports, Prefix, RULE_BYTES, Rule, Scope, SelfSet, ip, martian_source, mask,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 { self.next() % n }

    fn addr(&mut self) -> u32 {
        // Mostly from a few small networks, so prefixes overlap often.
        match self.below(4) {
            0 => self.next() as u32,
            1 => ip(10, 0, self.below(4) as u8, self.below(256) as u8),
            2 => ip(10, self.below(2) as u8, 0, 0) | self.below(1 << 16) as u32,
            _ => ip(192, 168, 1, self.below(256) as u8),
        }
    }

    fn prefix(&mut self) -> Prefix {
        let len = [0, 8, 16, 24, 30, 32, self.below(33) as u8][self.below(7) as usize];
        Prefix::new(self.addr() & mask(len), len).unwrap()
    }

    fn port(&mut self) -> u16 {
        [1, 22, 80, 443, 1024, 8000, 65535, 1 + self.below(65535) as u16][self.below(8) as usize]
    }

    fn ports(&mut self) -> Ports {
        let (a, b) = (self.port(), self.port());
        Ports::new(a.min(b), a.max(b)).unwrap()
    }

    fn rule(&mut self) -> Rule {
        if self.below(3) == 0 {
            Rule::Listen(self.ports())
        } else {
            Rule::Connect(self.prefix(), self.ports())
        }
    }

    fn scope(&mut self) -> Scope {
        let n = self.below(MAX_RULES as u64 + 1) as usize;
        let rules: Vec<Rule> = (0..n).map(|_| self.rule()).collect();
        Scope::new(&rules).unwrap()
    }

    /// A scope inside `held`: each rule narrowed from one of its rules.
    fn narrower(&mut self, held: &Scope) -> Scope {
        if held.rules().is_empty() {
            return Scope::default();
        }
        let n = 1 + self.below(MAX_RULES as u64) as usize;
        let rules: Vec<Rule> = (0..n)
            .map(|_| {
                let r = held.rules()[self.below(held.rules().len() as u64) as usize];
                match r {
                    Rule::Listen(p) => Rule::Listen(self.within(p)),
                    Rule::Connect(pre, p) => {
                        let len = pre.len() + self.below(u64::from(33 - pre.len())) as u8;
                        let addr = (pre.addr() | (self.next() as u32 & !mask(pre.len()))) & mask(len);
                        Rule::Connect(Prefix::new(addr, len).unwrap(), self.within(p))
                    }
                }
            })
            .collect();
        Scope::new(&rules).unwrap()
    }

    fn within(&mut self, p: Ports) -> Ports {
        let span = u64::from(p.hi() - p.lo()) + 1;
        let a = p.lo() + self.below(span) as u16;
        let b = p.lo() + self.below(span) as u16;
        Ports::new(a.min(b), a.max(b)).unwrap()
    }
}

/// Whether some rule of `scope` allows exactly this (brute force, the specification).
fn allows(scope: &Scope, connect: bool, addr: u32, port: u16) -> bool {
    scope.rules().iter().any(|r| match *r {
        Rule::Connect(p, ports) => {
            connect && addr & mask(p.len()) == p.addr() && ports.lo() <= port && port <= ports.hi()
        }
        Rule::Listen(ports) => !connect && ports.lo() <= port && port <= ports.hi(),
    })
}

#[test]
fn a_scope_permits_exactly_what_its_rules_contain() {
    let mut r = Rng(0x1234_5678_9abc_def1);
    for _ in 0..20_000 {
        let s = r.scope();
        for _ in 0..20 {
            let (addr, port) = (r.addr(), r.port());
            assert_eq!(s.permits_connect(addr, port), allows(&s, true, addr, port), "{s:?} {addr:#x}:{port}");
            assert_eq!(s.permits_listen(port), allows(&s, false, 0, port), "{s:?} listen {port}");
        }
    }
}

/// A grant built inside the held scope is accepted; and whatever is accepted permits nothing the
/// held scope does not (checked on the rules' corners and random points).
#[test]
fn a_grant_only_narrows() {
    let mut r = Rng(0x0bad_cafe_f00d_d00d);
    let mut accepted = 0;
    for _ in 0..20_000 {
        let held = r.scope();
        let inside = r.narrower(&held);
        assert!(held.narrows(&inside), "{held:?} refused its own narrowing {inside:?}");
        let asked = if r.below(2) == 0 { inside } else { r.scope() };
        if !held.narrows(&asked) {
            continue;
        }
        accepted += 1;
        for rule in asked.rules() {
            let (connect, points): (bool, Vec<(u32, u16)>) = match *rule {
                Rule::Connect(p, ports) => {
                    let last = p.addr() | !mask(p.len());
                    (
                        true,
                        vec![
                            (p.addr(), ports.lo()),
                            (last, ports.hi()),
                            (p.addr(), ports.hi()),
                            (last, ports.lo()),
                        ],
                    )
                }
                Rule::Listen(ports) => (false, vec![(0, ports.lo()), (0, ports.hi())]),
            };
            for (addr, port) in points {
                assert!(
                    allows(&held, connect, addr, port),
                    "{asked:?} from {held:?} widened to {addr:#x}:{port}"
                );
            }
        }
        for _ in 0..10 {
            let (addr, port) = (r.addr(), r.port());
            assert!(!asked.permits_connect(addr, port) || held.permits_connect(addr, port));
            assert!(!asked.permits_listen(port) || held.permits_listen(port));
        }
    }
    assert!(accepted > 10_000, "the sweep accepted too few grants to mean anything ({accepted})");
}

#[test]
fn a_grant_never_widens_or_adds_listen() {
    let held =
        Scope::new(&[Rule::Connect(Prefix::new(ip(10, 0, 0, 0), 8).unwrap(), Ports::new(443, 443).unwrap())])
            .unwrap();
    let wider = [
        Rule::Connect(Prefix::new(0, 0).unwrap(), Ports::new(443, 443).unwrap()),
        Rule::Connect(Prefix::new(ip(10, 0, 0, 0), 7).unwrap(), Ports::new(443, 443).unwrap()),
        Rule::Connect(Prefix::new(ip(11, 0, 0, 0), 8).unwrap(), Ports::new(443, 443).unwrap()),
        Rule::Connect(Prefix::new(ip(10, 0, 0, 0), 8).unwrap(), Ports::new(443, 444).unwrap()),
        Rule::Connect(Prefix::new(ip(10, 0, 0, 0), 8).unwrap(), Ports::new(80, 443).unwrap()),
        Rule::Listen(Ports::new(443, 443).unwrap()),
    ];
    for rule in wider {
        assert!(!held.narrows(&Scope::new(&[rule]).unwrap()), "{rule:?}");
        // Nor hidden behind a rule that is inside.
        let both = Scope::new(&[held.rules()[0], rule]).unwrap();
        assert!(!held.narrows(&both), "{rule:?} with an inside rule");
    }
    assert!(held.narrows(&Scope::default()), "the empty scope is inside anything");
}

/// The wire form: canonical in, canonical out, and nothing else decodes.
#[test]
fn the_encoding_is_canonical_and_round_trips() {
    let mut r = Rng(0xfeed_face_dead_beef);
    for _ in 0..20_000 {
        let s = r.scope();
        let bytes = s.encode();
        assert_eq!(bytes.len(), 1 + s.rules().len() * RULE_BYTES);
        assert_eq!(Scope::decode(&bytes), Ok(s.clone()));
        // Every one-byte change either decodes to a scope that encodes back to the same bytes,
        // or is refused: there is one encoding per scope.
        let mut bad = bytes.clone();
        let at = r.below(bad.len() as u64) as usize;
        bad[at] ^= 1 << r.below(8);
        if let Ok(other) = Scope::decode(&bad) {
            assert_eq!(other.encode(), bad);
        }
    }
    let refused: &[&[u8]] = &[
        &[],
        &[1],
        &[9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        // Host bits set; length past 32; lo > hi; an unknown kind; listen with an address.
        &[1, 1, 10, 0, 0, 1, 8, 1, 0, 1, 0],
        &[1, 1, 10, 0, 0, 0, 33, 1, 0, 1, 0],
        &[1, 1, 10, 0, 0, 0, 8, 2, 0, 1, 0],
        &[1, 3, 10, 0, 0, 0, 8, 1, 0, 1, 0],
        &[1, 2, 10, 0, 0, 0, 0, 1, 0, 1, 0],
        &[1, 2, 0, 0, 0, 0, 8, 1, 0, 1, 0],
        // One byte short, one byte over.
        &[1, 1, 10, 0, 0, 0, 8, 1, 0, 1],
        &[1, 1, 10, 0, 0, 0, 8, 1, 0, 1, 0, 0],
    ];
    for bytes in refused {
        assert!(Scope::decode(bytes).is_err(), "{bytes:?}");
    }
}

/// The box's own addresses, under a scope of everything (servers/ipd.md R59: checked before the
/// scope).
#[test]
fn the_self_set() {
    let set = SelfSet::new(
        ip(10, 0, 2, 15),
        24,
        &[Prefix::new(ip(10, 0, 2, 0), 24).unwrap(), Prefix::new(ip(10, 0, 9, 102), 32).unwrap()],
    );
    let own = [
        ip(10, 0, 2, 15),
        ip(10, 0, 2, 0),
        ip(10, 0, 2, 255),
        ip(10, 0, 2, 2),
        ip(10, 0, 2, 3),
        ip(10, 0, 9, 102),
        ip(255, 255, 255, 255),
        ip(127, 0, 0, 1),
        ip(127, 255, 255, 254),
        ip(0, 0, 0, 0),
        ip(0, 255, 0, 1),
        ip(224, 0, 0, 1),
        ip(239, 1, 2, 3),
        ip(240, 0, 0, 0),
        ip(254, 1, 1, 1),
    ];
    for addr in own {
        assert!(set.contains(addr), "{addr:#x}");
    }
    for addr in [
        ip(10, 0, 9, 100),
        ip(10, 0, 9, 101),
        ip(10, 0, 3, 1),
        ip(8, 8, 8, 8),
        ip(223, 255, 255, 255),
        ip(1, 0, 0, 0),
    ] {
        assert!(!set.contains(addr), "{addr:#x}");
    }
    // Without the QEMU entries, the rest of the /24 is someone else's.
    let bare = SelfSet::new(ip(192, 168, 1, 10), 24, &[]);
    assert!(
        bare.contains(ip(192, 168, 1, 10))
            && bare.contains(ip(192, 168, 1, 255))
            && bare.contains(ip(192, 168, 1, 0))
    );
    assert!(!bare.contains(ip(192, 168, 1, 1)));
    // Martian sources: the box, loopback, "this host"; the gateway is not one.
    let own = ip(10, 0, 2, 15);
    for src in [own, ip(127, 0, 0, 1), ip(0, 0, 0, 0), ip(0, 9, 9, 9)] {
        assert!(martian_source(src, own), "{src:#x}");
    }
    assert!(!martian_source(ip(10, 0, 2, 2), own));
}
