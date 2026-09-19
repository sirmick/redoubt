//! Plain 9P2000 (no `.u`, no `.L`), as in Plan 9's intro(5), with `msize` fixed at 64 KiB
//! (WIRE.md). A message travels in a lent buffer: `size[4] type[1] tag[2]` and the body.
//!
//! [`Message::decode`] returns borrowed views into the buffer; [`Message::encode`] writes
//! into one. Decoding is strict: the size field must frame the message exactly, every
//! string must be UTF-8, a walk has at most [`MAXWELEM`] elements, and a stat's own size
//! fields must agree with its contents. So `encode(decode(b)) == b` for every accepted `b`
//! (the fuzz target checks it).
//!
//! What the codec does not judge: whether a walk name is `..` or contains `/`, whether a
//! fid is in use, whether `version` is "9P2000". Those are protocol state and belong to the
//! server (NAMESPACES.md: servers clean paths themselves).

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

/// Message types (intro(5)). `Terror` (106) is not a message and is refused.
pub mod kind {
    pub const TVERSION: u8 = 100;
    pub const RVERSION: u8 = 101;
    pub const TAUTH: u8 = 102;
    pub const RAUTH: u8 = 103;
    pub const TATTACH: u8 = 104;
    pub const RATTACH: u8 = 105;
    pub const RERROR: u8 = 107;
    pub const TFLUSH: u8 = 108;
    pub const RFLUSH: u8 = 109;
    pub const TWALK: u8 = 110;
    pub const RWALK: u8 = 111;
    pub const TOPEN: u8 = 112;
    pub const ROPEN: u8 = 113;
    pub const TCREATE: u8 = 114;
    pub const RCREATE: u8 = 115;
    pub const TREAD: u8 = 116;
    pub const RREAD: u8 = 117;
    pub const TWRITE: u8 = 118;
    pub const RWRITE: u8 = 119;
    pub const TCLUNK: u8 = 120;
    pub const RCLUNK: u8 = 121;
    pub const TREMOVE: u8 = 122;
    pub const RREMOVE: u8 = 123;
    pub const TSTAT: u8 = 124;
    pub const RSTAT: u8 = 125;
    pub const TWSTAT: u8 = 126;
    pub const RWSTAT: u8 = 127;
}

/// A file's server-unique identity: `type[1] vers[4] path[8]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Qid {
    /// 9P's `type` (QTDIR, QTAPPEND, ...).
    pub kind: u8,
    pub version: u32,
    pub path: u64,
}

impl Qid {
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
    /// Reads one stat, whose leading size must cover exactly its fields.
    pub fn read(r: &mut Reader<'a>) -> Result<Self, Error> {
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

    /// Writes one stat with its leading size (so a directory read is these, concatenated).
    pub fn write(&self, w: &mut Writer<'_>) -> Result<(), Error> {
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
    }
}

/// Patches the `u16` at `at` with the number of bytes written after it.
fn patch_len16(w: &mut Writer<'_>, at: usize) -> Result<(), Error> {
    let start = at.checked_add(2).ok_or(Error::TooLarge)?;
    let len = w.position().checked_sub(start).ok_or(Error::TooLarge)?;
    w.patch_u16(at, u16::try_from(len).map_err(|_| Error::TooLarge)?)
}

/// `stat[n]` in `Rstat` and `Twstat`: a `u16` count, then one stat of exactly that many bytes.
fn read_stat_field<'a>(r: &mut Reader<'a>) -> Result<Stat<'a>, Error> {
    let n = usize::from(r.u16()?);
    let mut s = Reader::new(r.take(n)?);
    let stat = Stat::read(&mut s)?;
    s.finish()?;
    Ok(stat)
}

fn write_stat_field(w: &mut Writer<'_>, stat: &Stat<'_>) -> Result<(), Error> {
    let at = w.position();
    w.u16(0)?;
    stat.write(w)?;
    patch_len16(w, at)
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
        let stat = Stat::read(&mut self.r);
        self.failed = stat.is_err();
        Some(stat)
    }
}

/// The names of a `Twalk`: at most [`MAXWELEM`].
#[derive(Debug, Clone, Copy)]
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

impl PartialEq for Names<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for Names<'_> {}

/// The qids of an `Rwalk`: at most [`MAXWELEM`].
#[derive(Debug, Clone, Copy)]
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

