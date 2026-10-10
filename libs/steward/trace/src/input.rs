//! A trace's input: the boot manifest, then one event per line (servers/steward.md, "The trace
//! encoding").

use std::num::NonZeroU64;

use redoubt_steward::consts::RANDOM_WORDS;
use redoubt_steward::domain::{Domain, Labels};
use redoubt_steward::effect::{Kind, Object, Produced, StepFailed};
use redoubt_steward::event::{Content, Event, EventKind};
use redoubt_steward::manifest::{Lines, Manifest};

use crate::text::{bytes, field, hash, list, string, tokens, u64_of};

/// One event line: its line number and text, the event, and whether its hash is the one the last
/// screen of its request showed (`hash=shown`).
pub struct Line {
    pub number: usize,
    pub text: String,
    pub event: Event,
    pub shown: bool,
}

pub struct Trace {
    pub manifest: Manifest,
    pub events: Vec<Line>,
}

/// The `key=value` tokens of a line after its head.
struct Fields<'a>(Vec<&'a str>);

impl<'a> Fields<'a> {
    fn get(&self, key: &str) -> Result<&'a str, String> { self.opt(key).ok_or(format!("no `{key}=`")) }

    fn opt(&self, key: &str) -> Option<&'a str> { self.0.iter().find_map(|t| field(t, key)) }

    fn u64(&self, key: &str) -> Result<u64, String> { u64_of(self.get(key)?) }

    fn list(&self, key: &str) -> Result<Vec<u64>, String> { list(self.get(key)?) }
}

/// `1/` or `1/7,9`.
pub fn domain(s: &str) -> Result<Domain, String> {
    let (a, l) = s.split_once('/').ok_or(format!("`{s}` is not account/labels"))?;
    let account = NonZeroU64::new(u64_of(a)?).ok_or(format!("`{s}` has account 0"))?;
    let labels: Vec<u64> =
        if l.is_empty() { Vec::new() } else { l.split(',').map(u64_of).collect::<Result<_, _>>()? };
    Ok(Domain::new(account, Labels::new(&labels).ok_or(format!("`{s}` has too many labels"))?))
}

pub fn kind(s: &str) -> Result<Kind, String> {
    Ok(match s {
        "session" => Kind::Session,
        "lease" => Kind::Lease,
        "request" => Kind::Request,
        "crossing" => Kind::Crossing,
        "blame" => Kind::Blame,
        "channel" => Kind::Channel,
        _ => return Err(format!("`{s}` is not an object kind")),
    })
}

/// `session@1/7#100`.
pub fn object(s: &str) -> Result<Object, String> {
    let bad = || format!("`{s}` is not kind@domain#id");
    let (k, rest) = s.split_once('@').ok_or_else(bad)?;
    let (d, id) = rest.rsplit_once('#').ok_or_else(bad)?;
    Ok(Object { domain: domain(d)?, kind: kind(k)?, id: u64_of(id)? })
}

fn produced(s: &str) -> Result<Produced, String> {
    let arg = |p: &str| s.strip_prefix(p).and_then(|r| r.strip_prefix('(')).and_then(|r| r.strip_suffix(')'));
    Ok(match s {
        "scope" => Produced::Scope,
        "connection" => Produced::Connection,
        "done" => Produced::Done,
        _ => {
            if let Some(b) = arg("budget") {
                Produced::Budget(u64_of(b)?)
            } else if let Some(p) = arg("process") {
                Produced::Process(u64_of(p)?)
            } else if let Some(b) = arg("bytes") {
                Produced::Bytes(bytes(b)?)
            } else {
                return Err(format!("`{s}` is not something a step makes"));
            }
        }
    })
}

