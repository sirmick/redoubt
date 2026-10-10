//! The steward core's event traces (servers/steward.md, "Two embedders and a reference"): a
//! trace's text encoding, the core's output for it, and the check that the Elixir reference gave
//! the same output, byte for byte, and took every row of every table.
//!
//! Host-only (`std`), outside the shipped core; nothing here runs on Redoubt.

pub mod input;
pub mod output;
pub mod record;
pub mod text;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use redoubt_steward::effect::Output;
use redoubt_steward::event::{Event, EventKind};
use redoubt_steward::{Policy, Store, decide};

/// The core's output for a trace: what boot fixed and carved, then for each event its effects and
/// the store after it.
pub fn run(text: &str) -> Result<String, String> {
    let trace = input::parse(text)?;
    let mut s = String::from("boot\n");
    let Some((mut store, carves)) = Store::boot(&trace.manifest, Policy::SHIPPED) else {
        s.push_str("refused\n");
        return Ok(s);
    };
    output::boot(&mut s, &store, &carves);
    // `hash=shown`: the hash the last screen of that request showed, as an approver would copy it.
    let mut shown: BTreeMap<u64, [u8; 32]> = BTreeMap::new();
    for (i, line) in trace.events.iter().enumerate() {
        let mut event: Event = line.event.clone();
        if let (true, EventKind::Approve { request, hash, .. }) = (line.shown, &mut event.kind) {
            *hash = shown.get(request).copied().unwrap_or([0; 32]);
        }
        let effects = decide(&mut store, event);
        for o in &effects.outputs {
            if let Output::Screen { screen, .. } = o {
                shown.insert(screen.id, screen.hash);
            }
        }
        let _ = writeln!(s, "event {}", i + 1);
        output::effects(&mut s, &effects);
        s.push_str("store\n");
        output::store(&mut s, &store);
    }
    Ok(s)
}

/// An output's blocks: `boot`, then `event N` for each event, each with its lines.
fn blocks(out: &str) -> Vec<Vec<&str>> {
    let mut b: Vec<Vec<&str>> = Vec::new();
    for line in out.lines() {
        if line == "boot" || line.starts_with("event ") || b.is_empty() {
            b.push(Vec::new());
        }
        b.last_mut().map(|v| v.push(line));
    }
    b
}

/// The first divergence between the core's output and the reference's for one trace, as a
/// report: the trace, the event whose output differs and its input line, the first differing line
/// from each side (the core's, then the reference's), and the rest of that event's output from
/// both. `None` if they are equal byte for byte.
pub fn compare(name: &str, input: &str, rust: &str, elixir: &str) -> Option<String> {
    if rust == elixir {
        return None;
    }
    let (r, e) = (blocks(rust), blocks(elixir));
    let n = (0..r.len().max(e.len())).find(|&i| r.get(i) != e.get(i)).unwrap_or(0);
    let empty = Vec::new();
    let (rb, eb) = (r.get(n).unwrap_or(&empty), e.get(n).unwrap_or(&empty));
    let k = (0..rb.len().max(eb.len())).find(|&i| rb.get(i) != eb.get(i)).unwrap_or(0);
    let mut s = format!("DIVERGES {name}: ");
    match n {
        0 => s.push_str("at boot\n"),
        n => {
            let _ = writeln!(s, "event {n}: {}", events(input).get(n - 1).unwrap_or(&""));
        }
    }
    let line = |b: &Vec<&str>| b.get(k).map_or("(nothing)".to_string(), |l| l.to_string());
    let _ = writeln!(s, "  rust:   {}", line(rb));
    let _ = writeln!(s, "  elixir: {}", line(eb));
    for (side, b) in [("rust", rb), ("elixir", eb)] {
        let _ = writeln!(s, "  the rest of the event, {side}:");
        for l in b.iter().skip(k + 1) {
            let _ = writeln!(s, "    {l}");
        }
    }
    Some(s)
}

/// The events of a trace's input: its `event` lines.
pub fn events(input: &str) -> Vec<&str> {
    input.lines().map(str::trim).filter(|l| l.starts_with("event ")).collect()
}

/// A trace with no events drives the core through nothing and checks nothing: a failure, as a
/// report, else `None`.
pub fn no_events(name: &str, input: &str) -> Option<String> {
    events(input).is_empty().then(|| format!("EMPTY {name}: no events, so it checks nothing\n"))
}

/// Rows of the tables: (machine, line of its table file).
pub type Rows = BTreeSet<(String, usize)>;

/// The reference's output for a run over several traces: each trace's output after a line
/// `trace NAME`, with the rows it took as lines `row MACHINE LINE` among it, which are not
/// output, and a line `done` at the end, after which nothing is read.
pub struct Reference {
    pub traces: BTreeMap<String, (String, Rows)>,
}

