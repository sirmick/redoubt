//! The approval screen (servers/steward.md, "The powerbox and approvals", R38): rendered by the
//! steward from the structured request, printable ASCII only, requester text capped and marked,
//! and a labelled requester's free text never shown.

use alloc::format;
use alloc::string::String;

use crate::consts::{DECLASSIFY_MAX, FIELD_CAP};
use crate::domain::Domain;
use crate::effect::{Kind, Rendered};
use crate::event::Content;
use crate::manifest::Fixed;
use crate::store::{DomainState, Request};

/// Printable ASCII only (a whitelist, so control, bidi and format characters go too, each shown
/// as `?`), cut to `cap` characters, with quotes and backslashes escaped so a field cannot end
/// its own quoting.
pub fn sanitize(s: &str, cap: usize) -> String {
    let mut out = String::new();
    for c in s.chars().take(cap) {
        let c = if (' '..='~').contains(&c) { c } else { '?' };
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Declassifiable text: printable ASCII and newlines.
pub fn printable(b: &[u8]) -> bool { b.iter().all(|c| (0x20..0x7f).contains(c) || *c == b'\n') }

/// A lease in human units: "2 h 5 min", "90 s", "250 ms".
pub fn human(us: u64) -> String {
    let (h, m, s, ms) = (us / 3_600_000_000, us / 60_000_000 % 60, us / 1_000_000 % 60, us / 1000 % 1000);
    match (h, m, s) {
        (0, 0, 0) => format!("{ms} ms"),
        (0, 0, s) => format!("{s} s"),
        (0, m, _) => format!("{m} min"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

/// How a screen treats requester text: the shipped rules, or the model's broken ones.
#[derive(Clone, Copy)]
pub struct Rules {
    /// Renders one requester field, capped.
    pub field: fn(&str, usize) -> String,
    /// Whether a labelled requester's free text is withheld.
    pub withhold_labelled: bool,
}

impl Rules {
    pub const SHIPPED: Rules = Rules { field: sanitize, withhold_labelled: true };
}

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

/// The screen for request `r` of `domain`: who asks (kind and steward-assigned name, beside its
/// principal), what, for how long, and the label consequences.
pub fn screen(fixed: &Fixed, domain: &Domain, state: &DomainState, r: &Request, rules: &Rules) -> Rendered {
    let labelled = !domain.labels().is_empty() && rules.withhold_labelled;
    let field = rules.field;
    let who = field(&fixed.principals[r.principal].name, FIELD_CAP);
    let (kind, number) = match r.by.kind {
        Kind::Lease => ("agent", state.leases.get(&r.by.id).map_or(0, |l| l.number)),
        _ => ("session", state.sessions.get(&r.by.id).map_or(0, |s| s.number)),
    };
    let what = match &r.content {
        Content::Agent { labels, lease } => {
            format!("start an agent labelled {labels:?} for {}", human(*lease))
        }
        Content::Declassify { labels, item } => {
            let shown = r.snapshot.as_ref().map(|b| field(&String::from_utf8_lossy(b), DECLASSIFY_MAX));
            format!("declassify item {item} of labels {labels:?}: \"{}\"", shown.unwrap_or_default())
        }
        Content::Push { source, target, item } => {
            let size = r.snapshot.as_ref().map_or(0, |b| b.len());
            let digest = crate::hash::item(r.snapshot.as_deref().unwrap_or_default());
            format!(
                "push unlabelled item {source} ({size} bytes, sha256 {}) to item {item} of labels {target:?}",
                hex(&digest)
            )
        }
        // A labelled requester's free text is not shown: it could carry the vault out.
        Content::Note { .. } if labelled => String::from("a note (text withheld: labelled requester)"),
        Content::Note { what } => format!("a note (untrusted): \"{}\"", field(what, FIELD_CAP)),
    };
    let reason = if labelled {
        String::from("reason withheld (labelled requester)")
    } else {
        format!("reason (untrusted): \"{}\"", field(&r.reason, FIELD_CAP))
    };
    let text = format!(
        "request from {who} ({kind}-{number}, labels {:?}): {what}; {reason}",
        domain.labels().as_slice()
    );
    Rendered { id: r.id, hash: r.hash, labels: domain.labels().clone(), text }
}