/// `[budget(5),scope,bytes("a,b")]`: split at commas outside strings and parentheses.
fn produced_list(s: &str) -> Result<Vec<Produced>, String> {
    let inner =
        s.strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or(format!("`{s}` is not a list"))?;
    let (mut out, mut start, mut depth, mut quoted, mut escaped) = (Vec::new(), 0, 0, false, false);
    for (i, c) in inner.char_indices() {
        match (quoted, escaped, c) {
            (true, true, _) => escaped = false,
            (true, false, '\\') => escaped = true,
            (_, false, '"') => quoted = !quoted,
            (false, _, '(') => depth += 1,
            (false, _, ')') => depth -= 1,
            (false, _, ',') if depth == 0 => {
                out.push(produced(&inner[start..i])?);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !inner.is_empty() {
        out.push(produced(&inner[start..])?);
    }
    Ok(out)
}

fn content(f: &Fields<'_>) -> Result<Content, String> {
    if let Some(what) = f.opt("note") {
        return Ok(Content::Note { what: string(what)? });
    }
    if let Some(labels) = f.opt("agent") {
        return Ok(Content::Agent { labels: list(labels)?, lease: f.u64("lease")? });
    }
    if let Some(labels) = f.opt("declassify") {
        return Ok(Content::Declassify { labels: list(labels)?, item: f.u64("item")? });
    }
    if let Some(target) = f.opt("push") {
        return Ok(Content::Push { source: f.u64("source")?, target: list(target)?, item: f.u64("item")? });
    }
    Err("a Submit names one of note=, agent=, declassify=, push=".into())
}

fn event_kind(name: &str, f: &Fields<'_>) -> Result<(EventKind, bool), String> {
    let mut shown = false;
    let kind = match name {
        // `context=` and `from=` may be left out: the default context, and no address.
        "Login" => EventKind::Login {
            principal: string(f.get("principal")?)?,
            labels: f.list("labels")?,
            context: f.opt("context").map(string).transpose()?.unwrap_or_default(),
            key: f.u64("key")?,
            from: f.opt("from").map(string).transpose()?.unwrap_or_default(),
        },
        "Console" => EventKind::Console { principal: string(f.get("principal")?)? },
        "ChannelClosed" => EventKind::ChannelClosed { session: f.u64("session")? },
        "SshdGone" => EventKind::SshdGone,
        "ApprovalOpened" => EventKind::ApprovalOpened {
            channel: f.u64("channel")?,
            principal: string(f.get("principal")?)?,
            key: f.u64("key")?,
        },
        "ApprovalClosed" => EventKind::ApprovalClosed { channel: f.u64("channel")? },
        "StartAgent" => EventKind::StartAgent { badge: f.u64("badge")?, lease: f.u64("lease")? },
        "Submit" => EventKind::Submit {
            badge: f.u64("badge")?,
            content: content(f)?,
            reason: string(f.get("reason")?)?,
        },
        "EndLease" => EventKind::EndLease { badge: f.u64("badge")?, lease: f.u64("lease")? },
        "EndSession" => EventKind::EndSession { badge: f.u64("badge")? },
        "Contexts" => EventKind::Contexts { badge: f.u64("badge")? },
        "Leave" => EventKind::Leave { badge: f.u64("badge")? },
        "EndContext" => EventKind::EndContext { badge: f.u64("badge")?, name: string(f.get("name")?)? },
        "Idle" => EventKind::Idle,
        "Pending" => EventKind::Pending { channel: f.u64("channel")? },
        "Approve" => {
            let h = f.get("hash")?;
            shown = h == "shown";
            let hash = if shown { [0; 32] } else { hash(h)? };
            EventKind::Approve { channel: f.u64("channel")?, request: f.u64("request")?, hash }
        }
        "Deny" => EventKind::Deny { channel: f.u64("channel")?, request: f.u64("request")? },
        "Blame" => EventKind::Blame { account: f.u64("account")?, labels: f.list("labels")? },
        "Exited" => EventKind::Exited { object: object(f.get("object")?)? },
        "Done" => {
            let result = match (f.opt("ok"), f.opt("failed")) {
                (Some(ok), None) => Ok(produced_list(ok)?),
                (None, Some(failed)) => {
                    let (step, error) = failed.split_once(',').ok_or("failed=STEP,ERROR")?;
                    Err(StepFailed { step: u64_of(step)? as usize, error: u64_of(error)? as u32 })
                }
                _ => return Err("a Done has one of ok= and failed=".into()),
            };
            EventKind::Done { object: object(f.get("object")?)?, result }
        }
        _ => return Err(format!("unknown event `{name}`")),
    };
    Ok((kind, shown))
}

fn event(toks: &[&str]) -> Result<(Event, bool), String> {
    let at = toks.iter().position(|t| !t.contains('=')).ok_or("an event line names its event")?;
    let head = Fields(toks[..at].to_vec());
    let words = head.opt("random").map(list).transpose()?.unwrap_or_default();
    if words.len() > RANDOM_WORDS {
        return Err(format!("at most {RANDOM_WORDS} random words"));
    }
    let mut random = [0; RANDOM_WORDS];
    random[..words.len()].copy_from_slice(&words);
    let now = head.u64("now")?;
    let reply = head.opt("reply").map(u64_of).transpose()?.unwrap_or(0);
    let (kind, shown) = event_kind(toks[at], &Fields(toks[at + 1..].to_vec()))?;
    Ok((Event { now, random, reply, kind }, shown))
}

/// A trace file: the manifest's lines (`principal`, `keyd`, `servers`, `sizes`), read by the core's
/// parser, before the first `event` line. Blank lines and `#` comments are skipped.
pub fn parse(text: &str) -> Result<Trace, String> {
    let (mut lines, mut events) = (Lines::default(), Vec::new());
    for (i, raw) in text.lines().enumerate() {
        let at = |e: String| format!("line {}: {e}", i + 1);
        let toks = tokens(raw).map_err(at)?;
        let Some((head, rest)) = toks.split_first() else { continue };
        match *head {
            "event" => {
                let (event, shown) = event(rest).map_err(at)?;
                events.push(Line { number: i + 1, text: raw.trim().to_string(), event, shown });
            }
            _ if !events.is_empty() => return Err(at(format!("`{head}` after the first event"))),
            _ => lines.line(raw).map_err(at)?,
        }
    }
    Ok(Trace { manifest: lines.finish()?, events })
}
