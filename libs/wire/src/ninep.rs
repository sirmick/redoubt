//! Plain 9P2000 (no `.u`, no `.L`), as in Plan 9's intro(5), with `msize` fixed at 64 KiB
//! (servers/wire.md). A message travels in a lent buffer: `size[4] type[1] tag[2]` and the body.
//!
//! [`Message::decode`] returns borrowed views into the buffer; [`Message::encode`] writes
//! into one. Decoding is strict: the size field must frame the message exactly, every
//! string must be UTF-8, a walk has at most [`MAXWELEM`] elements, and a stat's own size
//! fields must agree with its contents. So `encode(decode(b)) == b` for every accepted `b`
//! (the fuzz target checks it).
//!
//! What the codec does not judge: whether a walk name is `..` or contains `/`, whether a
//! fid is in use, whether `version` is "9P2000". Those are protocol state and belong to the
//! server (userland/sessions.md: servers clean paths themselves).

use crate::codec::{Error, Reader, Writer};
use crate::MSIZE;

/// The only version string we speak.
pub const VERSION: &str = "9P2000";
/// The tag of `Tversion`.
pub const NOTAG: u16 = 0xffff;
/// "No fid", e.g. the `afid` of an unauthenticated `Tattach`.
pub const NOFID: u32 = 0xffff_ffff;
/// Most names in one `Twalk`, and qids in one `Rwalk`.
pub const MAXWELEM: usize = 16;
/// `size[4] type[1] tag[2]`.
pub const HEADER: usize = 7;
/// Room for the header of `Twrite`/`Rread` around their data: the most data one read or
/// write carries is `MSIZE - IOHDRSZ`.
pub const IOHDRSZ: usize = 24;

/// A file's server-unique identity: `type[1] vers[4] path[8]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Qid {
    /// 9P's `type` (QTDIR, QTAPPEND, ...).
    pub kind: u8,
    pub version: u32,
    pub path: u64,
}

/// How one field of a message is read and written. Every field type of every message
/// below implements it, so the `messages!` table at the end of this file is the whole codec.
pub trait Field<'a>: Sized {
    fn read(r: &mut Reader<'a>) -> Result<Self, Error>;
    fn write(&self, w: &mut Writer<'_>) -> Result<(), Error>;
}

macro_rules! int_fields {
    ($($t:ident)*) => {$(
        impl Field<'_> for $t {
            fn read(r: &mut Reader<'_>) -> Result<Self, Error> { r.$t() }
            fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> { w.$t(*self) }
        }
    )*};
}
int_fields!(u8 u16 u32 u64);

impl<'a> Field<'a> for &'a str {
    fn read(r: &mut Reader<'a>) -> Result<Self, Error> { r.string() }
    fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> { w.string(self) }
}

impl<'a> Field<'a> for &'a [u8] {
    fn read(r: &mut Reader<'a>) -> Result<Self, Error> { r.bytes() }
    fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> { w.bytes(self) }
}

impl Field<'_> for Qid {
    fn read(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Qid { kind: r.u8()?, version: r.u32()?, path: r.u64()? })
    }

    fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> {
        w.u8(self.kind)?;
        w.u32(self.version)?;
        w.u64(self.path)
    }
}

/// A directory entry, as in `Rstat`, `Twstat` and directory reads:
/// `size[2] type[2] dev[4] qid[13] mode[4] atime[4] mtime[4] length[8] name[s] uid[s]
/// gid[s] muid[s]`, where `size` counts the bytes after itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat<'a> {
    /// 9P's `type` (for kernel use in Plan 9; zero here).
    pub kind: u16,
    pub dev: u32,
    pub qid: Qid,
    pub mode: u32,
    pub atime: u32,
    pub mtime: u32,
    pub length: u64,
    pub name: &'a str,
    pub uid: &'a str,
    pub gid: &'a str,
    pub muid: &'a str,
}

