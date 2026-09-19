//! The shared test vectors: `vectors/example.txt` (typed messages, also run by the Elixir
//! codec on beamlet) and `vectors/9p.txt` (9P2000, for servers' conformance tests).
//! Formats are described at the top of each file.

use redoubt_wire::ninep::{self, Body, Message as NineP, Names, Qid, Qids, Stat};
use redoubt_wire::proto::example::*;
use redoubt_wire::typed::{Value, Words};
use redoubt_wire::MSIZE;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    if s == "-" {
        return Vec::new();
    }
    assert!(s.len().is_multiple_of(2), "odd hex {s}");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{}/vectors/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Lines that are not blank or comments.
fn lines(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).map(|l| l.split_whitespace().collect())
}

/// `NAME FIELD=VALUE ...`, the fields rendered as in the vector files.
fn render(m: &Message<'_>) -> String {
    let mut s = m.name().to_string();
    m.fields(&mut |name, value| {
        let v = match value {
            Value::U8(v) => v.to_string(),
            Value::U16(v) => v.to_string(),
            Value::U32(v) => v.to_string(),
            Value::U64(v) => v.to_string(),
            Value::Str(v) => format!("s:{}", hex(v.as_bytes())),
            Value::Bytes(v) => format!("b:{}", hex(v)),
        };
        s.push_str(&format!(" {name}={v}"));
    });
    s
}

/// The `ok` line for a message: what encoding it produces.
fn ok_line(m: &Message<'_>) -> String {
    let mut buf = vec![0u8; MSIZE];
    let words = m.encode(&mut buf).unwrap();
    // An inline message encodes without a buffer (every buffer message has at least 2 bytes,
    // so encoding one into an empty buffer fails).
    let inline = m.encode(&mut []).is_ok();
    let buffer = if inline { "-".to_string() } else { hex(&buf[..words[1] as usize]) };
    format!(
        "ok {} {:x} {:x} {:x} {:x} {buffer} {}",
        m.handle_names().len(),
        words[0],
        words[1],
        words[2],
        words[3],
        render(m)
    )
}

fn words(fields: &[&str]) -> Words {
    let mut w = [0u64; 4];
    for (slot, f) in w.iter_mut().zip(fields) {
        *slot = u64::from_str_radix(f, 16).unwrap();
    }
    w
}

#[test]
fn example_vectors() {
    let text = read("example.txt");
    let (mut ok, mut bad) = (0, 0);
    for f in lines(&text) {
        let handles: usize = f[1].parse().unwrap();
        let w = words(&f[2..6]);
        let buf = unhex(f[6]);
        let decoded = Message::decode(&w, &buf, handles);
        match f[0] {
            "ok" => {
                let m = decoded.unwrap_or_else(|e| panic!("{f:?}: {e:?}"));
                assert_eq!(render(&m), f[7..].join(" "), "{f:?}");
                let mut out = vec![0u8; MSIZE];
                let again = m.encode(&mut out).unwrap();
                assert_eq!(again, w, "{f:?}");
                // A buffer message re-encodes to the first word-1 bytes of what arrived.
                let len = if buf.is_empty() { 0 } else { w[1] as usize };
                assert_eq!(&out[..len], &buf[..len], "{f:?}");
                ok += 1;
            }
            "bad" => {
                let e = decoded.expect_err(&format!("{f:?} decoded"));
                assert_eq!(format!("{e:?}"), f[7], "{f:?}");
                bad += 1;
            }
            _ => panic!("bad line {f:?}"),
        }
    }
    assert!(ok >= 10 && bad >= 15, "ok {ok} bad {bad}");
}

/// The messages behind the `ok` vectors, built by hand: the vectors say what these encode
/// to, so a change in the encoder cannot slip past by rewriting the file.
fn hand_built() -> Vec<Message<'static>> {
    vec![
        Message::Ping(Ping {}),
        Message::Pong(Pong { seq: 0x0102_0304_0506_0708, flags: 0xdead_beef }),
        Message::Pong(Pong { seq: u64::MAX, flags: u32::MAX }),
        Message::Small(Small { a: 0xab, b: 0x1234 }),
        Message::Wide(Wide { a: 1, b: 2, c: 3 }),
        Message::Named(Named { id: 42, name: "fsd:data" }),
        Message::Named(Named { id: 0, name: "" }),
        Message::Named(Named { id: 7, name: "h\u{e9}llo \u{1f600}" }),
        Message::Blob(Blob { offset: 1 << 40, data: &[0, 1, 2, 0xff], label: "x" }),
        Message::Blob(Blob { offset: 0, data: &[], label: "" }),
        Message::Grant(Grant { pages: 16 }),
        Message::Last(Last { note: "max opcode" }),
    ]
}

#[test]
fn hand_built_messages_are_in_the_vectors() {
    let text = read("example.txt");
    for m in hand_built() {
        let line = ok_line(&m);
        assert!(text.lines().any(|l| l == line), "missing vector:\n{line}");
    }
}

