//! Every line on the console says who wrote it (servers/consoled.md, "Started by `init`"):
//! [`Lines`] against a buffer standing in for the UART, which may take less than it is given.

use redoubt_consoled::{Lines, PREFIX_LEN, prefix};

/// `init`'s own badge at `consoled`: a root badge, so its lines go out bare.
const ROOT: u64 = 1;
/// Two minted connections, as two children of `init` hold, with the ids their requester got.
const A: (u64, u64) = (0x4000_0000_0000_0001, 0x0123_4567_89ab_cdef);
const B: (u64, u64) = (0x4000_0000_0000_0002, 0x0000_0000_0000_00b2);

/// The bytes that went out.
#[derive(Default)]
struct Uart(Vec<u8>);

impl Uart {
    fn write(&mut self, lines: &mut Lines, (badge, id): (u64, Option<u64>), data: &[u8]) -> usize {
        lines.write(badge, id, data, &mut |bytes| {
            self.0.extend_from_slice(bytes);
            bytes.len()
        })
    }

    fn text(&self) -> &str { std::str::from_utf8(&self.0).unwrap() }
}

fn root() -> (u64, Option<u64>) { (ROOT, None) }
fn a() -> (u64, Option<u64>) { (A.0, Some(A.1)) }
fn b() -> (u64, Option<u64>) { (B.0, Some(B.1)) }

#[test]
fn the_prefix_is_the_id_in_sixteen_hex_digits() {
    assert_eq!(&prefix(A.1), b"[con 0123456789abcdef] ");
    assert_eq!(&prefix(B.1), b"[con 00000000000000b2] ");
    assert_eq!(&prefix(u64::MAX), b"[con ffffffffffffffff] ");
    assert_eq!(prefix(0).len(), PREFIX_LEN);
}

#[test]
fn a_root_line_is_bare_and_a_minted_one_carries_its_id_on_every_line() {
    let (mut lines, mut uart) = (Lines::new(), Uart::default());
    assert_eq!(uart.write(&mut lines, root(), b"init: up\n"), 9);
    assert_eq!(uart.write(&mut lines, a(), b"one\ntwo\n"), 8);
    assert_eq!(uart.write(&mut lines, a(), b""), 0);
    assert_eq!(uart.text(), "init: up\n[con 0123456789abcdef] one\n[con 0123456789abcdef] two\n");
}

#[test]
fn a_line_is_continued_by_its_writer_and_ended_by_any_other() {
    let (mut lines, mut uart) = (Lines::new(), Uart::default());
    uart.write(&mut lines, a(), b"par");
    uart.write(&mut lines, a(), b"tial");
    // `b` cuts in: `a`'s line ends first, and `b`'s starts with its own id.
    uart.write(&mut lines, b(), b"init: started x, console 0000000000000001");
    // So does `init`: its line is bare, on a line of its own.
    uart.write(&mut lines, root(), b"init: done\n");
    // And `a` comes back to a fresh line.
    uart.write(&mut lines, a(), b"more\n");
    assert_eq!(
        uart.text(),
        "[con 0123456789abcdef] partial\n\
         [con 00000000000000b2] init: started x, console 0000000000000001\n\
         init: done\n\
         [con 0123456789abcdef] more\n"
    );
}

/// The UART may take less than it is given, a prefix's bytes included. Whatever the cuts, every
/// line is one writer's: a bare line holds only `init`'s bytes, and a prefixed one only the bytes
/// of the connection whose id it carries. Each writer writes only its own letter, so a line's
/// bytes say whose they are.
#[test]
fn whatever_the_uart_takes_no_line_mixes_writers_or_carries_another_s_id() {
    let writers = [(root(), b'r'), (a(), b'a'), (b(), b'b')];
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..200 {
        let (mut lines, mut out) = (Lines::new(), Vec::new());
        let mut written = [0usize; 3];
        for _ in 0..60 {
            let w = next() as usize % 3;
            let (who, letter) = writers[w];
            let data: Vec<u8> =
                (0..next() % 12).map(|_| if next() % 4 == 0 { b'\n' } else { letter }).collect();
            // The UART takes at most `room` bytes in this call, sometimes none.
            let mut room = (next() % 30) as usize;
            let took = lines.write(who.0, who.1, &data, &mut |bytes| {
                let n = bytes.len().min(room);
                room -= n;
                out.extend_from_slice(&bytes[..n]);
                n
            });
            assert!(took <= data.len());
            written[w] += data[..took].iter().filter(|&&c| c == letter).count();
        }
        let text = String::from_utf8(out).unwrap();
        let mut seen = [0usize; 3];
        for line in text.split('\n') {
            let (w, body) = if line.starts_with('[') {
                // A prefix cut short, then ended by another writer or the end of the test: the
                // start of one connection's own prefix, and nothing of anyone's after it.
                let Some((id, body)) = line.strip_prefix("[con ").and_then(|rest| rest.split_once("] "))
                else {
                    assert!([A.1, B.1].iter().any(|&id| prefix(id).starts_with(line.as_bytes())), "{line:?}");
                    continue;
                };
                let id = u64::from_str_radix(id, 16).unwrap();
                (
                    if id == A.1 {
                        1
                    } else if id == B.1 {
                        2
                    } else {
                        panic!("{line:?}")
                    },
                    body,
                )
            } else {
                (0, line)
            };
            let letter = writers[w].1;
            assert!(body.bytes().all(|c| c == letter), "{line:?} in {text:?}");
            seen[w] += body.len();
        }
        assert_eq!(seen, written, "every byte reported written went out, once");
    }
}