impl<'a> Stat<'a> {
    /// Reads one directory entry: a stat whose leading size covers exactly its fields.
    pub fn read_entry(r: &mut Reader<'a>) -> Result<Self, Error> {
        let size = usize::from(r.u16()?);
        let mut s = Reader::new(r.take(size)?);
        let stat = Stat {
            kind: s.u16()?,
            dev: s.u32()?,
            qid: Qid::read(&mut s)?,
            mode: s.u32()?,
            atime: s.u32()?,
            mtime: s.u32()?,
            length: s.u64()?,
            name: s.string()?,
            uid: s.string()?,
            gid: s.string()?,
            muid: s.string()?,
        };
        s.finish()?;
        Ok(stat)
    }

    /// Writes one directory entry with its leading size (a directory read is these,
    /// concatenated). Atomic: an entry that does not fit leaves the writer unchanged, so a
    /// server can fill a read with whole entries until one fails.
    pub fn write_entry(&self, w: &mut Writer<'_>) -> Result<(), Error> {
        w.atomic(|w| {
            let at = w.position();
            w.u16(0)?;
            w.u16(self.kind)?;
            w.u32(self.dev)?;
            self.qid.write(w)?;
            w.u32(self.mode)?;
            w.u32(self.atime)?;
            w.u32(self.mtime)?;
            w.u64(self.length)?;
            w.string(self.name)?;
            w.string(self.uid)?;
            w.string(self.gid)?;
            w.string(self.muid)?;
            patch_len16(w, at)
        })
    }
}

/// Patches the `u16` at `at` with the number of bytes written after it.
fn patch_len16(w: &mut Writer<'_>, at: usize) -> Result<(), Error> {
    let start = at.checked_add(2).ok_or(Error::TooLarge)?;
    let len = w.position().checked_sub(start).ok_or(Error::TooLarge)?;
    w.patch(at, &u16::try_from(len).map_err(|_| Error::TooLarge)?.to_le_bytes())
}

/// As a message field (`stat[n]` in `Rstat` and `Twstat`): a `u16` count, then one stat of
/// exactly that many bytes. In directory data a stat has no such count: [`Stat::read_entry`].
impl<'a> Field<'a> for Stat<'a> {
    fn read(r: &mut Reader<'a>) -> Result<Self, Error> {
        let n = usize::from(r.u16()?);
        let mut s = Reader::new(r.take(n)?);
        let stat = Stat::read_entry(&mut s)?;
        s.finish()?;
        Ok(stat)
    }

    fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> {
        w.atomic(|w| {
            let at = w.position();
            w.u16(0)?;
            self.write_entry(w)?;
            patch_len16(w, at)
        })
    }
}

/// The stats in the data of a directory read, one after another. Iteration stops after the
/// first malformed entry (which is yielded as an error), so a hostile server cannot make a
/// client loop: every step consumes at least two bytes or ends.
pub fn stats(data: &[u8]) -> Stats<'_> {
    Stats { r: Reader::new(data), failed: false }
}

/// Iterator returned by [`stats`].
#[derive(Debug, Clone)]
pub struct Stats<'a> {
    r: Reader<'a>,
    failed: bool,
}

impl<'a> Iterator for Stats<'a> {
    type Item = Result<Stat<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.r.rest().is_empty() {
            return None;
        }
        let stat = Stat::read_entry(&mut self.r);
        self.failed = stat.is_err();
        Some(stat)
    }
}

/// The names of a `Twalk`: at most [`MAXWELEM`].
///
/// Equality is derived: sound because the slots past `len` are always `""` (both `new` and
/// `read` start from an all-default array and the fields are private).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Names<'a> {
    len: usize,
    items: [&'a str; MAXWELEM],
}

impl<'a> Names<'a> {
    pub fn new(names: &[&'a str]) -> Result<Self, Error> {
        let mut items = [""; MAXWELEM];
        items.get_mut(..names.len()).ok_or(Error::TooManyElements)?.copy_from_slice(names);
        Ok(Names { len: names.len(), items })
    }

    pub fn as_slice(&self) -> &[&'a str] {
        self.items.get(..self.len).unwrap_or(&[])
    }
}

/// `nwname[2] nwname*(wname[s])`.
impl<'a> Field<'a> for Names<'a> {
    fn read(r: &mut Reader<'a>) -> Result<Self, Error> {
        let n = usize::from(r.u16()?);
        let mut items = [""; MAXWELEM];
        for name in items.get_mut(..n).ok_or(Error::TooManyElements)? {
            *name = r.string()?;
        }
        Ok(Names { len: n, items })
    }

    fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> {
        w.u16(u16::try_from(self.len).map_err(|_| Error::TooManyElements)?)?;
        self.as_slice().iter().try_for_each(|name| w.string(name))
    }
}

/// The qids of an `Rwalk`: at most [`MAXWELEM`].
///
/// Equality is derived: sound because the slots past `len` are always `Qid::default()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Qids {
    len: usize,
    items: [Qid; MAXWELEM],
}

impl Qids {
    pub fn new(qids: &[Qid]) -> Result<Self, Error> {
        let mut items = [Qid::default(); MAXWELEM];
        items.get_mut(..qids.len()).ok_or(Error::TooManyElements)?.copy_from_slice(qids);
        Ok(Qids { len: qids.len(), items })
    }

    pub fn as_slice(&self) -> &[Qid] {
        self.items.get(..self.len).unwrap_or(&[])
    }
}

/// `nwqid[2] nwqid*(qid[13])`.
impl Field<'_> for Qids {
    fn read(r: &mut Reader<'_>) -> Result<Self, Error> {
        let n = usize::from(r.u16()?);
        let mut items = [Qid::default(); MAXWELEM];
        for qid in items.get_mut(..n).ok_or(Error::TooManyElements)? {
            *qid = Qid::read(r)?;
        }
        Ok(Qids { len: n, items })
    }

    fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> {
        w.u16(u16::try_from(self.len).map_err(|_| Error::TooManyElements)?)?;
        self.as_slice().iter().try_for_each(|qid| qid.write(w))
    }
}

/// A tagged 9P message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message<'a> {
    pub tag: u16,
    pub body: Body<'a>,
}

/// The size of the message at the front of `buf`, from its size field, checked against
/// [`HEADER`], [`MSIZE`] and the buffer. Bytes after it (the rest of a lent buffer) are not
/// part of the message.
pub fn message_size(buf: &[u8]) -> Result<usize, Error> {
    let size = usize::try_from(Reader::new(buf).u32()?).map_err(|_| Error::BadSize)?;
    if !(HEADER..=MSIZE).contains(&size) {
        return Err(Error::BadSize);
    }
    if size > buf.len() {
        return Err(Error::Short);
    }
    Ok(size)
}

