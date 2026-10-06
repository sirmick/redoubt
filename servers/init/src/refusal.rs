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
    /// More servers than `init` has threads to watch, one each beside its own (`MAX_THREADS`).
    Watchers { servers: usize, most: usize },
    /// `keyd` holds a key the box is authenticated by (R35): `at` is where the manifest lists it,
    /// or `bundle key`.
    KeyHeld { at: String },
    /// A step of the boot failed after the checks passed: a bug in the bound or the checks, and
    /// no boot runs half started. `at` names the server, `step` what failed.
    Failed { at: String, step: &'static str },
}

impl Refusal {
    /// A step of the boot that failed after the checks passed ([`Refusal::Failed`]).
    pub fn failed(at: &str, step: &'static str) -> Refusal { Refusal::Failed { at: String::from(at), step } }
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
    /// A first-thread stack is empty, over the cap, or does not fit its own budget.
    Stack,
    /// More labels than a budget holds.
    TooManyLabels,
    /// An argument with a NUL, one a `bootfsd` entry may not carry, or one `init` passes itself
    /// (`labels=` to a volume's server, `labels.` to `blkd`).
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
    /// No `keyd` to ask about the keys the box is authenticated by (R35).
    NoKeyd,
    /// An endpoint a `consoled` receives on, handed to a server: only `init` holds a root badge
    /// there, so only `init`'s lines are bare.
    ConsoledRoot,
    /// A volume a server attaches, but no `blkd` of its disk, receiving on an endpoint, to mint
    /// its range at.
    NoBlkd,
    /// A volume that does not name its disk, in a manifest with more than one `blkd`.
    NoDisk,
    /// An endpoint a `blkd` receives on, handed to a server: each badge there is a volume's
    /// range, which only `init` mints, for the one server attaching it (R47).
    BlkdHanded,
    /// A `blkd` whose `endpoint=` argument, exactly one, does not name the endpoint it receives
    /// on first, where `init` mints its volumes' ranges.
    BlkdEndpoint,
    /// A second entry for a program `init` calls itself (`keyd`, `consoled`, `bootfsd`): `init`
    /// starts and calls one of each.
    Second(&'static str),
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
            Why::Second(program) => return write!(f, "a second {program}, and init calls only one"),
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
            Why::Stack => "a stack is 1 to 128 pages and smaller than its budget",
            Why::TooManyLabels => "more labels than a budget holds",
            Why::Argument => {
                "an argument with a NUL, on bootfsd one that is not buckets=N, or one init passes itself"
            }
            Why::BucketsArgument => "not one buckets=N of 1 to 32",
            Why::Block => "the handles and arguments do not fit a startup block",
            Why::Key => "not one ssh-ed25519 key",
            Why::Value => "not a value this member takes",
            Why::PublicManifest => "the manifest is never public",
            Why::NoBootfsd => "public entries but no bootfsd",
            Why::NoBlkd => "a volume's range needs its disk's blkd, receiving on an endpoint",
            Why::NoDisk => "with more than one blkd, a volume names its disk",
            Why::BlkdHanded => "only init mints a badge at blkd, a volume's range (R47)",
            Why::BlkdEndpoint => "a blkd's one endpoint= names the endpoint it receives on first",
            Why::NoKeyd => "no keyd to ask about the keys the box is authenticated by (R35)",
            Why::ConsoledRoot => "only init holds a root badge at consoled",
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
            Refusal::Watchers { servers, most } => {
                write!(f, "{servers} servers, and init has threads to watch {most} (MAX_THREADS)")
            }
            Refusal::KeyHeld { at } => {
                write!(f, "{at}: keyd holds this key, and the box is authenticated by it (R35)")
            }
            Refusal::Failed { at, step } => write!(f, "{at}: could not {step}"),
        }
    }
}
