//! The startup block (INIT.md): one page a parent writes for its child, naming the handles it
//! installed in the child's slots 1..=n (`process_start`). The parent may be hostile, so parsing
//! bounds everything, checks everything up front, and never panics.
//!
//! # Format
//!
//! One typed message, `startup` (INIT.md's table; its codec is generated into
//! [`redoubt_wire::proto::startup`]), laid out as a typed operation written into a file is: the
//! opcode as a `u32`, then the buffer-shape encoding of its fields (WIRE.md). In front of it, the
//! page holds the message's length (a `u32`, QUESTIONS.md 112); the rest of the page is not read.
//!
//! | Field | Holds |
//! | --- | --- |
//! | `version` | 1 |
//! | `handle_count` | n, the handles `process_start` installed (slots 1..=n), at most `MAX_START_HANDLES` |
//! | `namespace` | entries `handle: u32`, `path: string`: a clean absolute path (`/`, `/dev/cons`) |
//! | `handles` | entries `handle: u32`, `name: string`: a named handle, the name under the manifest's rule ([`valid_name`]) |
//! | `argv` | `string`s, the arguments in order (each may be empty) |
//!
//! Handles are 1..=n. Paths are unique among `namespace` entries and names among `handles`
//! entries; each `bytes` field holds whole entries and nothing else. A block breaking any rule is
//! refused whole.

use alloc::vec::Vec;

use redoubt_sys::{Handle, MAX_START_HANDLES, PAGE_SIZE};
use redoubt_wire::codec::Reader;
use redoubt_wire::proto::startup::{Message, Startup as Fields};

use crate::path;

/// The largest block: one page.
pub const MAX_BLOCK: usize = PAGE_SIZE;
const VERSION: u32 = 1;
/// The longest handle name (INIT.md, Names).
pub const MAX_NAME: usize = 64;

/// Why a startup block was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupError {
    /// The page holds fewer bytes than the block's length says.
    Short,
    /// The block's length is impossible (beyond the page), or the page is misaligned.
    BadLength,
    /// The message does not decode as `startup`: a wrong opcode, bad lengths, trailing bytes.
    Malformed(redoubt_wire::Error),
    BadVersion,
    /// A handle outside 1..=n, or n over `MAX_START_HANDLES`.
    BadHandle,
    /// A path that is not clean and absolute, or a name outside the manifest's rule.
    BadString,
    /// A path or name given twice.
    Duplicate,
    /// Longer than [`MAX_BLOCK`], or out of memory for its entries.
    TooLarge,
    /// `image_addr`/`image_len` break INIT.md's rule: one is 0 and the other is not,
    /// `image_addr` is not page-aligned, `image_addr + image_len` overflows, or either does not
    /// fit this target's `usize` (rv32).
    BadImage,
}

/// Whether `name` may name a handle: the boot manifest's rule (INIT.md, Names; answer 64), 1-64
/// bytes of `[a-z0-9_:+-]` starting with a letter, so no empty name, NUL, U+FEFF or control
/// character reaches anything that uses it.
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.len() <= MAX_NAME
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:+-".contains(b))
}

/// QUESTIONS.md 112 (pending): how the message sits in its page. A typed message has no overall
/// length and its decoder refuses trailing bytes, but the rest of the page is not read, so the
/// page starts with the message's length in bytes, a little-endian `u32`, and the message
/// follows. Reading and writing the frame are both here.
mod frame {
    use super::{MAX_BLOCK, StartupError};

    /// The length word's size.
    pub const HEADER: usize = 4;

    /// The message in `page`: exactly the bytes its length names.
    pub fn read(page: &[u8]) -> Result<&[u8], StartupError> {
        let word = page.get(..HEADER).ok_or(StartupError::Short)?;
        let len = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        let len = usize::try_from(len).map_err(|_| StartupError::BadLength)?;
        if len > MAX_BLOCK - HEADER {
            return Err(StartupError::BadLength);
        }
        page.get(HEADER..HEADER + len).ok_or(StartupError::Short)
    }

