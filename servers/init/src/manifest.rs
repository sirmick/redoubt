//! The boot manifest's structure, decoded from strict JSON (servers/init.md, "The boot manifest";
//! servers/wire.md, "Strict JSON"). Decoding checks only types and members: the wrong JSON type,
//! a missing member, an unknown member or a repeated one is an error. What the values must say
//! (names, references, sizes) is [`crate::check`]'s.
//!
//! The members, with their JSON types (a 64-bit quantity is a decimal string, a small count a
//! number):
//!
//! | Entry | Members |
//! | --- | --- |
//! | `confined` | a boolean, optional |
//! | `devices[]` | `name`; `base` (string), `irq` (number), either may be absent, not both; `dma` (boolean) |
//! | `labels[]` | `name`, `owner` (a principal), `id` (string) |
//! | `volumes[]` | `name`, `partition` (number), then optional `labels` (label names), `disk` (the `servers` entry of its `blkd`), `verity` (`server`, the `servers` entry of its `verityd`; pinned, `root` (64 lowercase hex digits) and `blocks` (string), or signed, `key` (64 lowercase hex digits or `bundle`) and `floor` (string)), `bytes` (string) |
//! | `servers[]` | `name`, `program` (a bundle entry), `budget`, then optional `stack_pages` (string), `heap_pages` (string), `labels`, `devices[]` (`device`, `as`), `volume`, `receives` (endpoint names), `handed[]` (`endpoint`, `badge` (string)), `args` |
//! | `public` | bundle entry names |
//! | `principals[]` | `name`, `account` (string), `budget`, then optional `ssh_keys`, `approval_keys`, `labels` (owned), `label_sets[]` (`labels`), `home` (`VOLUME:/PATH`), `home_quota` (string), `net[]` (`prefix`, `ports`) |
//! | `steward` | `server` (the `servers` entry of the steward), `sizes` (`session`, `agent`, `sub_agent`, `crossing`: each a `budget`; `cost` (string)), optional |
//! | `console` | a principal's name, optional |
//!
//! A `budget` is `{ "pages": string, "processes": number, "weight": number }`. Every list is
//! optional and empty when absent.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use redoubt_client::launch::STACK_PAGES;
use redoubt_rt::wire::json::{self, Members, SchemaError, SchemaKind, Value};

/// The decoded manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub confined: bool,
    pub devices: Vec<Device>,
    pub labels: Vec<Label>,
    pub volumes: Vec<Volume>,
    pub servers: Vec<Server>,
    pub public: Vec<String>,
    pub principals: Vec<Principal>,
    pub steward: Option<Steward>,
    /// The principal whose unlabelled session the steward opens on the UART.
    pub console: Option<String>,
}

/// The steward: its `servers` entry, which `init` hands `users`, and the sizes it carves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Steward {
    pub server: String,
    pub sizes: Sizes,
}

/// The sizes the steward carves for each session, agent, sub-agent and crossing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sizes {
    pub session: Budget,
    pub agent: Budget,
    pub sub_agent: Budget,
    pub crossing: Budget,
    /// The pages a budget object itself costs.
    pub cost: u64,
}

/// One device: its register region by base, its interrupt by number, or both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub base: Option<u64>,
    pub irq: Option<i64>,
    pub dma: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub owner: String,
    pub id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    pub partition: i64,
    pub labels: Vec<String>,
    /// The name of the `servers` entry of the `blkd` serving the volume's disk; required when
    /// the manifest has more than one `blkd`.
    pub disk: Option<String>,
    /// For a verified volume (servers/verityd.md), its verifier and what it checks against.
    pub verity: Option<Verity>,
    /// The volume's size in bytes: its partition's, which the disk's packer is held to. Required
    /// on a volume homes' quotas are carved from, which may not sum past it.
    pub bytes: Option<u64>,
}

