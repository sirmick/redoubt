//! The manifest lines (servers/steward.md, "The manifest lines"): the strict parser the steward
//! reads its arguments with and a trace begins with, the writer `init` and the model use, and the
//! key id both sides derive.

use redoubt_steward::hash::key_id;
use redoubt_steward::manifest::{Limits, Manifest, PrincipalSpec, Sizes, lines, parse_lines};

const SIZES: &str = "sizes session=10,2,5 agent=10,2,5 sub_agent=5,1,2 crossing=2,1,1 cost=1";
const ALICE: &str =
    "principal \"alice\" account=1 login=[11] approval=[21] owned=[7] sets=[[],[7]] top=1000,30,300";

fn parse(text: &[&str]) -> Result<Manifest, String> { parse_lines(text.iter().copied()) }

#[test]
fn the_lines_read_as_the_manifest_and_write_back_the_same() {
    let m = parse(&[ALICE, "keyd [31]", "servers 4", SIZES]).unwrap();
    let l = |pages, processes, weight| Limits { pages, processes, weight };
    let want = Manifest {
        principals: vec![PrincipalSpec {
            name: "alice".into(),
            account: 1,
            login_keys: vec![11],
            approval_keys: vec![21],
            owned: vec![7],
            label_sets: vec![vec![], vec![7]],
            top: l(1000, 30, 300),
        }],
        keyd_keys: vec![31],
        servers: 4,
        sizes: Sizes {
            session: l(10, 2, 5),
            agent: l(10, 2, 5),
            sub_agent: l(5, 1, 2),
            crossing: l(2, 1, 1),
            budget_cost: 1,
        },
    };
    assert_eq!(m, want);
    assert_eq!(lines(&m), [ALICE, "keyd [31]", "servers 4", SIZES]);
}

#[test]
fn keyd_and_servers_are_empty_when_absent_and_sizes_is_required() {
    let m = parse(&[SIZES]).unwrap();
    assert!(m.principals.is_empty() && m.keyd_keys.is_empty() && m.servers == 0);
    assert_eq!(parse(&[ALICE]).unwrap_err(), "no `sizes` line");
}

#[test]
fn a_malformed_line_is_refused_with_its_number() {
    let principal = |fields: &str| format!("principal \"alice\" {fields}");
    let ok = "account=1 login=[11] approval=[21] owned=[7] sets=[[]] top=1,1,1";
    let cases = [
        principal(&format!("{ok} account=2")),
        principal(&format!("{ok} extra=1")),
        principal("account=1 login=[11] approval=[21] owned=[7] sets=[[]]"),
        principal(&ok.replace("account=1", "account=+1")),
        principal(&ok.replace("account=1", "account=")),
        principal(&ok.replace("top=1,1,1", "top=1,1,1,1")),
        principal(&ok.replace("top=1,1,1", "top=1,1")),
        principal(&ok.replace("sets=[[]]", "sets=[[]][[7]]")),
        principal(&ok.replace("sets=[[]]", "sets=[[],]")),
        principal(&ok.replace("sets=[[]]", "sets=[[x]]")),
        principal(&ok.replace("login=[11]", "login=[11,]")),
        principal(&ok.replace("login=[11]", "login=11")),
        principal(ok).replace("\"alice\"", "alice"),
        principal(ok).replace("\"alice\"", "\"al\"ice\""),
        principal(ok).replace("\"alice\"", "\"a\\q\""),
        principal(ok).replace("\"alice\"", "\"a\\x4\""),
        principal(ok).replace("\"alice\"", "\"\\xff\""),
        principal(ok).replace("\"alice\"", "\"alice"),
        "principal".into(),
        "keyd".into(),
        "keyd [1] [2]".into(),
        "keyd [1]\u{0}".into(),
        "servers 65536".into(),
        "servers -1".into(),
        "servers".into(),
        SIZES.replace(" cost=1", ""),
        SIZES.replace("cost=1", "cost=1 cost=1"),
        "".into(),
        "# a comment".into(),
        "users 3".into(),
    ];
    for bad in &cases {
        let err = parse(&["servers 1", bad, SIZES]).expect_err(bad);
        assert!(err.starts_with("line 2: "), "{bad}: {err}");
    }
    for twice in ["keyd [1]", "servers 1", SIZES] {
        let err = parse(&[twice, twice, SIZES]).unwrap_err();
        assert!(err.starts_with("line 2: a second"), "{twice}: {err}");
    }
}

#[test]
fn a_name_with_any_bytes_writes_back_the_same() {
    let name = "al\"i\\ce\n\u{e9}";
    let mut m = parse(&[ALICE, SIZES]).unwrap();
    m.principals[0].name = name.into();
    let written = lines(&m);
    assert_eq!(parse_lines(written.iter().map(String::as_str)).unwrap(), m);
}

/// The vector is SHA-256 of 32 zero bytes: its first eight bytes, read little-endian.
#[test]
fn the_key_id_is_the_first_eight_bytes_of_the_key_s_sha_256_little_endian() {
    assert_eq!(key_id(&[0; 32]), 0x77bd_62f8_ad7a_6866);
    let mut k = [0; 32];
    k[31] = 1;
    assert_ne!(key_id(&k), key_id(&[0; 32]));
}

/// The fuzz campaign's kept corpus (`fuzz/seeds/lines`), rerun.
#[test]
fn the_kept_corpus_reads_or_is_refused_without_a_panic() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds/lines");
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        redoubt_steward::fuzz::lines_one(&std::fs::read(entry.unwrap().path()).unwrap());
        n += 1;
    }
    assert!(n > 0);
}