/// Defines [`Body`] and the codec from one table of `Name = type { field: Type, ... }`.
/// Written out by hand, decode and encode are two 27-arm matches that must agree field for
/// field; generated from one table, decoding reads the fields in order and encoding writes
/// them in order, so they cannot disagree and `encode(decode(b)) == b` holds by construction.
macro_rules! messages {
    ($($(#[$doc:meta])* $name:ident = $kind:literal $({ $($field:ident : $ty:ty),* })?),* $(,)?) => {
        /// One 9P2000 message body; field names follow intro(5).
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Body<'a> {
            $($(#[$doc])* $name $({ $($field: $ty),* })?),*
        }

        impl Body<'_> {
            /// The message's type byte.
            pub fn kind(&self) -> u8 {
                match self { $(Body::$name { .. } => $kind),* }
            }
        }

        impl<'a> Message<'a> {
            /// Decodes the message at the front of `buf` (see [`message_size`]).
            pub fn decode(buf: &'a [u8]) -> Result<Self, Error> {
                let size = message_size(buf)?;
                let mut r = Reader::new(buf.get(4..size).ok_or(Error::Short)?);
                let kind = r.u8()?;
                let tag = r.u16()?;
                let body = match kind {
                    $($kind => Body::$name $({ $($field: Field::read(&mut r)?),* })?,)*
                    _ => return Err(Error::BadType),
                };
                r.finish()?;
                Ok(Message { tag, body })
            }

            /// Encodes into the front of `out` and returns the message's size. Fails with
            /// `TooLarge` if `out` is too small or the message would exceed [`MSIZE`]; a
            /// failed encode leaves the bytes it touched zeroed, never half a message.
            pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
                Writer::new(out).atomic(|w| {
                    w.u32(0)?; // size, patched below
                    w.u8(self.body.kind())?;
                    w.u16(self.tag)?;
                    match &self.body {
                        $(Body::$name $({ $($field),* })? => { $($(Field::write($field, w)?;)*)? })*
                    }
                    let size = w.position();
                    if size > MSIZE {
                        return Err(Error::TooLarge);
                    }
                    w.patch(0, &u32::try_from(size).map_err(|_| Error::TooLarge)?.to_le_bytes())?;
                    Ok(size)
                })
            }
        }
    };
}

messages! {
    Tversion = 100 { msize: u32, version: &'a str },
    Rversion = 101 { msize: u32, version: &'a str },
    Tauth = 102 { afid: u32, uname: &'a str, aname: &'a str },
    Rauth = 103 { aqid: Qid },
    Tattach = 104 { fid: u32, afid: u32, uname: &'a str, aname: &'a str },
    Rattach = 105 { qid: Qid },
    /// `Terror` (106) is not a message and is refused.
    Rerror = 107 { ename: &'a str },
    Tflush = 108 { oldtag: u16 },
    Rflush = 109,
    Twalk = 110 { fid: u32, newfid: u32, wnames: Names<'a> },
    Rwalk = 111 { qids: Qids },
    Topen = 112 { fid: u32, mode: u8 },
    Ropen = 113 { qid: Qid, iounit: u32 },
    Tcreate = 114 { fid: u32, name: &'a str, perm: u32, mode: u8 },
    Rcreate = 115 { qid: Qid, iounit: u32 },
    Tread = 116 { fid: u32, offset: u64, count: u32 },
    Rread = 117 { data: &'a [u8] },
    Twrite = 118 { fid: u32, offset: u64, data: &'a [u8] },
    Rwrite = 119 { count: u32 },
    Tclunk = 120 { fid: u32 },
    Rclunk = 121,
    Tremove = 122 { fid: u32 },
    Rremove = 123,
    Tstat = 124 { fid: u32 },
    Rstat = 125 { stat: Stat<'a> },
    Twstat = 126 { fid: u32, stat: Stat<'a> },
    Rwstat = 127,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(m: &Message<'_>, bytes: &[u8]) {
        let mut out = [0u8; 256];
        let n = m.encode(&mut out).unwrap();
        assert_eq!(&out[..n], bytes);
        assert_eq!(Message::decode(bytes).unwrap(), *m);
    }

    // Spelled out byte by byte from intro(5), independent of the encoder.
    #[test]
    fn tversion_bytes() {
        let m = Message { tag: NOTAG, body: Body::Tversion { msize: 65536, version: VERSION } };
        round_trip(&m, &[19, 0, 0, 0, 100, 0xff, 0xff, 0, 0, 1, 0, 6, 0, b'9', b'P', b'2', b'0', b'0', b'0']);
    }

    #[test]
    fn twalk_bytes() {
        let m = Message {
            tag: 1,
            body: Body::Twalk { fid: 2, newfid: 3, wnames: Names::new(&["a", "bc"]).unwrap() },
        };
        #[rustfmt::skip]
        let bytes = [24, 0, 0, 0, 110, 1, 0, 2, 0, 0, 0, 3, 0, 0, 0, 2, 0, 1, 0, b'a', 2, 0, b'b', b'c'];
        round_trip(&m, &bytes);
    }

    #[test]
    fn rstat_sizes_nest() {
        let stat = Stat {
            kind: 0,
            dev: 0,
            qid: Qid { kind: 0x80, version: 1, path: 2 },
            mode: 0o755 | 0x8000_0000,
            atime: 3,
            mtime: 4,
            length: 0,
            name: "d",
            uid: "",
            gid: "",
            muid: "",
        };
        let m = Message { tag: 5, body: Body::Rstat { stat } };
        let mut out = [0u8; 128];
        let n = m.encode(&mut out).unwrap();
        // stat body: 39 fixed bytes + 4 strings (2 + 1 + 2 + 2 + 2 = 9) = 48.
        assert_eq!(&out[7..11], &[50, 0, 48, 0]);
        assert_eq!(n, 7 + 2 + 50);
        assert_eq!(Message::decode(&out[..n]).unwrap(), m);
        // An inner size that disagrees with n[2] is refused, both ways.
        let mut bad = out;
        bad[9] = 47;
        assert!(Message::decode(&bad[..n]).is_err());
        bad[9] = 49;
        assert!(Message::decode(&bad[..n]).is_err());
    }

    #[test]
    fn framing_is_strict() {
        let tclunk = [11, 0, 0, 0, 120, 0, 0, 7, 0, 0, 0];
        assert!(Message::decode(&tclunk).is_ok());
        // A lent buffer may be longer than the message.
        let mut longer = [0u8; 20];
        longer[..11].copy_from_slice(&tclunk);
        assert!(Message::decode(&longer).is_ok());
        // Size larger than the buffer, smaller than a header, beyond msize, or with slack.
        let mut b = tclunk;
        b[0] = 12;
        assert_eq!(Message::decode(&b), Err(Error::Short));
        b[0] = 6;
        assert_eq!(Message::decode(&b), Err(Error::BadSize));
        assert_eq!(Message::decode(&[1, 0, 1, 0, 120, 0, 0]), Err(Error::BadSize));
        assert_eq!(Message::decode(&[12, 0, 0, 0, 120, 0, 0, 7, 0, 0, 0, 0]), Err(Error::Trailing));
        // Terror and unknown types.
        assert_eq!(Message::decode(&[7, 0, 0, 0, 106, 0, 0]), Err(Error::BadType));
        assert_eq!(Message::decode(&[7, 0, 0, 0, 99, 0, 0]), Err(Error::BadType));
    }

    #[test]
    fn walk_limit() {
        let mut buf = [0u8; 64];
        buf[..17].copy_from_slice(&[17, 0, 0, 0, 110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 17, 0]);
        assert_eq!(Message::decode(&buf[..17]), Err(Error::TooManyElements));
        assert_eq!(Names::new(&[""; 17]).err(), Some(Error::TooManyElements));
        assert!(Names::new(&[""; 16]).is_ok());
    }

    #[test]
    fn encode_respects_msize_and_buffer() {
        let data = [0u8; MSIZE];
        let m = Message { tag: 0, body: Body::Rread { data: &data } };
        let mut out = [0u8; MSIZE + 64];
        assert_eq!(m.encode(&mut out), Err(Error::TooLarge));
        let m = Message { tag: 0, body: Body::Rread { data: &data[..MSIZE - 11] } };
        assert_eq!(m.encode(&mut out), Ok(MSIZE));
        assert_eq!(m.encode(&mut out[..100]), Err(Error::TooLarge));
        assert!(out[..100].iter().all(|&b| b == 0), "a failed encode leaves no partial message");
    }

    #[test]
    fn a_directory_entry_that_does_not_fit_is_not_written() {
        let stat = Stat {
            kind: 0,
            dev: 0,
            qid: Qid::default(),
            mode: 0,
            atime: 0,
            mtime: 0,
            length: 0,
            name: "hello",
            uid: "u",
            gid: "g",
            muid: "m",
        };
        let mut buf = [0u8; 77]; // one entry is 57 bytes: the second fails midway
        let mut w = Writer::new(&mut buf);
        stat.write_entry(&mut w).unwrap();
        assert_eq!(stat.write_entry(&mut w), Err(Error::TooLarge));
        assert_eq!(w.position(), 57);
        assert_eq!(stats(&buf[..57]).count(), 1);
        assert!(buf[57..].iter().all(|&b| b == 0));
    }

    #[test]
    fn directory_reads() {
        let stat = Stat {
            kind: 0,
            dev: 0,
            qid: Qid::default(),
            mode: 0,
            atime: 0,
            mtime: 0,
            length: 9,
            name: "f",
            uid: "",
            gid: "",
            muid: "",
        };
        let mut buf = [0u8; 256];
        let mut w = Writer::new(&mut buf);
        stat.write_entry(&mut w).unwrap();
        stat.write_entry(&mut w).unwrap();
        let n = w.position();
        let all: Result<alloc::vec::Vec<_>, _> = stats(&buf[..n]).collect();
        assert_eq!(all.unwrap(), [stat, stat]);
        // A truncated entry is one error, then the end.
        let mut it = stats(&buf[..n - 1]);
        assert!(it.next().unwrap().is_ok());
        assert!(it.next().unwrap().is_err());
        assert!(it.next().is_none());
    }
}
