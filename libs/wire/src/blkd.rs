//! `blkd`'s protocol limits, beside its generated messages ([`crate::proto::blkd`]; the table is
//! `libs/wire/tables/blkd.md`, on servers/blkd.md): named once, where `blkd` and every client of
//! a range take them from. The tables carry no constants, so these two are written here.

/// The sector, in bytes: virtio-blk's unit, which the protocol's `sector` and `count` are in.
pub const SECTOR: u32 = 512;

/// The most sectors one `read` or `write` carries (servers/blkd.md, "Messages"): 64 sectors is
/// 32 KiB, inside one `MAX_LEND_PAGES` lend with its encoding, and a whole number of blocks at
/// every block size the volume servers use. A request over it is `too_many`.
pub const MAX_SECTORS: u32 = 64;
