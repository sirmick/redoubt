//! What the boot manifest fixes and nothing changes (servers/steward.md, "Principals" and
//! "Fixed sub-budgets per label set"): the principals, their keys, owned labels and label sets,
//! and the keys `keyd` holds; and the manifest lines that carry it (servers/steward.md, "The
//! manifest lines"), which `init` hands the steward as arguments and a trace begins with.

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::num::NonZeroU64;

use crate::domain::{Domain, Labels};

/// A budget's limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub pages: u64,
    pub processes: u64,
    pub weight: u64,
}

/// One principal as the manifest gives it. Keys are opaque ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrincipalSpec {
    pub name: String,
    pub account: u64,
    pub login_keys: Vec<u64>,
    pub approval_keys: Vec<u64>,
    /// The labels it owns.
    pub owned: Vec<u64>,
    /// The label sets it works under: one domain each. A set need not be owned (a project's
    /// label, say), so ownership is checked on its own (`owns_labels`).
    pub label_sets: Vec<Vec<u64>>,
    /// Its top budget, which boot splits into one fixed sub-budget per label set.
    pub top: Limits,
}

/// The sizes the steward carves, set by the system bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sizes {
    pub session: Limits,
    pub agent: Limits,
    /// A sub-agent, carved inside its agent's budget.
    pub sub_agent: Limits,
    /// A reader or writer budget.
    pub crossing: Limits,
    /// The pages a budget object itself costs, taken from each sub-budget's share.
    pub budget_cost: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub principals: Vec<PrincipalSpec>,
    /// Keys `keyd` holds.
    pub keyd_keys: Vec<u64>,
    /// The shared servers a session's namespace holds a fresh connection to.
    pub servers: u16,
    pub sizes: Sizes,
}

/// A principal, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub name: String,
    pub account: NonZeroU64,
    pub login_keys: Vec<u64>,
    pub approval_keys: Vec<u64>,
    pub owned: Labels,
    pub domains: Vec<Domain>,
    pub top: Limits,
}

/// The fixed part of the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixed {
    pub principals: Vec<Principal>,
    pub keyd: BTreeSet<u64>,
    pub servers: u16,
    pub sizes: Sizes,
}

/// What boot carves for one principal: its top budget, and a fixed sub-budget per label set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carve {
    pub account: NonZeroU64,
    pub top: Limits,
    pub subs: Vec<(Domain, Limits)>,
}

impl Fixed {
    /// The manifest, checked: accounts non-zero and distinct, names distinct, label sets valid
    /// and distinct, and no key in two roles or held by `keyd` (R35).
    pub fn new(m: &Manifest) -> Option<Fixed> {
        let mut accounts = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut logins = BTreeSet::new();
        let mut approvals = BTreeSet::new();
        let mut principals = Vec::new();
        for p in &m.principals {
            let account = NonZeroU64::new(p.account)?;
            if !accounts.insert(account) || !names.insert(p.name.clone()) {
                return None;
            }
            logins.extend(p.login_keys.iter().copied());
            approvals.extend(p.approval_keys.iter().copied());
            let mut domains: Vec<Domain> = Vec::new();
            for set in &p.label_sets {
                let d = Domain::new(account, Labels::new(set)?);
                if domains.contains(&d) {
                    return None;
                }
                domains.push(d);
            }
            let principal = Principal {
                name: p.name.clone(),
                account,
                login_keys: p.login_keys.clone(),
                approval_keys: p.approval_keys.clone(),
                owned: Labels::new(&p.owned)?,
                domains,
                top: p.top,
            };
            principals.push(principal);
        }
        let keyd: BTreeSet<u64> = m.keyd_keys.iter().copied().collect();
        if logins.intersection(&approvals).next().is_some()
            || keyd.iter().any(|k| logins.contains(k) || approvals.contains(k))
        {
            return None;
        }
        Some(Fixed { principals, keyd, servers: m.servers, sizes: m.sizes })
    }

    pub fn principal(&self, name: &str) -> Option<usize> {
        self.principals.iter().position(|p| p.name == name)
    }

    /// The principal whose account this is.
    pub fn by_account(&self, account: NonZeroU64) -> Option<usize> {
        self.principals.iter().position(|p| p.account == account)
    }

    /// Boot's carving: an equal share of the top budget per label set, less each sub-budget's own
    /// object.
    pub fn carves(&self) -> Vec<Carve> {
        let cost = self.sizes.budget_cost;
        self.principals
            .iter()
            .map(|p| {
                let n = (p.domains.len() as u64).max(1);
                let share = Limits {
                    pages: (p.top.pages / n).saturating_sub(cost),
                    processes: p.top.processes / n,
                    weight: p.top.weight / n,
                };
                Carve {
                    account: p.account,
                    top: p.top,
                    subs: p.domains.iter().map(|d| (d.clone(), share)).collect(),
                }
            })
            .collect()
    }
}