impl PartialEq for Qids {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for Qids {}

/// One 9P2000 message body; field names follow intro(5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body<'a> {
    Tversion { msize: u32, version: &'a str },
    Rversion { msize: u32, version: &'a str },
    Tauth { afid: u32, uname: &'a str, aname: &'a str },
    Rauth { aqid: Qid },
    Tattach { fid: u32, afid: u32, uname: &'a str, aname: &'a str },
    Rattach { qid: Qid },
    Rerror { ename: &'a str },
    Tflush { oldtag: u16 },
    Rflush,
    Twalk { fid: u32, newfid: u32, wnames: Names<'a> },
    Rwalk { qids: Qids },
    Topen { fid: u32, mode: u8 },
    Ropen { qid: Qid, iounit: u32 },
    Tcreate { fid: u32, name: &'a str, perm: u32, mode: u8 },
    Rcreate { qid: Qid, iounit: u32 },
    Tread { fid: u32, offset: u64, count: u32 },
    Rread { data: &'a [u8] },
    Twrite { fid: u32, offset: u64, data: &'a [u8] },
    Rwrite { count: u32 },
    Tclunk { fid: u32 },
    Rclunk,
    Tremove { fid: u32 },
    Rremove,
    Tstat { fid: u32 },
    Rstat { stat: Stat<'a> },
    Twstat { fid: u32, stat: Stat<'a> },
    Rwstat,
}

impl Body<'_> {
    /// The message's type byte.
    pub fn kind(&self) -> u8 {
        use kind::*;
        match self {
            Body::Tversion { .. } => TVERSION,
            Body::Rversion { .. } => RVERSION,
            Body::Tauth { .. } => TAUTH,
            Body::Rauth { .. } => RAUTH,
            Body::Tattach { .. } => TATTACH,
            Body::Rattach { .. } => RATTACH,
            Body::Rerror { .. } => RERROR,
            Body::Tflush { .. } => TFLUSH,
            Body::Rflush => RFLUSH,
            Body::Twalk { .. } => TWALK,
            Body::Rwalk { .. } => RWALK,
            Body::Topen { .. } => TOPEN,
            Body::Ropen { .. } => ROPEN,
            Body::Tcreate { .. } => TCREATE,
            Body::Rcreate { .. } => RCREATE,
            Body::Tread { .. } => TREAD,
            Body::Rread { .. } => RREAD,
            Body::Twrite { .. } => TWRITE,
            Body::Rwrite { .. } => RWRITE,
            Body::Tclunk { .. } => TCLUNK,
            Body::Rclunk => RCLUNK,
            Body::Tremove { .. } => TREMOVE,
            Body::Rremove => RREMOVE,
            Body::Tstat { .. } => TSTAT,
            Body::Rstat { .. } => RSTAT,
            Body::Twstat { .. } => TWSTAT,
            Body::Rwstat => RWSTAT,
        }
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

impl<'a> Message<'a> {
    /// Decodes the message at the front of `buf` (see [`message_size`]).
    pub fn decode(buf: &'a [u8]) -> Result<Self, Error> {
        use kind::*;
        let size = message_size(buf)?;
        let mut r = Reader::new(buf.get(4..size).ok_or(Error::Short)?);
        let kind = r.u8()?;
        let tag = r.u16()?;
        let body = match kind {
            TVERSION => Body::Tversion { msize: r.u32()?, version: r.string()? },
            RVERSION => Body::Rversion { msize: r.u32()?, version: r.string()? },
            TAUTH => Body::Tauth { afid: r.u32()?, uname: r.string()?, aname: r.string()? },
            RAUTH => Body::Rauth { aqid: Qid::read(&mut r)? },
            TATTACH => Body::Tattach {
                fid: r.u32()?,
                afid: r.u32()?,
                uname: r.string()?,
                aname: r.string()?,
            },
            RATTACH => Body::Rattach { qid: Qid::read(&mut r)? },
            RERROR => Body::Rerror { ename: r.string()? },
            TFLUSH => Body::Tflush { oldtag: r.u16()? },
            RFLUSH => Body::Rflush,
            TWALK => {
                let fid = r.u32()?;
                let newfid = r.u32()?;
                let n = usize::from(r.u16()?);
                if n > MAXWELEM {
                    return Err(Error::TooManyElements);
                }
                let mut names = [""; MAXWELEM];
                for name in names.iter_mut().take(n) {
                    *name = r.string()?;
                }
                Body::Twalk { fid, newfid, wnames: Names { len: n, items: names } }
            }
            RWALK => {
                let n = usize::from(r.u16()?);
                if n > MAXWELEM {
                    return Err(Error::TooManyElements);
                }
                let mut qids = [Qid::default(); MAXWELEM];
                for qid in qids.iter_mut().take(n) {
                    *qid = Qid::read(&mut r)?;
                }
                Body::Rwalk { qids: Qids { len: n, items: qids } }
            }
            TOPEN => Body::Topen { fid: r.u32()?, mode: r.u8()? },
            ROPEN => Body::Ropen { qid: Qid::read(&mut r)?, iounit: r.u32()? },
            TCREATE => Body::Tcreate { fid: r.u32()?, name: r.string()?, perm: r.u32()?, mode: r.u8()? },
            RCREATE => Body::Rcreate { qid: Qid::read(&mut r)?, iounit: r.u32()? },
            TREAD => Body::Tread { fid: r.u32()?, offset: r.u64()?, count: r.u32()? },
            RREAD => Body::Rread { data: r.bytes()? },
            TWRITE => Body::Twrite { fid: r.u32()?, offset: r.u64()?, data: r.bytes()? },
            RWRITE => Body::Rwrite { count: r.u32()? },
            TCLUNK => Body::Tclunk { fid: r.u32()? },
            RCLUNK => Body::Rclunk,
            TREMOVE => Body::Tremove { fid: r.u32()? },
            RREMOVE => Body::Rremove,
            TSTAT => Body::Tstat { fid: r.u32()? },
            RSTAT => Body::Rstat { stat: read_stat_field(&mut r)? },
            TWSTAT => Body::Twstat { fid: r.u32()?, stat: read_stat_field(&mut r)? },
            RWSTAT => Body::Rwstat,
            _ => return Err(Error::BadType),
        };
        r.finish()?;
        Ok(Message { tag, body })
    }

    /// Encodes into the front of `out` and returns the message's size. Fails with
    /// `TooLarge` if `out` is too small or the message would exceed [`MSIZE`].
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.u32(0)?; // size, patched below
        w.u8(self.body.kind())?;
        w.u16(self.tag)?;
        match &self.body {
            Body::Tversion { msize, version } | Body::Rversion { msize, version } => {
                w.u32(*msize)?;
                w.string(version)?;
            }
            Body::Tauth { afid, uname, aname } => {
                w.u32(*afid)?;
                w.string(uname)?;
                w.string(aname)?;
            }
            Body::Rauth { aqid: qid } | Body::Rattach { qid } => qid.write(&mut w)?,
            Body::Tattach { fid, afid, uname, aname } => {
                w.u32(*fid)?;
                w.u32(*afid)?;
                w.string(uname)?;
                w.string(aname)?;
            }
            Body::Rerror { ename } => w.string(ename)?,
            Body::Tflush { oldtag } => w.u16(*oldtag)?,
            Body::Rflush | Body::Rclunk | Body::Rremove | Body::Rwstat => {}
            Body::Twalk { fid, newfid, wnames } => {
                w.u32(*fid)?;
                w.u32(*newfid)?;
                let names = wnames.as_slice();
                w.u16(u16::try_from(names.len()).map_err(|_| Error::TooManyElements)?)?;
                for name in names {
                    w.string(name)?;
                }
            }
            Body::Rwalk { qids } => {
                let qids = qids.as_slice();
                w.u16(u16::try_from(qids.len()).map_err(|_| Error::TooManyElements)?)?;
                for qid in qids {
                    qid.write(&mut w)?;
                }
            }
            Body::Topen { fid, mode } => {
                w.u32(*fid)?;
                w.u8(*mode)?;
            }
            Body::Ropen { qid, iounit } | Body::Rcreate { qid, iounit } => {
                qid.write(&mut w)?;
                w.u32(*iounit)?;
            }
            Body::Tcreate { fid, name, perm, mode } => {
                w.u32(*fid)?;
                w.string(name)?;
                w.u32(*perm)?;
                w.u8(*mode)?;
            }
            Body::Tread { fid, offset, count } => {
                w.u32(*fid)?;
                w.u64(*offset)?;
                w.u32(*count)?;
            }
            Body::Rread { data } => w.bytes(data)?,
            Body::Twrite { fid, offset, data } => {
                w.u32(*fid)?;
                w.u64(*offset)?;
                w.bytes(data)?;
            }
            Body::Rwrite { count } => w.u32(*count)?,
            Body::Tclunk { fid } | Body::Tremove { fid } | Body::Tstat { fid } => w.u32(*fid)?,
            Body::Rstat { stat } => write_stat_field(&mut w, stat)?,
            Body::Twstat { fid, stat } => {
                w.u32(*fid)?;
                write_stat_field(&mut w, stat)?;
            }
        }
        let size = w.position();
        if size > MSIZE {
            return Err(Error::TooLarge);
        }
        w.patch_u32(0, u32::try_from(size).map_err(|_| Error::TooLarge)?)?;
        Ok(size)
    }
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
        stat.write(&mut w).unwrap();
        stat.write(&mut w).unwrap();
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