/// Spelled out from WIRE.md by hand, independent of the encoder.
#[test]
fn layouts_by_hand() {
    let pong = Message::Pong(Pong { seq: 0x0102_0304_0506_0708, flags: 0xdead_beef });
    assert_eq!(pong.encode(&mut []).unwrap(), [2, 0x0506_0708, 0x0102_0304, 0xdead_beef]);
    let mut buf = [0u8; 16];
    let named = Message::Named(Named { id: 42, name: "ab" });
    assert_eq!(named.encode(&mut buf).unwrap(), [5, 8, 0, 0]);
    assert_eq!(buf[..8], [42, 0, 0, 0, 2, 0, b'a', b'b']);
    // Too small an output buffer is an error, not a truncation.
    assert_eq!(named.encode(&mut buf[..7]), Err(redoubt_wire::Error::TooLarge));
}

fn ninep_messages() -> Vec<NineP<'static>> {
    const STAT: Stat<'static> = Stat {
        kind: 0,
        dev: 0,
        qid: Qid { kind: 0x80, version: 3, path: 0x1234 },
        mode: 0x8000_01ed,
        atime: 1_700_000_000,
        mtime: 1_700_000_001,
        length: 0,
        name: "home",
        uid: "",
        gid: "",
        muid: "",
    };
    let qid = Qid { kind: 0, version: 1, path: 99 };
    let m = |tag, body| NineP { tag, body };
    vec![
        m(ninep::NOTAG, Body::Tversion { msize: MSIZE as u32, version: ninep::VERSION }),
        m(ninep::NOTAG, Body::Rversion { msize: MSIZE as u32, version: ninep::VERSION }),
        m(1, Body::Tauth { afid: 5, uname: "alice", aname: "" }),
        m(1, Body::Rauth { aqid: qid }),
        m(2, Body::Tattach { fid: 0, afid: ninep::NOFID, uname: "alice", aname: "/home/alice" }),
        m(2, Body::Rattach { qid: STAT.qid }),
        m(3, Body::Rerror { ename: "file does not exist" }),
        m(4, Body::Tflush { oldtag: 3 }),
        m(4, Body::Rflush),
        m(5, Body::Twalk { fid: 0, newfid: 1, wnames: Names::new(&["notes", "todo.txt"]).unwrap() }),
        m(5, Body::Rwalk { qids: Qids::new(&[STAT.qid, qid]).unwrap() }),
        m(6, Body::Twalk { fid: 0, newfid: 2, wnames: Names::new(&[]).unwrap() }),
        m(6, Body::Rwalk { qids: Qids::new(&[]).unwrap() }),
        m(7, Body::Topen { fid: 1, mode: 0 }),
        m(7, Body::Ropen { qid, iounit: (MSIZE - ninep::IOHDRSZ) as u32 }),
        m(8, Body::Tcreate { fid: 1, name: "new", perm: 0o644, mode: 1 }),
        m(8, Body::Rcreate { qid, iounit: 0 }),
        m(9, Body::Tread { fid: 1, offset: 0, count: 8192 }),
        m(9, Body::Rread { data: b"hello\n" }),
        m(10, Body::Twrite { fid: 1, offset: 1 << 33, data: &[0, 0xff] }),
        m(10, Body::Rwrite { count: 2 }),
        m(11, Body::Tclunk { fid: 1 }),
        m(11, Body::Rclunk),
        m(12, Body::Tremove { fid: 2 }),
        m(12, Body::Rremove),
        m(13, Body::Tstat { fid: 0 }),
        m(13, Body::Rstat { stat: STAT }),
        m(14, Body::Twstat { fid: 0, stat: Stat { name: "", mode: u32::MAX, ..STAT } }),
        m(14, Body::Rwstat),
    ]
}

#[test]
fn ninep_vectors() {
    let text = read("9p.txt");
    let mut expected = ninep_messages().into_iter();
    let (mut ok, mut bad) = (0, 0);
    for f in lines(&text) {
        match f[0] {
            "ok" => {
                let bytes = unhex(f[1]);
                let m = NineP::decode(&bytes).unwrap_or_else(|e| panic!("{f:?}: {e:?}"));
                assert_eq!(Some(m), expected.next(), "{f:?}");
                let mut out = vec![0u8; MSIZE];
                let n = m.encode(&mut out).unwrap();
                assert_eq!(hex(&out[..n]), f[1]);
                ok += 1;
            }
            "bad" => {
                let e = NineP::decode(&unhex(f[2])).expect_err(&format!("{f:?} decoded"));
                assert_eq!(format!("{e:?}"), f[1], "{f:?}");
                bad += 1;
            }
            _ => panic!("bad line {f:?}"),
        }
    }
    assert_eq!(expected.next(), None, "a hand-built 9P message has no vector");
    assert!(ok >= 29 && bad >= 10, "ok {ok} bad {bad}");
}

/// Prints the `ok` lines for both files; used once to write them, and to show what changed
/// if the tests above fail. `cargo test -p redoubt-wire --test vectors -- --ignored --nocapture`
#[test]
#[ignore]
fn print_ok_lines() {
    for m in hand_built() {
        println!("{}", ok_line(&m));
    }
    for m in ninep_messages() {
        let mut out = vec![0u8; MSIZE];
        let n = m.encode(&mut out).unwrap();
        println!("ok {}", hex(&out[..n]));
    }
}