/// The manifest's lines, read one at a time: `principal "NAME" account=N login=[..]
/// approval=[..] owned=[..] sets=[[..],..] top=P,N,W`, `keyd [..]`, `servers N` and `sizes
/// session=P,N,W agent=.. sub_agent=.. crossing=.. cost=N`. Strict: every field once and no
/// other, `keyd`, `servers` and `sizes` at most once, `sizes` required; `keyd` and `servers` are
/// empty and 0 when absent. A line is refused whole, with why.
#[derive(Default)]
pub struct Lines {
    principals: Vec<PrincipalSpec>,
    keyd: Option<Vec<u64>>,
    servers: Option<u16>,
    sizes: Option<Sizes>,
}

impl Lines {
    pub fn line(&mut self, line: &str) -> Result<(), String> {
        let toks = tokens(line)?;
        let (head, rest) = toks.split_first().ok_or("an empty line")?;
        match *head {
            "principal" => self.principals.push(principal(rest)?),
            "keyd" => once(&mut self.keyd, "keyd", list(single(rest, "keyd [..]")?)?)?,
            "servers" => {
                let n = u64_of(single(rest, "servers N")?)?;
                let n = u16::try_from(n).map_err(|_| format!("`{n}` servers is too many"))?;
                once(&mut self.servers, "servers", n)?
            }
            "sizes" => once(&mut self.sizes, "sizes", sizes(rest)?)?,
            _ => return Err(format!("unknown line `{head}`")),
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Manifest, String> {
        Ok(Manifest {
            principals: self.principals,
            keyd_keys: self.keyd.unwrap_or_default(),
            servers: self.servers.unwrap_or(0),
            sizes: self.sizes.ok_or("no `sizes` line")?,
        })
    }
}

/// The manifest's lines, which [`parse_lines`] reads back: each principal, then `keyd`,
/// `servers` and `sizes`.
pub fn lines(m: &Manifest) -> Vec<String> {
    let show = |l: &Limits| format!("{},{},{}", l.pages, l.processes, l.weight);
    let mut out: Vec<String> = m
        .principals
        .iter()
        .map(|p| {
            let sets: Vec<String> = p.label_sets.iter().map(|l| show_list(l)).collect();
            format!(
                "principal {} account={} login={} approval={} owned={} sets=[{}] top={}",
                quote(p.name.as_bytes()),
                p.account,
                show_list(&p.login_keys),
                show_list(&p.approval_keys),
                show_list(&p.owned),
                sets.join(","),
                show(&p.top)
            )
        })
        .collect();
    let z = &m.sizes;
    out.push(format!("keyd {}", show_list(&m.keyd_keys)));
    out.push(format!("servers {}", m.servers));
    out.push(format!(
        "sizes session={} agent={} sub_agent={} crossing={} cost={}",
        show(&z.session),
        show(&z.agent),
        show(&z.sub_agent),
        show(&z.crossing),
        z.budget_cost
    ));
    out
}

/// The manifest from its lines, the first refusal naming its line from 1.
pub fn parse_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Result<Manifest, String> {
    let mut m = Lines::default();
    for (i, l) in lines.into_iter().enumerate() {
        m.line(l).map_err(|e| format!("line {}: {e}", i + 1))?;
    }
    m.finish()
}

fn once<T>(slot: &mut Option<T>, what: &str, v: T) -> Result<(), String> {
    if slot.replace(v).is_some() {
        return Err(format!("a second `{what}` line"));
    }
    Ok(())
}

fn single<'a>(rest: &[&'a str], shape: &str) -> Result<&'a str, String> {
    match rest {
        [one] => Ok(one),
        _ => Err(format!("not `{shape}`")),
    }
}

/// A line's `key=value` tokens, each key once and every key one of `keys`, in `keys`' order.
fn fields<'a, const N: usize>(toks: &[&'a str], keys: [&str; N]) -> Result<[&'a str; N], String> {
    let mut out = [None; N];
    for t in toks {
        let (k, v) = t.split_once('=').ok_or(format!("`{t}` is not key=value"))?;
        let i = keys.iter().position(|x| *x == k).ok_or(format!("unknown field `{k}`"))?;
        if out[i].replace(v).is_some() {
            return Err(format!("a second `{k}=`"));
        }
    }
    let mut got = [""; N];
    for (i, k) in keys.iter().enumerate() {
        got[i] = out[i].ok_or(format!("no `{k}=`"))?;
    }
    Ok(got)
}

fn principal(toks: &[&str]) -> Result<PrincipalSpec, String> {
    let (name, rest) = toks.split_first().ok_or("a principal has a name")?;
    let [account, login, approval, owned, sets, top] =
        fields(rest, ["account", "login", "approval", "owned", "sets", "top"])?;
    Ok(PrincipalSpec {
        name: string(name)?,
        account: u64_of(account)?,
        login_keys: list(login)?,
        approval_keys: list(approval)?,
        owned: list(owned)?,
        label_sets: lists(sets)?,
        top: limits(top)?,
    })
}

