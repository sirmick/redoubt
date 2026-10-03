//! Why `init` refuses a boot. Each refusal is one line, printed before anything else runs
//! (servers/init.md, "Starting the servers", step 1), and the bench's refusal cases match it: the
//! line names where in the manifest the fault is, and the rule it breaks.

use alloc::string::String;
use core::fmt;

use redoubt_rt::wire::json::{self, SchemaError, SchemaKind};

/// A boot `init` will not run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The manifest is longer than the arena can parse ([`crate::ARENA_PAGES`]).
    Arena { len: usize },
    /// The bundle holds no `manifest` entry.
    NoManifest,
    /// Not strict JSON (servers/wire.md, "Strict JSON").
    Json(json::Error),
    /// Strict JSON, but not the manifest's structure.
    Schema(SchemaError),
    /// A value the manifest's rules refuse, and where it is.
    At { at: String, why: Why },
    /// The servers ask `system` for more than it has free.
    SystemFit { what: &'static str, need: u64, free: u64 },
    /// A shared server's `buckets=N` is below the domains the manifest declares there.
    Buckets { at: String, have: u32, need: u32 },
    /// With `confined` set, two label sets share something (R34).
    Confined { at: String, sharing: Sharing },
    /// What the manifest will cost `init` in `root` is more than `root` keeps for it.
    Bound { need: u64, free: u64 },
}

/// What is wrong with one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Outside the manifest's name rule (servers/init.md, Names).
    NotAName,
    /// A name given twice where names must differ.
    Twice,
    /// A reference to nothing the manifest or the bundle holds.
    Unknown,
    /// A device name over 60 bytes or ending in `-irq`.
    DeviceName,
    /// A base or an interrupt no device handle has.
    Unmatched,
    /// A base or an interrupt two `devices` entries name.
    SplitDevice,
    /// The entry's DMA flag and the kernel's object disagree.
    DmaMismatch,
    /// A `devices` entry two servers hold.
    HeldTwice,
    /// An endpoint two servers receive on.
    ReceivedTwice,
    /// A budget handled to a server (R33).
    BudgetHandle,
    /// A badge that is 0, not a root badge, or given twice at one endpoint.
    Badge,
    /// Limits no process could run in, or beyond what the kernel takes.
    Budget,
    /// More labels than a budget holds.
    TooManyLabels,
    /// An argument with a NUL, or one a `bootfsd` entry may not carry.
    Argument,
    /// A `buckets=` argument the serving library would refuse.
    BucketsArgument,
    /// The handles and arguments do not fit a startup block.
    Block,
    /// Not one Ed25519 key in OpenSSH's form.
    Key,
    /// A value outside its member's form (an account of 0, a home, a prefix, a port).
    Value,
    /// `public` names the manifest, which is never public.
    PublicManifest,
    /// Entries for `bootfsd` to serve, but no `bootfsd` to serve them.
    NoBootfsd,
}

/// What two label sets would share under `confined` (servers/init.md, "The confinement check").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sharing {
    Endpoint,
    Volume,
    Network,
    Device,
    Server,
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Why::NotAName => "not a name",
            Why::Twice => "named twice",
            Why::Unknown => "names nothing the manifest or the bundle holds",
            Why::DeviceName => "a device name is at most 60 bytes and does not end in -irq",
            Why::Unmatched => "no device handle has it",
            Why::SplitDevice => "two devices entries name one device",
            Why::DmaMismatch => "the DMA flag is not the kernel's",
            Why::HeldTwice => "two servers hold one device",
            Why::ReceivedTwice => "two servers receive on one endpoint",
            Why::BudgetHandle => "a server may not hold a budget handle (R33)",
            Why::Badge => "not a root badge given once at its endpoint",
            Why::Budget => "no process could run in this budget",
            Why::TooManyLabels => "more labels than a budget holds",
            Why::Argument => "an argument with a NUL, or on bootfsd one that is not buckets=N",
            Why::BucketsArgument => "not one buckets=N of 1 to 32",
            Why::Block => "the handles and arguments do not fit a startup block",
            Why::Key => "not one ssh-ed25519 key",
            Why::Value => "not a value this member takes",
            Why::PublicManifest => "the manifest is never public",
            Why::NoBootfsd => "public entries but no bootfsd",
        })
    }
}

impl fmt::Display for Sharing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Sharing::Endpoint => "an endpoint",
            Sharing::Volume => "a volume",
            Sharing::Network => "a network instance",
            Sharing::Device => "a device",
            Sharing::Server => "a server instance",
        })
    }
}

impl fmt::Display for Refusal {
    /// The reason, after `init`'s prefix: `init: refused the boot: <this>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Arena { len } => write!(f, "the manifest's {len} bytes do not fit init's arena"),
            Refusal::NoManifest => f.write_str("the bundle holds no manifest"),
            Refusal::Json(e) => {
                write!(f, "the manifest is not strict JSON: {:?} at byte {}", e.kind, e.offset)
            }
            Refusal::Schema(e) => {
                let what = match e.kind {
                    SchemaKind::Missing => "missing",
                    SchemaKind::Unknown => "not a member here",
                    SchemaKind::WrongType => "the wrong type",
                };
                write!(f, "{}: {what}", e.path)
            }
            Refusal::At { at, why } => write!(f, "{at}: {why}"),
            Refusal::SystemFit { what, need, free } => {
                write!(f, "the servers need {need} {what} and system has {free} free")
            }
            Refusal::Buckets { at, have, need } => {
                write!(f, "{at}: buckets={have} is below the {need} domains declared there")
            }
            Refusal::Confined { at, sharing } => {
                write!(f, "{at}: two label sets share {sharing} in a confined boot (R34)")
            }
            Refusal::Bound { need, free } => {
                write!(f, "the boot would cost init {need} pages of root and root keeps {free} (INIT_PAGES)")
            }
        }
    }
}