/// A verified volume's `verity` key: the `servers` entry of the `verityd` that checks it, and
/// what it checks against, pinned (the root and data blocks) or signed (the key the volume's root
/// block is signed under and the lowest version it may carry). Decoding takes any of the four;
/// [`crate::check`] holds the entry to one mode, both of its members.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verity {
    pub server: String,
    /// As the manifest gives it; [`crate::check`] holds it to 64 lowercase hex digits.
    pub root: Option<String>,
    pub blocks: Option<u64>,
    /// As the manifest gives it; [`crate::check`] holds it to 64 lowercase hex digits or
    /// `bundle`.
    pub key: Option<String>,
    pub floor: Option<u64>,
}

/// The limits of a budget `init` creates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub pages: u64,
    pub processes: i64,
    pub weight: i64,
}

/// A device a server gets: the `devices` entry, and the name the program looks it up by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceUse {
    pub device: String,
    pub name: String,
}

/// An endpoint a server is handed, and the root badge `init` mints for it there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handed {
    pub endpoint: String,
    pub badge: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub name: String,
    pub program: String,
    pub budget: Budget,
    /// First-thread stack, in pages; absent in an older manifest means the launcher default.
    pub stack_pages: u64,
    /// The heap's cap, in pages, at most `u32::MAX` (the startup block's field); absent means
    /// none, the budget alone.
    pub heap_pages: Option<u32>,
    pub labels: Vec<String>,
    pub devices: Vec<DeviceUse>,
    pub volume: Option<String>,
    pub receives: Vec<String>,
    pub handed: Vec<Handed>,
    pub args: Vec<String>,
}

/// A label set a principal works under: its fixed sub-budget is an equal share of the
/// principal's budget (servers/steward.md, "Fixed sub-budgets per label set").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelSet {
    pub labels: Vec<String>,
}

/// A network scope: an IP prefix and the ports in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Net {
    pub prefix: String,
    pub ports: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub name: String,
    pub account: u64,
    pub budget: Budget,
    pub ssh_keys: Vec<String>,
    pub approval_keys: Vec<String>,
    pub labels: Vec<String>,
    pub label_sets: Vec<LabelSet>,
    pub home: Option<String>,
    /// The bytes the principal may hold in its home, whatever its sessions: required with
    /// `home`.
    pub home_quota: Option<u64>,
    pub net: Vec<Net>,
}

/// Why the file is not a manifest: not strict JSON, or not this structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Json(json::Error),
    Schema(SchemaError),
}

/// Parses and decodes `input`.
pub fn decode(input: &[u8]) -> Result<Manifest, DecodeError> {
    let value = json::parse(input).map_err(DecodeError::Json)?;
    manifest(&value).map_err(DecodeError::Schema)
}

fn manifest(v: &Value) -> Result<Manifest, SchemaError> {
    v.object(|m| {
        Ok(Manifest {
            confined: m.optional("confined", Value::bool)?.unwrap_or(false),
            devices: list(m, "devices", device)?,
            labels: list(m, "labels", label)?,
            volumes: list(m, "volumes", volume)?,
            servers: list(m, "servers", server)?,
            public: list(m, "public", string)?,
            principals: list(m, "principals", principal)?,
            steward: m.optional("steward", steward)?,
            console: m.optional("console", string)?,
        })
    })
}

/// An optional array member, empty when absent.
fn list<'v, 'a: 'v, T>(
    m: &mut Members<'v, 'a>,
    name: &str,
    decode: impl FnMut(&Value<'a>) -> Result<T, SchemaError>,
) -> Result<Vec<T>, SchemaError> {
    Ok(m.optional(name, |v| v.items(decode))?.unwrap_or_default())
}

fn string(v: &Value) -> Result<String, SchemaError> { v.str().map(ToString::to_string) }

fn device(v: &Value) -> Result<Device, SchemaError> {
    v.object(|m| {
        let name = m.required("name", string)?;
        let base = m.optional("base", Value::u64_string)?;
        let irq = m.optional("irq", Value::int)?;
        let dma = m.required("dma", Value::bool)?;
        // A device is named by its register region, its interrupt, or both: never neither.
        if base.is_none() && irq.is_none() {
            return Err(SchemaError { path: "base".to_string(), kind: SchemaKind::Missing });
        }
        Ok(Device { name, base, irq, dma })
    })
}