    /// Writes the length of the message already at `page[HEADER..HEADER + len]`.
    pub fn write(page: &mut [u8], len: usize) -> Result<(), StartupError> {
        let word = u32::try_from(len).map_err(|_| StartupError::TooLarge)?;
        page.get_mut(..HEADER).ok_or(StartupError::TooLarge)?.copy_from_slice(&word.to_le_bytes());
        Ok(())
    }
}

/// One entry, as parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry<'a> {
    Namespace(&'a str, Handle),
    Handle(&'a str, Handle),
    Arg(&'a str),
}

/// A parsed startup block: its entries, validated by [`Startup::parse`].
#[derive(Clone, Debug)]
pub struct Startup<'a> {
    entries: Vec<Entry<'a>>,
    /// `(image_addr, image_len)`, `None` when the block named no image (INIT.md, Startup block).
    image: Option<(usize, usize)>,
}

impl<'a> Startup<'a> {
    /// A process started with no block.
    pub const EMPTY: Startup<'static> = Startup { entries: Vec::new(), image: None };

    /// Parses the block at the front of `page` (at most [`MAX_BLOCK`] bytes of it are looked at).
    pub fn parse(page: &'a [u8]) -> Result<Startup<'a>, StartupError> {
        let page = page.get(..MAX_BLOCK).unwrap_or(page);
        let message = frame::read(page)?;
        let Message::Startup(fields) = Message::decode_file(message).map_err(StartupError::Malformed)?;
        Self::from_fields(&fields)
    }

    fn from_fields(fields: &Fields<'a>) -> Result<Startup<'a>, StartupError> {
        if fields.version != VERSION {
            return Err(StartupError::BadVersion);
        }
        // `process_start` installs at most MAX_START_HANDLES handles, so no more can be named.
        let count = fields.handle_count;
        if count as usize > MAX_START_HANDLES {
            return Err(StartupError::BadHandle);
        }
        let handle = |raw: u32| match Handle::new(raw) {
            Some(handle) if raw <= count => Ok(handle),
            _ => Err(StartupError::BadHandle),
        };
        let mut entries: Vec<Entry<'a>> = Vec::new();
        let mut r = Reader::new(fields.namespace);
        while !r.rest().is_empty() {
            let (raw, path) = handle_and_string(&mut r)?;
            if !path::is_clean_absolute(path) {
                return Err(StartupError::BadString);
            }
            push(&mut entries, Entry::Namespace(path, handle(raw)?))?;
        }
        let mut r = Reader::new(fields.handles);
        while !r.rest().is_empty() {
            let (raw, name) = handle_and_string(&mut r)?;
            if !valid_name(name) {
                return Err(StartupError::BadString);
            }
            push(&mut entries, Entry::Handle(name, handle(raw)?))?;
        }
        let mut r = Reader::new(fields.argv);
        while !r.rest().is_empty() {
            push(&mut entries, Entry::Arg(r.string().map_err(StartupError::Malformed)?))?;
        }
        let image = parse_image(fields.image_addr, fields.image_len)?;
        Ok(Startup { entries, image })
    }

    fn entries(&self) -> impl Iterator<Item = Entry<'a>> + '_ { self.entries.iter().copied() }

    /// `(image_addr, image_len)`, the program image the loader stub loads (INIT.md, Startup
    /// block; PACKAGES.md, Launching a process): the address of its first byte and its exact
    /// byte length, both already checked non-overflowing and page-aligned. `None` for a process
    /// started at its own entry rather than through the stub.
    pub fn image(&self) -> Option<(usize, usize)> { self.image }

    /// The namespace table: (path, handle), in block order.
    pub fn namespace(&self) -> impl Iterator<Item = (&'a str, Handle)> + '_ {
        self.entries().filter_map(|e| if let Entry::Namespace(path, h) = e { Some((path, h)) } else { None })
    }

    /// The handle named `name`.
    pub fn handle(&self, name: &str) -> Option<Handle> {
        self.entries().find_map(|e| match e {
            Entry::Handle(n, h) if n == name => Some(h),
            _ => None,
        })
    }

    /// The arguments, in order.
    pub fn args(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.entries().filter_map(|e| if let Entry::Arg(arg) = e { Some(arg) } else { None })
    }

    /// Resolves the clean absolute `path` against the namespace table: the entry with the
    /// longest matching prefix, and what is left of `path` after it (no leading `/`).
    pub fn resolve<'p>(&self, path: &'p str) -> Option<(Handle, &'p str)> {
        let mut best: Option<(usize, Handle, &'p str)> = None;
        for (prefix, handle) in self.namespace() {
            let rest = if prefix == "/" {
                path.strip_prefix('/')
            } else {
                path.strip_prefix(prefix)
                    .and_then(|r| if r.is_empty() { Some(r) } else { r.strip_prefix('/') })
            };
            if let Some(rest) = rest {
                if best.is_none_or(|(len, _, _)| prefix.len() > len) {
                    best = Some((prefix.len(), handle, rest));
                }
            }
        }
        best.map(|(_, handle, rest)| (handle, rest))
    }
}

/// INIT.md's `image_addr`/`image_len` rule: `image_addr` is 0 exactly when `image_len` is, is
/// page-aligned, and `image_addr + image_len` does not overflow. Both 0 is "no image".
fn parse_image(image_addr: u64, image_len: u64) -> Result<Option<(usize, usize)>, StartupError> {
    if image_addr == 0 && image_len == 0 {
        return Ok(None);
    }
    if image_addr == 0 || image_len == 0 {
        return Err(StartupError::BadImage);
    }
    let addr = usize::try_from(image_addr).map_err(|_| StartupError::BadImage)?;
    let len = usize::try_from(image_len).map_err(|_| StartupError::BadImage)?;
    if !addr.is_multiple_of(PAGE_SIZE) {
        return Err(StartupError::BadImage);
    }
    addr.checked_add(len).ok_or(StartupError::BadImage)?;
    Ok(Some((addr, len)))
}

/// One `handle: u32`, `string` entry.
fn handle_and_string<'a>(r: &mut Reader<'a>) -> Result<(u32, &'a str), StartupError> {
    let raw = r.u32().map_err(StartupError::Malformed)?;
    Ok((raw, r.string().map_err(StartupError::Malformed)?))
}

/// Adds `entry`, refusing a path or name already given. A page holds a few hundred entries at
/// most, so the quadratic check is small; no memory for one more is a refusal.
fn push<'a>(entries: &mut Vec<Entry<'a>>, entry: Entry<'a>) -> Result<(), StartupError> {
    let repeated = entries.iter().any(|old| match (old, &entry) {
        (Entry::Namespace(a, _), Entry::Namespace(b, _)) | (Entry::Handle(a, _), Entry::Handle(b, _)) => {
            a == b
        }
        _ => false,
    });
    if repeated {
        return Err(StartupError::Duplicate);
    }
    entries.try_reserve(1).map_err(|_| StartupError::TooLarge)?;
    entries.push(entry);
    Ok(())
}

/// Writes a startup block, for launchers (`init`, the steward) and tests. [`finish`] parses the
/// result, so a builder can only produce blocks the parser accepts.
///
/// **A launcher never passes its own connection to a child** (answer 50): every handle named in a
/// child's namespace is a fresh connection the server made for that child (`new_connection`,
/// [`crate::client::Client::new_connection`]), which the launcher disconnects when the child
/// exits. A copied connection would share the launcher's fids and admission with the child.
///
/// [`finish`]: StartupBuilder::finish
pub struct StartupBuilder {
    handles: u32,
    namespace: Vec<u8>,
    names: Vec<u8>,
    argv: Vec<u8>,
    image_addr: u64,
    image_len: u64,
    /// A string too long for its `u16` length, remembered for `finish`.
    too_long: bool,
}

impl StartupBuilder {
    /// A block for a child given `handles` handles (its slots 1..=handles), naming no image.
    pub fn new(handles: u32) -> StartupBuilder {
        StartupBuilder {
            handles,
            namespace: Vec::new(),
            names: Vec::new(),
            argv: Vec::new(),
            image_addr: 0,
            image_len: 0,
            too_long: false,
        }
    }

    /// Names the program image the loader stub loads at `addr..addr + len` (INIT.md, Startup
    /// block): `addr` must be page-aligned for [`finish`](Self::finish) to accept it.
    pub fn image(&mut self, addr: usize, len: usize) -> &mut Self {
        self.image_addr = addr as u64;
        self.image_len = len as u64;
        self
    }

    /// Appends a `string`: its `u16` length and its bytes.
    fn string(out: &mut Vec<u8>, text: &str, too_long: &mut bool) {
        let len = u16::try_from(text.len()).unwrap_or_else(|_| {
            *too_long = true;
            0
        });
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(text.as_bytes().get(..usize::from(len)).unwrap_or(&[]));
    }

    pub fn namespace(&mut self, path: &str, handle: Handle) -> &mut Self {
        self.namespace.extend_from_slice(&handle.index().to_le_bytes());
        Self::string(&mut self.namespace, path, &mut self.too_long);
        self
    }

    pub fn handle(&mut self, name: &str, handle: Handle) -> &mut Self {
        self.names.extend_from_slice(&handle.index().to_le_bytes());
        Self::string(&mut self.names, name, &mut self.too_long);
        self
    }

    pub fn arg(&mut self, arg: &str) -> &mut Self {
        Self::string(&mut self.argv, arg, &mut self.too_long);
        self
    }

    /// The block's bytes (the page's used part), checked by [`Startup::parse`].
    pub fn finish(&self) -> Result<Vec<u8>, StartupError> {
        if self.too_long {
            return Err(StartupError::TooLarge);
        }
        let message = Message::Startup(Fields {
            version: VERSION,
            handle_count: self.handles,
            namespace: &self.namespace,
            handles: &self.names,
            argv: &self.argv,
            image_addr: self.image_addr,
            image_len: self.image_len,
        });
        let mut page = alloc::vec![0; MAX_BLOCK];
        let body = page.get_mut(frame::HEADER..).ok_or(StartupError::TooLarge)?;
        let len = message.encode_file(body).map_err(|_| StartupError::TooLarge)?;
        frame::write(&mut page, len)?;
        page.truncate(frame::HEADER + len);
        Startup::parse(&page)?;
        Ok(page)
    }
}

#[cfg(test)]
mod tests {

    use alloc::vec;

    use super::*;

    fn h(i: u32) -> Handle { Handle::new(i).unwrap() }

    fn sample() -> Vec<u8> {
        StartupBuilder::new(4)
            .namespace("/", h(1))
            .namespace("/dev/cons", h(2))
            .handle("keys", h(3))
            .handle("budget", h(4))
            .arg("--verbose")
            .arg("")
            .arg("naïve")
            .finish()
            .unwrap()
    }

    #[test]
    fn round_trip() {
        let bytes = sample();
        let s = Startup::parse(&bytes).unwrap();
        assert_eq!(s.namespace().collect::<Vec<_>>(), vec![("/", h(1)), ("/dev/cons", h(2))]);
        assert_eq!(s.handle("keys"), Some(h(3)));
        assert_eq!(s.handle("budget"), Some(h(4)));
        assert_eq!(s.handle("nope"), None);
        assert_eq!(s.args().collect::<Vec<_>>(), vec!["--verbose", "", "naïve"]);
        assert_eq!(s.image(), None);
        // The rest of the page is not read.
        let mut page = bytes.clone();
        page.resize(MAX_BLOCK, 0xaa);
        assert_eq!(Startup::parse(&page).unwrap().args().count(), 3);
    }

    /// The page is the length word and then exactly INIT.md's `startup` message, as the
    /// generated codec writes it: opcode 1, then the fields.
    #[test]
    fn the_page_is_the_wire_message() {
        let bytes = StartupBuilder::new(1).namespace("/", h(1)).arg("a").finish().unwrap();
        let mut want = vec![];
        want.extend_from_slice(&1u32.to_le_bytes()); // opcode
        want.extend_from_slice(&1u32.to_le_bytes()); // version
        want.extend_from_slice(&1u32.to_le_bytes()); // handle_count
        want.extend_from_slice(&7u32.to_le_bytes()); // namespace: 7 bytes
        want.extend_from_slice(&[1, 0, 0, 0, 1, 0, b'/']);
        want.extend_from_slice(&0u32.to_le_bytes()); // handles: none
        want.extend_from_slice(&3u32.to_le_bytes()); // argv: 3 bytes
        want.extend_from_slice(&[1, 0, b'a']);
        want.extend_from_slice(&0u64.to_le_bytes()); // image_addr: none
        want.extend_from_slice(&0u64.to_le_bytes()); // image_len: none
        assert_eq!(bytes[..4], (want.len() as u32).to_le_bytes());
        assert_eq!(bytes[4..], want[..]);
    }

    #[test]
    fn image_round_trips_and_is_validated() {
        let bytes = StartupBuilder::new(0).image(PAGE_SIZE, 42).finish().unwrap();
        assert_eq!(Startup::parse(&bytes).unwrap().image(), Some((PAGE_SIZE, 42)));

        // image_addr is 0 exactly when image_len is: one without the other is refused.
        assert_eq!(StartupBuilder::new(0).image(PAGE_SIZE, 0).finish().err(), Some(StartupError::BadImage));
        assert_eq!(StartupBuilder::new(0).image(0, 1).finish().err(), Some(StartupError::BadImage));
        // Not page-aligned.
        assert_eq!(StartupBuilder::new(0).image(1, 1).finish().err(), Some(StartupError::BadImage));
        // Overflows.
        assert_eq!(
            StartupBuilder::new(0).image(usize::MAX & !(PAGE_SIZE - 1), PAGE_SIZE).finish().err(),
            Some(StartupError::BadImage)
        );
    }

    #[test]
    fn resolve_takes_the_longest_prefix() {
        let bytes = sample();
        let s = Startup::parse(&bytes).unwrap();
        assert_eq!(s.resolve("/dev/cons"), Some((h(2), "")));
        assert_eq!(s.resolve("/dev/consx"), Some((h(1), "dev/consx")));
        assert_eq!(s.resolve("/dev/cons/x"), Some((h(2), "x")));
        assert_eq!(s.resolve("/home/a"), Some((h(1), "home/a")));
        assert_eq!(s.resolve("relative"), None);
        assert_eq!(Startup::EMPTY.resolve("/"), None);
    }

    #[test]
    fn handle_names_follow_the_manifest_rule() {
        for good in ["keys", "budget", "fsd:data", "alice+secrets", "a-b_c9", &"a".repeat(MAX_NAME)] {
            assert!(valid_name(good), "{good:?}");
        }
        let long = "a".repeat(MAX_NAME + 1);
        for bad in ["", "9p", "Keys", "_x", "a b", "a\0", "a\u{feff}", "a\u{85}", "é", "a/b", "a.b", &long] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn hostile_blocks_are_refused() {
        let good = sample();
        let reject = |bytes: &[u8]| Startup::parse(bytes).err();
        assert_eq!(reject(&[]), Some(StartupError::Short));
        assert_eq!(reject(&good[..3]), Some(StartupError::Short));
        assert_eq!(reject(&good[..good.len() - 1]), Some(StartupError::Short));
        // A length past the page, and one that leaves bytes of the message out.
        let mut bad = good.clone();
        bad[..4].copy_from_slice(&(MAX_BLOCK as u32 - 3).to_le_bytes());
        assert_eq!(reject(&bad), Some(StartupError::BadLength));
        bad[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(reject(&bad), Some(StartupError::BadLength));
        let mut cut = good.clone();
        let len = u32::from_le_bytes(cut[..4].try_into().unwrap());
        cut[..4].copy_from_slice(&(len - 1).to_le_bytes());
        assert!(matches!(reject(&cut), Some(StartupError::Malformed(_))));
        // A wrong opcode or version.
        let mut bad = good.clone();
        bad[4] = 2;
        assert!(matches!(reject(&bad), Some(StartupError::Malformed(_))));
        let mut bad = good.clone();
        bad[8] = 2;
        assert_eq!(reject(&bad), Some(StartupError::BadVersion));
        // Each builder step the parser must refuse, and why.
        type Case<'a> = (&'a dyn Fn(&mut StartupBuilder), Option<StartupError>);
        let cases: [Case; 11] = [
            (&|s| _ = s.namespace("/", h(3)), Some(StartupError::BadHandle)),
            (&|s| _ = s.handle("x", h(3)), Some(StartupError::BadHandle)),
            (&|s| _ = s.namespace("/a/../b", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.namespace("dev", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.handle("", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.handle("a\0", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.handle("x", h(1)).handle("x", h(2)), Some(StartupError::Duplicate)),
            (&|s| _ = s.namespace("/", h(1)).namespace("/", h(2)), Some(StartupError::Duplicate)),
            (&|s| _ = s.arg(&"x".repeat(MAX_BLOCK)), Some(StartupError::TooLarge)),
            (&|s| _ = s.arg(&"x".repeat(usize::from(u16::MAX) + 1)), Some(StartupError::TooLarge)),
            // One handle may be both a path and a name.
            (&|s| _ = s.namespace("/a", h(1)).handle("a", h(1)), None),
        ];
        for (i, (build, expected)) in cases.iter().enumerate() {
            let mut builder = StartupBuilder::new(2);
            build(&mut builder);
            assert_eq!(builder.finish().err(), *expected, "case {i}");
        }
    }

    /// Each `bytes` field holds whole entries and nothing else.
    #[test]
    fn fields_hold_whole_entries() {
        let fields = |namespace: &'static [u8], handles: &'static [u8], argv: &'static [u8]| Fields {
            version: VERSION,
            handle_count: 1,
            namespace,
            handles,
            argv,
            image_addr: 0,
            image_len: 0,
        };
        let ok = fields(&[1, 0, 0, 0, 1, 0, b'/'], &[1, 0, 0, 0, 1, 0, b'k'], &[0, 0]);
        assert!(Startup::from_fields(&ok).is_ok());
        for bad in [
            fields(&[1, 0, 0, 0, 1, 0, b'/', 0], &[], &[]),
            fields(&[1, 0, 0, 0, 2, 0, b'/'], &[], &[]),
            fields(&[1, 0, 0], &[], &[]),
            fields(&[], &[1, 0, 0, 0, 1, 0], &[]),
            fields(&[], &[], &[1]),
            fields(&[], &[], &[0, 0, 0]),
        ] {
            assert!(matches!(Startup::from_fields(&bad), Err(StartupError::Malformed(_))), "{bad:?}");
        }
    }

    #[test]
    fn handle_counts_are_what_process_start_can_install() {
        let most = MAX_START_HANDLES as u32;
        assert!(StartupBuilder::new(most).handle("last", h(most)).finish().is_ok());
        assert_eq!(StartupBuilder::new(most + 1).finish().err(), Some(StartupError::BadHandle));
        assert_eq!(StartupBuilder::new(u32::MAX).finish().err(), Some(StartupError::BadHandle));
    }

    #[test]
    fn random_bytes_never_panic() {
        // A tiny xorshift: deterministic, no dependency.
        let mut x = 0x2545_f491_4f6c_dd1d_u64;
        let good = sample();
        for round in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let mut bytes = if round % 2 == 0 { good.clone() } else { vec![0; (x % 256) as usize] };
            for (i, byte) in bytes.iter_mut().enumerate() {
                if (x >> (i % 64)) & 7 == 0 {
                    *byte = (x >> 8) as u8 ^ i as u8;
                }
            }
            let _ = Startup::parse(&bytes);
        }
    }
}