fn sizes(toks: &[&str]) -> Result<Sizes, String> {
    let [session, agent, sub_agent, crossing, cost] =
        fields(toks, ["session", "agent", "sub_agent", "crossing", "cost"])?;
    Ok(Sizes {
        session: limits(session)?,
        agent: limits(agent)?,
        sub_agent: limits(sub_agent)?,
        crossing: limits(crossing)?,
        budget_cost: u64_of(cost)?,
    })
}

/// `P,N,W`: a budget's pages, processes and weight.
pub fn limits(s: &str) -> Result<Limits, String> {
    let mut it = s.split(',').map(u64_of);
    match (it.next(), it.next(), it.next(), it.next()) {
        (Some(pages), Some(processes), Some(weight), None) => {
            Ok(Limits { pages: pages?, processes: processes?, weight: weight? })
        }
        _ => Err(format!("`{s}` is not pages,processes,weight")),
    }
}

/// Names no principal takes: each is a terminal's own user name (`ssh approve@box`), which takes no
/// label and no context (servers/steward.md, "Contexts").
pub const RESERVED: [&str; 1] = ["approve"];

/// A principal's, a label's or a context's name: 1 to 64 bytes of `[a-z0-9_-]`, starting with a
/// letter (servers/steward.md, "Contexts"). `init` holds principals and labels to it; the steward
/// holds a login's context to it, whatever `sshd` parsed.
pub fn name(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() <= 64
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|&c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

// The lexical layer the trace encoding shares (servers/steward.md, "The trace encoding").

/// The tokens of a line, split at spaces outside a quoted string. `#` outside a string starts a
/// comment.
pub fn tokens(line: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let (mut start, mut quoted, mut escaped) = (None, false, false);
    for (i, c) in line.char_indices() {
        if quoted {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            ' ' => {
                if let Some(s) = start.take() {
                    out.push(&line[s..i]);
                }
            }
            '#' if start.is_none() => break,
            _ => {
                start.get_or_insert(i);
                quoted = c == '"';
            }
        }
    }
    if quoted {
        return Err(format!("unclosed string in `{line}`"));
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    Ok(out)
}

/// A decimal number: digits only.
pub fn u64_of(s: &str) -> Result<u64, String> {
    let bad = || format!("`{s}` is not a number");
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    s.parse().map_err(|_| bad())
}

/// `[1,2,3]`, or `[]`.
pub fn list(s: &str) -> Result<Vec<u64>, String> {
    let inner =
        s.strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or(format!("`{s}` is not a list"))?;
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner.split(',').map(u64_of).collect()
}

/// `[[],[7],[7,9]]`, or `[]`.
pub fn lists(s: &str) -> Result<Vec<Vec<u64>>, String> {
    let bad = || format!("`{s}` is not a list of lists");
    let inner = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or_else(bad)?;
    let mut out = Vec::new();
    let mut rest = inner;
    while !rest.is_empty() {
        let end = rest.find(']').ok_or_else(bad)?;
        out.push(list(&rest[..=end]).map_err(|_| bad())?);
        rest = &rest[end + 1..];
        if !rest.is_empty() {
            rest = rest.strip_prefix(',').filter(|r| !r.is_empty()).ok_or_else(bad)?;
        }
    }
    Ok(out)
}

/// A quoted string's bytes: `\"`, `\\`, `\n` and `\xNN` are the escapes, and a bare `"` inside is
/// refused.
pub fn bytes(s: &str) -> Result<Vec<u8>, String> {
    let bad = || format!("`{s}` is not a string");
    let inner = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or_else(bad)?;
    let mut out = Vec::new();
    let mut it = inner.bytes();
    while let Some(b) = it.next() {
        match b {
            b'"' => return Err(bad()),
            b'\\' => {}
            _ => {
                out.push(b);
                continue;
            }
        }
        match it.next() {
            Some(b'n') => out.push(b'\n'),
            Some(b'x') => {
                let hex = |d: Option<u8>| d.and_then(|d| (d as char).to_digit(16));
                match (hex(it.next()), hex(it.next())) {
                    (Some(h), Some(l)) => out.push((h * 16 + l) as u8),
                    _ => return Err(format!("bad \\x escape in {s}")),
                }
            }
            Some(c @ (b'"' | b'\\')) => out.push(c),
            _ => return Err(format!("bad escape in {s}")),
        }
    }
    Ok(out)
}

/// A quoted string that is UTF-8.
pub fn string(s: &str) -> Result<String, String> {
    String::from_utf8(bytes(s)?).map_err(|_| format!("{s} is not UTF-8"))
}

/// Bytes as a quoted string: printable ASCII as itself, `"` and `\` escaped, a newline as `\n`,
/// and every other byte as `\xNN`.
pub fn quote(b: &[u8]) -> String {
    let mut s = String::from("\"");
    for &c in b {
        match c {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('"');
    s
}

/// A list as the lines write it: `[1,2,3]`.
pub fn show_list(l: &[u64]) -> String {
    let items: Vec<String> = l.iter().map(u64::to_string).collect();
    format!("[{}]", items.join(","))
}