pub fn reference(out: &str) -> Result<Reference, String> {
    let mut traces: BTreeMap<String, (String, Rows)> = BTreeMap::new();
    let mut current: Option<&mut (String, Rows)> = None;
    let mut done = false;
    for line in out.lines() {
        if line == "done" {
            done = true;
            break;
        }
        if let Some(name) = line.strip_prefix("trace ") {
            current = Some(traces.entry(name.to_string()).or_default());
            continue;
        }
        let Some(t) = current.as_mut() else {
            return Err(format!("output before the first `trace` line: `{line}`"));
        };
        if let Some(row) = line.strip_prefix("row ") {
            let (m, l) = row.split_once(' ').ok_or(format!("not a row: `{line}`"))?;
            t.1.insert((m.to_string(), text::u64_of(l)? as usize));
        } else {
            t.0.push_str(line);
            t.0.push('\n');
        }
    }
    if !done {
        return Err("the reference's output stops before its `done` line".into());
    }
    Ok(Reference { traces })
}

/// The rows of the tables no trace took: `machine` (`file:line`) for each.
pub fn untaken(machines: &[redoubt_steward_gen::Machine], taken: &Rows) -> Vec<String> {
    let mut out = Vec::new();
    for m in machines {
        for r in &m.rows {
            if !taken.contains(&(m.name.clone(), r.line)) {
                out.push(format!("{} ({}:{})", m.name, m.source, r.line));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACE: &str = "principal \"alice\" account=1 login=[11] approval=[21] owned=[7] sets=[[],[7]] top=100,10,10 contexts=2 idle=300\n\
                         sizes session=10,1,1 agent=10,1,1 sub_agent=5,1,1 crossing=2,1,1 cost=1\n\
                         event now=1 random=[5,6] reply=1 Login principal=\"alice\" labels=[] key=11\n\
                         event now=2 random=[7,8] reply=2 Login principal=\"alice\" labels=[7] key=99\n";

    #[test]
    fn equal_outputs_pass() {
        let out = run(TRACE).unwrap();
        assert!(out.contains("event 2\nreply 2 refused BadKey\n"), "{out}");
        assert_eq!(compare("t", TRACE, &out, &out), None);
    }

    /// The harness can fail: one changed output line is a divergence, reported at its event with
    /// its input line, each side's line, and the rest of that event's output.
    #[test]
    fn a_changed_output_line_is_caught() {
        let out = run(TRACE).unwrap();
        let changed = out.replacen("reply 2 refused BadKey", "reply 2 refused NotOwner", 1);
        let report = compare("t", TRACE, &out, &changed).expect("a changed line diverges");
        assert!(report.starts_with("DIVERGES t: event 2: event now=2"), "{report}");
        assert!(report.contains("  rust:   reply 2 refused BadKey\n"), "{report}");
        assert!(report.contains("  elixir: reply 2 refused NotOwner\n"), "{report}");
        assert!(report.contains("the rest of the event, elixir:\n    store\n"), "{report}");
        // A line missing at the end is caught too.
        let short = &out[..out.trim_end().rfind('\n').unwrap() + 1];
        assert!(compare("t", TRACE, &out, short).unwrap().contains("  elixir: (nothing)"));
    }

    /// A trace that names no event checks nothing, and fails rather than passing as equal.
    #[test]
    fn a_trace_with_no_events_fails() {
        let manifest: String =
            TRACE.lines().filter(|l| !l.starts_with("event ")).map(|l| l.to_string() + "\n").collect();
        // Equal on both sides, as an empty run always is: the comparison alone would pass it.
        let out = run(&manifest).unwrap();
        assert_eq!(compare("t", &manifest, &out, &out), None);
        assert!(no_events("t", &manifest).unwrap().starts_with("EMPTY t: "));
        assert_eq!(no_events("t", TRACE), None);
    }

    #[test]
    fn the_reference_output_splits_into_traces_and_rows() {
        let r =
            reference("trace a\nboot\nrefused\ntrace b\nboot\nrow session 12\nevent 1\ndone\nok\n").unwrap();
        assert_eq!(r.traces["a"].0, "boot\nrefused\n");
        assert_eq!(r.traces["b"].0, "boot\nevent 1\n");
        assert!(r.traces["b"].1.contains(&("session".into(), 12)));
        assert!(reference("stray\ndone\n").is_err());
        // A run cut short is not a reference's output.
        assert!(reference("trace a\nboot\n").is_err());
    }

    /// What the writer writes, the reader reads back to the same events: the model's recorded
    /// traces run as written.
    #[test]
    fn a_written_trace_reads_back() {
        let t = input::parse(TRACE).unwrap();
        let events: Vec<Event> = t.events.iter().map(|l| l.event.clone()).collect();
        let written = record::trace("recorded", &t.manifest, &events);
        let again = input::parse(&written).unwrap();
        assert_eq!(again.manifest, t.manifest);
        assert!(again.events.iter().map(|l| &l.event).eq(events.iter()));
        assert_eq!(run(&written).unwrap(), run(TRACE).unwrap());
    }

    #[test]
    fn strings_round_trip() {
        let b = b"a \"q\" \\ \n\x01\xff~".to_vec();
        assert_eq!(text::bytes(&text::quote(&b)).unwrap(), b);
        assert_eq!(text::tokens("x=\"a b # c\" y=1 # note").unwrap(), ["x=\"a b # c\"", "y=1"]);
    }
}
