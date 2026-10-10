//! Writing a trace: a manifest and the events an embedder decided, in the encoding `input`
//! reads (servers/steward.md, "The trace encoding"). The model records its families' runs with it.

use std::fmt::Write as _;

use redoubt_steward::effect::Produced;
use redoubt_steward::event::{Content, Event, EventKind};
use redoubt_steward::manifest::{Manifest, lines};

use crate::output::object;
use crate::text::{hex, quote, show_list};

fn produced(p: &Produced) -> String {
    match p {
        Produced::Budget(b) => format!("budget({b})"),
        Produced::Scope => "scope".into(),
        Produced::Connection => "connection".into(),
        Produced::Process(p) => format!("process({p})"),
        Produced::Bytes(b) => format!("bytes({})", quote(b)),
        Produced::Done => "done".into(),
    }
}

fn kind(k: &EventKind) -> String {
    let q = |s: &str| quote(s.as_bytes());
    match k {
        EventKind::Login { principal, labels, context, key, from } => {
            format!(
                "Login principal={} labels={} context={} key={key} from={}",
                q(principal),
                show_list(labels),
                q(context),
                q(from)
            )
        }
        EventKind::Console { principal } => format!("Console principal={}", q(principal)),
        EventKind::ChannelClosed { session } => format!("ChannelClosed session={session}"),
        EventKind::SshdGone => "SshdGone".into(),
        EventKind::ApprovalOpened { channel, principal, key } => {
            format!("ApprovalOpened channel={channel} principal={} key={key}", q(principal))
        }
        EventKind::ApprovalClosed { channel } => format!("ApprovalClosed channel={channel}"),
        EventKind::StartAgent { badge, lease } => format!("StartAgent badge={badge} lease={lease}"),
        EventKind::Submit { badge, content, reason } => {
            let c = match content {
                Content::Note { what } => format!("note={}", q(what)),
                Content::Agent { labels, lease } => format!("agent={} lease={lease}", show_list(labels)),
                Content::Declassify { labels, item } => {
                    format!("declassify={} item={item}", show_list(labels))
                }
                Content::Push { source, target, item } => {
                    format!("push={} source={source} item={item}", show_list(target))
                }
            };
            format!("Submit badge={badge} {c} reason={}", q(reason))
        }
        EventKind::EndLease { badge, lease } => format!("EndLease badge={badge} lease={lease}"),
        EventKind::EndSession { badge } => format!("EndSession badge={badge}"),
        EventKind::Contexts { badge } => format!("Contexts badge={badge}"),
        EventKind::Leave { badge } => format!("Leave badge={badge}"),
        EventKind::EndContext { badge, name } => format!("EndContext badge={badge} name={}", q(name)),
        EventKind::Idle => "Idle".into(),
        EventKind::Pending { channel } => format!("Pending channel={channel}"),
        EventKind::Approve { channel, request, hash } => {
            format!("Approve channel={channel} request={request} hash={}", hex(hash))
        }
        EventKind::Deny { channel, request } => format!("Deny channel={channel} request={request}"),
        EventKind::Blame { account, labels } => {
            format!("Blame account={account} labels={}", show_list(labels))
        }
        EventKind::Exited { object: o } => format!("Exited object={}", object(o)),
        EventKind::Done { object: o, result } => match result {
            Ok(list) => {
                let l: Vec<String> = list.iter().map(produced).collect();
                format!("Done object={} ok=[{}]", object(o), l.join(","))
            }
            Err(f) => format!("Done object={} failed={},{}", object(o), f.step, f.error),
        },
    }
}

/// One event's line.
pub fn event(e: &Event) -> String {
    format!("event now={} random={} reply={} {}", e.now, show_list(&e.random), e.reply, kind(&e.kind))
}

/// A trace: `comment` (each line after `# `), the manifest, then the events.
pub fn trace(comment: &str, m: &Manifest, events: &[Event]) -> String {
    let mut s = String::new();
    for line in comment.lines() {
        let _ = writeln!(s, "# {line}");
    }
    for line in lines(m) {
        let _ = writeln!(s, "{line}");
    }
    for e in events {
        let _ = writeln!(s, "{}", event(e));
    }
    s
}