fn label(v: &Value) -> Result<Label, SchemaError> {
    v.object(|m| {
        Ok(Label {
            name: m.required("name", string)?,
            owner: m.required("owner", string)?,
            id: m.required("id", Value::u64_string)?,
        })
    })
}

fn volume(v: &Value) -> Result<Volume, SchemaError> {
    v.object(|m| {
        Ok(Volume {
            name: m.required("name", string)?,
            partition: m.required("partition", Value::int)?,
            labels: list(m, "labels", string)?,
            disk: m.optional("disk", string)?,
            verity: m.optional("verity", verity)?,
            bytes: m.optional("bytes", Value::u64_string)?,
        })
    })
}

fn verity(v: &Value) -> Result<Verity, SchemaError> {
    v.object(|m| {
        Ok(Verity {
            server: m.required("server", string)?,
            root: m.optional("root", string)?,
            blocks: m.optional("blocks", Value::u64_string)?,
            key: m.optional("key", string)?,
            floor: m.optional("floor", Value::u64_string)?,
        })
    })
}

fn budget(v: &Value) -> Result<Budget, SchemaError> {
    v.object(|m| {
        Ok(Budget {
            pages: m.required("pages", Value::u64_string)?,
            processes: m.required("processes", Value::int)?,
            weight: m.required("weight", Value::int)?,
        })
    })
}

fn device_use(v: &Value) -> Result<DeviceUse, SchemaError> {
    v.object(|m| Ok(DeviceUse { device: m.required("device", string)?, name: m.required("as", string)? }))
}

fn handed(v: &Value) -> Result<Handed, SchemaError> {
    v.object(|m| {
        Ok(Handed {
            endpoint: m.required("endpoint", string)?,
            badge: m.required("badge", Value::u64_string)?,
        })
    })
}

fn server(v: &Value) -> Result<Server, SchemaError> {
    v.object(|m| {
        Ok(Server {
            name: m.required("name", string)?,
            program: m.required("program", string)?,
            budget: m.required("budget", budget)?,
            stack_pages: m.optional("stack_pages", Value::u64_string)?.unwrap_or(STACK_PAGES as u64),
            heap_pages: m.optional("heap_pages", |v| {
                u32::try_from(v.u64_string()?)
                    .map_err(|_| SchemaError { path: String::new(), kind: SchemaKind::WrongType })
            })?,
            labels: list(m, "labels", string)?,
            devices: list(m, "devices", device_use)?,
            volume: m.optional("volume", string)?,
            receives: list(m, "receives", string)?,
            handed: list(m, "handed", handed)?,
            args: list(m, "args", string)?,
        })
    })
}

fn steward(v: &Value) -> Result<Steward, SchemaError> {
    v.object(|m| Ok(Steward { server: m.required("server", string)?, sizes: m.required("sizes", sizes)? }))
}

fn sizes(v: &Value) -> Result<Sizes, SchemaError> {
    v.object(|m| {
        Ok(Sizes {
            session: m.required("session", budget)?,
            agent: m.required("agent", budget)?,
            sub_agent: m.required("sub_agent", budget)?,
            crossing: m.required("crossing", budget)?,
            cost: m.required("cost", Value::u64_string)?,
        })
    })
}

fn label_set(v: &Value) -> Result<LabelSet, SchemaError> {
    v.object(|m| Ok(LabelSet { labels: list(m, "labels", string)? }))
}

fn net(v: &Value) -> Result<Net, SchemaError> {
    v.object(|m| Ok(Net { prefix: m.required("prefix", string)?, ports: list(m, "ports", Value::int)? }))
}

fn principal(v: &Value) -> Result<Principal, SchemaError> {
    v.object(|m| {
        Ok(Principal {
            name: m.required("name", string)?,
            account: m.required("account", Value::u64_string)?,
            budget: m.required("budget", budget)?,
            ssh_keys: list(m, "ssh_keys", string)?,
            approval_keys: list(m, "approval_keys", string)?,
            labels: list(m, "labels", string)?,
            label_sets: list(m, "label_sets", label_set)?,
            home: m.optional("home", string)?,
            home_quota: m.optional("home_quota", Value::u64_string)?,
            net: list(m, "net", net)?,
        })
    })
}
