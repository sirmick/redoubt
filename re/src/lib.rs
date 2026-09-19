//! OTP's `re` module for beamlet, on `regex-automata`.
//!
//! BEAM's `re` is PCRE, in C. This is the same Erlang API over a pure-Rust engine that runs in
//! time linear in the input, whatever the pattern: no catastrophic backtracking, so a hostile
//! pattern or subject cannot make a process spin. The price is PCRE-only syntax: backreferences
//! and lookaround do not compile (`re:compile/2` returns `{error, {Reason, Position}}`).
//!
//! The natives are `re:compile/1,2`, `re:run/2,3`, `re:internal_run/4`, `re:inspect/2` and
//! `re:version/0`; `re:replace` and `re:split` are OTP's Erlang code on top of them.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use beamlet_vm::bif::{Ctx, NativeSpec};
use beamlet_vm::term::Resource;
use beamlet_vm::{Exception, Term};
use regex_automata::meta::Regex;
use regex_automata::util::syntax;
use regex_automata::{Anchored, Input};

type R = Result<Term, Exception>;

/// Largest compiled automaton a pattern may produce, in bytes.
const NFA_SIZE_LIMIT: usize = 1 << 20;
/// Memory for the lazy DFA's cache, per pattern.
const CACHE_CAPACITY: usize = 1 << 20;

pub static NATIVES: &[NativeSpec] = &[
    ("re", "compile", 1, compile),
    ("re", "compile", 2, compile),
    ("re", "run", 2, run),
    ("re", "run", 3, run),
    ("re", "internal_run", 4, run),
    ("re", "inspect", 2, inspect),
    ("re", "version", 0, version),
    ("re", "import", 1, import),
];

/// A compiled pattern (the resource inside `{re_pattern, Groups, Unicode, CrLf, Resource}`).
struct Compiled {
    regex: Regex,
    /// A lookbehind at the start of the pattern and a lookahead at its end (see [`split_edges`]),
    /// checked around each match of `regex`.
    behind: Option<Edge>,
    ahead: Option<Edge>,
    unicode: bool,
    /// Group names by index (index 0, the whole match, has none).
    names: Vec<Option<String>>,
}

/// A lookaround assertion at the edge of a pattern: its body, and whether it is negative.
struct Edge {
    regex: Regex,
    negative: bool,
    /// For a lookbehind: the most bytes its body can match (PCRE requires a bound).
    max_len: usize,
}

/// Compile options that change the pattern.
#[derive(Default, Clone, Copy)]
struct Flags {
    unicode: bool,
    caseless: bool,
    multiline: bool,
    dotall: bool,
    extended: bool,
    ungreedy: bool,
    crlf: bool,
    anchored: bool,
    /// `$` matches only at the very end (not also before a final newline).
    dollar_endonly: bool,
}

fn atom(t: &Term) -> Option<&str> {
    match t {
        Term::Atom(a) => Some(a.as_str()),
        _ => None,
    }
}

/// Apply one compile option. `false` if it is not a compile option.
fn compile_option(f: &mut Flags, o: &Term) -> bool {
    match atom(o) {
        Some("unicode") => f.unicode = true,
        Some("caseless") => f.caseless = true,
        Some("multiline") => f.multiline = true,
        Some("dotall") => f.dotall = true,
        Some("extended") => f.extended = true,
        Some("ungreedy") => f.ungreedy = true,
        Some("anchored") => f.anchored = true,
        // Accepted with no effect here: optimisation hints, and PCRE behaviours this engine
        // has anyway (UCP classes under `unicode`, no start optimisation to disable).
        Some("dollar_endonly") => f.dollar_endonly = true,
        Some("ucp" | "no_start_optimize" | "no_auto_capture" | "never_utf" | "dupnames"
            | "firstline" | "bsr_anycrlf" | "bsr_unicode" | "report_errors") => {}
        _ => match o.as_tuple() {
            Some([k, v]) if atom(k) == Some("newline") => f.crlf = matches!(atom(v), Some("crlf" | "anycrlf" | "any")),
            _ => return false,
        },
    }
    true
}

/// Bytes of a subject or pattern: a binary is used as it is; a list is characters (encoded as
/// UTF-8 under `unicode`, else as Latin-1 bytes).
fn text(t: &Term, unicode: bool) -> Option<Vec<u8>> {
    if let Some(b) = t.iodata_bytes() {
        return Some(b);
    }
    if !unicode {
        return None;
    }
    let mut s = String::new();
    let mut work = alloc::vec![t.clone()];
    while let Some(t) = work.pop() {
        match t {
            Term::Nil => {}
            Term::Bits(b) if b.is_binary() => s.push_str(core::str::from_utf8(&b.to_bytes()).ok()?),
            Term::Cons(c) => {
                work.push(c.tail.clone());
                match &c.head {
                    Term::Int(i) => s.push(u32::try_from(*i).ok().and_then(char::from_u32)?),
                    h => work.push(h.clone()),
                }
            }
            _ => return None,
        }
    }
    Some(s.into_bytes())
}

/// Rewrite the PCRE constructs that Rust's regex syntax spells differently, so that the same
/// pattern means the same thing:
/// - inside a character class, `[` is literal in PCRE but opens a nested class in Rust, and
///   `&&`, `--` and `~~` are Rust set operators: escape them (POSIX `[:name:]` classes stay);
/// - `\e` (escape), `\h` (horizontal space) and `\R` (any line break) have no Rust spelling.
///
/// Constructs with no equivalent at all (backreferences, lookaround) are left alone, and the
/// regex parser rejects them.
fn translate(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    let chars: Vec<char> = p.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            match next {
                'e' => out.push_str("\\x1B"),
                'h' if !in_class => out.push_str("[ \\t]"),
                'h' => out.push_str(" \\t"),
                'R' if !in_class => out.push_str("(?:\\r\\n|\\n|\\r|\\x0B|\\x0C)"),
                // In PCRE an escaped non-alphanumeric is itself; Rust reads `\<` and `\>` as word
                // boundaries.
                '<' | '>' => out.push(next),
                // `\p{Lu}`, `\x{263A}`, `\g{1}`...: the braces belong to the escape.
                'p' | 'P' | 'x' | 'o' | 'g' | 'k' if chars.get(i + 2) == Some(&'{') => {
                    let end = chars[i + 2..].iter().position(|&c| c == '}').map_or(chars.len(), |e| i + 2 + e + 1);
                    out.extend(&chars[i..end]);
                    i = end;
                    continue;
                }
                _ => {
                    out.push(ch);
                    out.push(next);
                }
            }
            i += 2;
            continue;
        }
        if in_class {
            match ch {
                // `]` first in a class (after `[` or `[^`) is literal in both syntaxes.
                ']' if i > class_start => in_class = false,
                '[' if chars.get(i + 1) == Some(&':') => {
                    // A POSIX class: copy through its closing `:]`.
                    let end = (i + 2..chars.len().saturating_sub(1)).find(|&j| chars[j] == ':' && chars[j + 1] == ']');
                    if let Some(end) = end {
                        out.extend(&chars[i..end + 2]);
                        i = end + 2;
                        continue;
                    }
                    out.push_str("\\[");
                    i += 1;
                    continue;
                }
                '[' => {
                    out.push_str("\\[");
                    i += 1;
                    continue;
                }
                '&' | '-' | '~' if chars.get(i + 1) == Some(&ch) => {
                    out.push(ch);
                    out.push('\\');
                    out.push(ch);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        } else if ch == '[' {
            in_class = true;
            class_start = i + 1 + usize::from(chars.get(i + 1) == Some(&'^'));
        } else if ch == '{' {
            // PCRE reads `{` as a quantifier only in `{n}`, `{n,}`, `{n,m}` or `{,m}`; anywhere
            // else it is a literal brace, which Rust's syntax wants escaped.
            match quantifier(&chars[i..]) {
                Some((len, lower_missing)) => {
                    out.push('{');
                    if lower_missing {
                        out.push('0');
                    }
                    out.extend(&chars[i + 1..i + len]);
                    i += len;
                }
                None => {
                    out.push_str("\\{");
                    i += 1;
                }
            }
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// If `s` starts with a PCRE counted quantifier, its length and whether its lower bound is
/// missing (`{,m}`).
fn quantifier(s: &[char]) -> Option<(usize, bool)> {
    let close = s.iter().position(|&c| c == '}')?;
    let body = &s[1..close];
    let digits = |p: &[char]| p.iter().all(|c| c.is_ascii_digit());
    let ok = match body.iter().position(|&c| c == ',') {
        None => !body.is_empty() && digits(body),
        Some(comma) => {
            let (lo, hi) = (&body[..comma], &body[comma + 1..]);
            digits(lo) && digits(hi) && (!lo.is_empty() || !hi.is_empty())
        }
    };
    ok.then_some((close + 1, body.first() == Some(&',')))
}

fn build(pattern: &[u8], f: Flags) -> Result<Compiled, (String, usize)> {
    let pattern = if f.unicode {
        core::str::from_utf8(pattern).map_err(|e| (String::from("invalid UTF-8 string"), e.valid_up_to()))?.into()
    } else {
        // Latin-1: every byte is a character; escape the non-ASCII ones as bytes.
        let mut p = String::new();
        for &b in pattern {
            if b < 0x80 {
                p.push(b as char);
            } else {
                p.push_str(&format!("\\x{b:02X}"));
            }
        }
        p
    };
    let cfg = syntax::Config::new()
        .unicode(f.unicode)
        .utf8(f.unicode)
        .case_insensitive(f.caseless)
        .multi_line(f.multiline)
        .dot_matches_new_line(f.dotall)
        .ignore_whitespace(f.extended)
        .swap_greed(f.ungreedy)
        .crlf(f.crlf);
    let original_len = pattern.chars().count();
    let (core, behind, ahead) = split_edges(&pattern, !f.multiline && !f.dollar_endonly);
    let edge = |body: &str, negative: bool, behind: bool| -> Result<Edge, (String, usize)> {
        let body = translate(body);
        // A lookbehind is matched against the text just before the match, ending there.
        let text = if behind { format!("(?:{body})\\z") } else { body.clone() };
        let regex = regex_for(&text, cfg, f, original_len)?;
        let max_len = if behind {
            let hir = regex_syntax::ParserBuilder::new()
                .unicode(f.unicode)
                .utf8(f.unicode)
                .case_insensitive(f.caseless)
                .multi_line(f.multiline)
                .build()
                .parse(&body)
                .map_err(|_| (String::from("lookbehind assertion is not fixed length"), 0))?;
            hir.properties().maximum_len().ok_or((String::from("lookbehind assertion is not fixed length"), 0))?
        } else {
            0
        };
        Ok(Edge { regex, negative, max_len })
    };
    let behind = behind.map(|(b, neg)| edge(&b, neg, true)).transpose()?;
    let ahead = ahead.map(|(a, neg)| edge(&a, neg, false)).transpose()?;
    let pattern = translate(&core);
    let regex = regex_for(&pattern, cfg, f, original_len)?;
    let names = regex.group_info().pattern_names(regex_automata::PatternID::ZERO).map(|n| n.map(String::from)).collect();
    Ok(Compiled { regex, behind, ahead, unicode: f.unicode, names })
}

/// Compile translated pattern text with the limits every pattern gets.
fn regex_for(pattern: &str, cfg: syntax::Config, f: Flags, len: usize) -> Result<Regex, (String, usize)> {
    Regex::builder()
        .syntax(cfg)
        // Bound what a pattern may cost to compile and run: a hostile `(a{1000}){1000}` is an
        // error, not a large allocation.
        .configure(
            Regex::config()
                .utf8_empty(f.unicode)
                .nfa_size_limit(Some(NFA_SIZE_LIMIT))
                .onepass_size_limit(Some(NFA_SIZE_LIMIT))
                .hybrid_cache_capacity(CACHE_CAPACITY)
                .dfa_size_limit(Some(NFA_SIZE_LIMIT)),
        )
        .build(pattern)
        .map_err(|e| pcre_error(&e, len))
}

/// Split a lookbehind off the start of `p` and a lookahead off its end, when they stand at the
/// top level of a pattern without top-level alternatives: `(?<!\\)\|` and `^(?=.+)` (both used by
/// Elixir itself). The engine has no lookaround, since it would lose the linear-time guarantee,
/// but at the edges it can be checked around each match instead. Anything else keeps its
/// lookaround and fails to compile. The lookahead is tried against the leftmost-first match
/// only: unlike PCRE, a shorter match is not tried when it fails.
#[allow(clippy::type_complexity)]
///
/// With `pcre_dollar`, a `$` ending the pattern becomes the lookahead `\n?\z`: outside multiline
/// mode PCRE's `$` also matches before a final newline, where Rust's matches only at the end.
fn split_edges(p: &str, pcre_dollar: bool) -> (String, Option<(String, bool)>, Option<(String, bool)>) {
    let chars: Vec<char> = p.chars().collect();
    let whole = || (String::from(p), None, None);
    // Top-level groups (start, end) and whether there is a top-level `|`.
    let mut groups = Vec::new();
    let (mut depth, mut i, mut bar) = (0usize, 0usize, false);
    let mut open = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' => {
                // Skip the class: `]` right after `[` or `[^` is literal.
                let mut j = i + 1;
                if chars.get(j) == Some(&'^') {
                    j += 1;
                }
                if chars.get(j) == Some(&']') {
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    if chars[j] == '\\' {
                        j += 1;
                    }
                    j += 1;
                }
                i = j;
            }
            '(' => {
                if depth == 0 {
                    open = i;
                }
                depth += 1;
            }
            ')' => {
                if depth == 0 {
                    return whole();
                }
                depth -= 1;
                if depth == 0 {
                    groups.push((open, i));
                }
            }
            '|' if depth == 0 => bar = true,
            _ => {}
        }
        i += 1;
    }
    if depth != 0 || bar {
        return whole();
    }
    let text = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    let kind = |(s, _): (usize, usize)| -> Option<(bool, bool)> {
        // (is lookbehind, is negative)
        match chars.get(s + 1..s + 4).map(|c| c.iter().collect::<String>()).as_deref() {
            Some("?<=") => Some((true, false)),
            Some("?<!") => Some((true, true)),
            _ => match chars.get(s + 1..s + 3).map(|c| c.iter().collect::<String>()).as_deref() {
                Some("?=") => Some((false, false)),
                Some("?!") => Some((false, true)),
                _ => None,
            },
        }
    };
    let (mut from, mut to) = (0, chars.len());
    let mut behind = None;
    let mut ahead = None;
    if let Some(&g) = groups.first().filter(|g| g.0 == 0) {
        if let Some((true, neg)) = kind(g) {
            behind = Some((text(g.0 + 4, g.1), neg));
            from = g.1 + 1;
        }
    }
    if let Some(&g) = groups.last().filter(|g| g.1 + 1 == chars.len() && g.0 >= from) {
        if let Some((false, neg)) = kind(g) {
            ahead = Some((text(g.0 + 3, g.1), neg));
            to = g.0;
        }
    }
    let escaped = |i: usize| chars[..i].iter().rev().take_while(|&&c| c == '\\').count() % 2 == 1;
    if ahead.is_none() && pcre_dollar && to > from && chars[to - 1] == '$' && !escaped(to - 1) {
        ahead = Some((String::from("\\n?\\z"), false));
        to -= 1;
    }
    (text(from, to), behind, ahead)
}

/// A compile error as PCRE2 words it (programs match on these messages, e.g. Elixir's tests), with
/// a position in the pattern: its end for unterminated constructs, as PCRE reports them, and
/// otherwise just after the offending item (in the translated pattern; the two differ only
/// where `translate` rewrote something before that point).
fn pcre_error(e: &regex_automata::meta::BuildError, len: usize) -> (String, usize) {
    use regex_syntax::ast::ErrorKind as K;
    let Some(regex_syntax::Error::Parse(err)) = e.syntax_error() else {
        return (format!("{e}"), 0);
    };
    // PCRE reports where it noticed the problem: just after the offending item, mostly.
    let at = err.span().end.offset.min(len);
    let (msg, pos) = match err.kind() {
        K::ClassUnclosed => ("missing terminating ] for character class", len),
        K::GroupUnclosed => ("missing closing parenthesis", len),
        K::GroupUnopened => ("unmatched closing parenthesis", at),
        K::RepetitionMissing => ("quantifier does not follow a repeatable item", (err.span().start.offset + 1).min(len)),
        K::RepetitionCountInvalid => ("numbers out of order in {} quantifier", at.saturating_sub(1)),
        K::RepetitionCountUnclosed | K::RepetitionCountDecimalEmpty => ("missing } after quantifier", at),
        K::EscapeUnexpectedEof => ("\\ at end of pattern", len),
        K::EscapeUnrecognized => ("unrecognized character follows \\", at),
        K::ClassRangeInvalid => ("range out of order in character class", at),
        K::GroupNameDuplicate { .. } => ("two named subpatterns have the same name (PCRE2_DUPNAMES not set)", (at + 1).min(len)),
        K::GroupNameInvalid | K::GroupNameEmpty => ("subpattern name expected", at),
        K::UnsupportedBackreference => ("backreferences are not supported", at),
        K::UnsupportedLookAround => ("lookaround assertions are not supported", at),
        _ => return (format!("{}", err.kind()), at),
    };
    (String::from(msg), pos)
}

fn error_tuple(c: &mut Ctx, msg: &str, pos: usize) -> Term {
    let chars = Term::list(msg.chars().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>());
    Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), Term::tuple(alloc::vec![chars, Term::Int(pos as i64)])])
}

fn mp_term(c: &mut Ctx, compiled: Compiled) -> Term {
    let groups = compiled.names.len() as i64 - 1;
    let unicode = compiled.unicode as i64;
    let res = Term::Resource(Rc::new(Resource { id: c.sys.make_ref().0, value: Box::new(compiled) }));
    Term::tuple(alloc::vec![c.atom("re_pattern"), Term::Int(groups), Term::Int(unicode), Term::Int(0), res])
}

fn compiled_of(t: &Term) -> Option<&Compiled> {
    match t.as_tuple() {
        Some([tag, _, _, _, Term::Resource(r)]) if atom(tag) == Some("re_pattern") => r.get::<Compiled>(),
        _ => None,
    }
}

/// `compile(Regexp[, Options])` → `{ok, MP}` or `{error, {Reason, Position}}`. With `export`,
/// `MP` is `{re_exported_pattern, Header, Source, Options, Bytecode}`, as in OTP 28.1; here the
/// bytecode is empty, and `import/1` compiles from the source.
pub fn compile(c: &mut Ctx, a: &[Term]) -> R {
    let mut f = Flags::default();
    let mut export = false;
    let opts = match a.get(1) {
        Some(o) => o.to_vec().ok_or_else(|| c.badarg())?,
        None => Vec::new(),
    };
    for o in &opts {
        if atom(o) == Some("export") {
            export = true;
        } else if !compile_option(&mut f, o) {
            return Err(c.badarg());
        }
    }
    let pattern = text(&a[0], f.unicode).ok_or_else(|| c.badarg())?;
    Ok(match build(&pattern, f) {
        Ok(_) if export => {
            let exported = Term::tuple(alloc::vec![
                c.atom("re_exported_pattern"),
                Term::binary(b"beamlet"),
                Term::binary(&pattern),
                a[1].clone(),
                Term::binary(&[]),
            ]);
            Term::tuple(alloc::vec![c.ok(), exported])
        }
        Ok(compiled) => Term::tuple(alloc::vec![c.ok(), mp_term(c, compiled)]),
        Err((msg, pos)) => error_tuple(c, &msg, pos),
    })
}

/// `import(Exported)`: compile the pattern from the source an exported pattern carries (the
/// fallback OTP documents for a node that cannot use the exporter's bytecode, as here).
pub fn import(c: &mut Ctx, a: &[Term]) -> R {
    let Some([tag, _header, source, opts, _code]) = a[0].as_tuple() else { return Err(c.badarg()) };
    if atom(tag) != Some("re_exported_pattern") {
        return Err(c.badarg());
    }
    let mut f = Flags::default();
    for o in opts.to_vec().ok_or_else(|| c.badarg())? {
        if atom(&o) != Some("export") && !compile_option(&mut f, &o) {
            return Err(c.badarg());
        }
    }
    let pattern = text(source, f.unicode).ok_or_else(|| c.badarg())?;
    match build(&pattern, f) {
        Ok(compiled) => Ok(mp_term(c, compiled)),
        Err(_) => Err(c.badarg()),
    }
}

// ---- run ----

#[derive(Clone)]
enum Spec {
    All,
    AllButFirst,
    First,
    None,
    AllNames,
    List(Vec<Term>),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Index,
    List,
    Binary,
}

struct RunOpts {
    global: bool,
    offset: usize,
    anchored: bool,
    notempty: bool,
    notempty_atstart: bool,
    spec: Spec,
    kind: Kind,
}

fn run_options(c: &Ctx, opts: &[Term], flags: &mut Flags) -> Result<RunOpts, Exception> {
    let mut r = RunOpts {
        global: false,
        offset: 0,
        anchored: false,
        notempty: false,
        notempty_atstart: false,
        spec: Spec::All,
        kind: Kind::Index,
    };
    for o in opts {
        match atom(o) {
            Some("global") => r.global = true,
            Some("anchored") => r.anchored = true,
            Some("notempty") => r.notempty = true,
            Some("notempty_atstart") => r.notempty_atstart = true,
            Some("notbol" | "noteol" | "report_errors") => {}
            _ => match o.as_tuple() {
                Some([k, v]) if atom(k) == Some("offset") => r.offset = v.as_usize().ok_or_else(|| c.badarg())?,
                Some([k, v]) if matches!(atom(k), Some("match_limit" | "match_limit_recursion")) => {
                    // Matching is linear here; limits have nothing to bound.
                    v.as_usize().ok_or_else(|| c.badarg())?;
                }
                Some([k, s]) if atom(k) == Some("capture") => r.spec = spec(c, s)?,
                Some([k, s, t]) if atom(k) == Some("capture") => {
                    r.spec = spec(c, s)?;
                    r.kind = match atom(t) {
                        Some("index") => Kind::Index,
                        Some("list") => Kind::List,
                        Some("binary") => Kind::Binary,
                        _ => return Err(c.badarg()),
                    };
                }
                _ => {
                    if !compile_option(flags, o) {
                        return Err(c.badarg());
                    }
                }
            },
        }
    }
    Ok(r)
}

fn spec(c: &Ctx, t: &Term) -> Result<Spec, Exception> {
    Ok(match atom(t) {
        Some("all") => Spec::All,
        Some("all_but_first") => Spec::AllButFirst,
        Some("first") => Spec::First,
        Some("none") => Spec::None,
        Some("all_names") => Spec::AllNames,
        _ => Spec::List(t.to_vec().ok_or_else(|| c.badarg())?),
    })
}

/// One match: the span of each group (`None` if it did not take part).
type Groups = Vec<Option<(usize, usize)>>;

/// The leftmost match at or after `at` (only at `at` if anchored). `skip_empty_at` rejects an
/// empty match at that position (PCRE's NOTEMPTY_ATSTART); `no_empty` rejects all empty ones.
fn find(re: &Compiled, subject: &[u8], at: usize, anchored: bool, skip_empty_at: Option<usize>, no_empty: bool) -> Option<Groups> {
    let mut caps = re.regex.create_captures();
    let mut start = at;
    loop {
        if start > subject.len() {
            return None;
        }
        let input = Input::new(subject).range(start..).anchored(if anchored { Anchored::Yes } else { Anchored::No });
        re.regex.search_captures(&input, &mut caps);
        let m = caps.get_match()?;
        let empty = m.start() == m.end();
        let rejected = (empty && (no_empty || skip_empty_at == Some(m.start()))) || !edges_hold(re, subject, m.start(), m.end());
        if !rejected {
            let n = caps.group_len();
            return Some((0..n).map(|g| caps.get_group(g).map(|s| (s.start, s.end))).collect());
        }
        if anchored {
            return None;
        }
        // Look again one character further on.
        start = m.start() + char_len(re, subject, m.start());
    }
}

/// Whether the edge lookarounds of `re` hold around a match from `start` to `end`.
fn edges_hold(re: &Compiled, subject: &[u8], start: usize, end: usize) -> bool {
    if let Some(b) = &re.behind {
        let window = &subject[start.saturating_sub(b.max_len)..start];
        let found = b.regex.search(&Input::new(window)).is_some();
        if found == b.negative {
            return false;
        }
    }
    if let Some(a) = &re.ahead {
        let found = a.regex.search(&Input::new(subject).range(end..).anchored(Anchored::Yes)).is_some();
        if found == a.negative {
            return false;
        }
    }
    true
}

/// The length of the character at `i` (1 for Latin-1, or at the end).
fn char_len(re: &Compiled, s: &[u8], i: usize) -> usize {
    if !re.unicode || i >= s.len() {
        return 1;
    }
    match s[i] {
        0xf0..=0xff => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
    .min(s.len() - i)
}

fn capture_term(c: &mut Ctx, re: &Compiled, subject: &[u8], span: Option<(usize, usize)>, kind: Kind) -> Term {
    match (kind, span) {
        (Kind::Index, Some((s, e))) => Term::tuple(alloc::vec![Term::Int(s as i64), Term::Int((e - s) as i64)]),
        (Kind::Index, None) => Term::tuple(alloc::vec![Term::Int(-1), Term::Int(0)]),
        (Kind::Binary, Some((s, e))) => Term::binary(&subject[s..e]),
        (Kind::Binary, None) => Term::binary(&[]),
        (Kind::List, Some((s, e))) => {
            let bytes = &subject[s..e];
            let chars: Vec<Term> = if re.unicode {
                core::str::from_utf8(bytes).map(|t| t.chars().map(|ch| Term::Int(ch as i64)).collect()).unwrap_or_default()
            } else {
                bytes.iter().map(|&b| Term::Int(b as i64)).collect()
            };
            let _ = c;
            Term::list(chars)
        }
        (Kind::List, None) => Term::Nil,
    }
}

/// The captures of one match, as the capture spec asks.
fn captures(c: &mut Ctx, re: &Compiled, subject: &[u8], g: &Groups, o: &RunOpts) -> Result<Term, Exception> {
    // As in PCRE, `all` stops at the last group that matched.
    let last_set = g.iter().rposition(|x| x.is_some()).unwrap_or(0);
    let indices: Vec<Option<usize>> = match &o.spec {
        Spec::All => (0..=last_set).map(Some).collect(),
        Spec::AllButFirst => (1..=last_set).map(Some).collect(),
        Spec::First => alloc::vec![Some(0)],
        Spec::None => Vec::new(),
        Spec::AllNames => {
            let mut named: Vec<(&str, usize)> = re.names.iter().enumerate().filter_map(|(i, n)| n.as_deref().map(|n| (n, i))).collect();
            named.sort();
            named.into_iter().map(|(_, i)| Some(i)).collect()
        }
        Spec::List(items) => {
            let mut v = Vec::new();
            for it in items {
                let idx = match it {
                    Term::Int(i) => usize::try_from(*i).ok().filter(|i| *i < g.len()),
                    Term::Atom(a) => re.names.iter().position(|n| n.as_deref() == Some(a.as_str())),
                    other => {
                        let name = other.iodata_bytes().and_then(|b| String::from_utf8(b).ok());
                        name.and_then(|n| re.names.iter().position(|x| x.as_deref() == Some(n.as_str())))
                    }
                };
                v.push(idx);
            }
            v
        }
    };
    let items: Vec<Term> = indices
        .into_iter()
        .map(|i| capture_term(c, re, subject, i.and_then(|i| g.get(i).copied().flatten()), o.kind))
        .collect();
    Ok(Term::list(items))
}

/// `run(Subject, RE[, Options])` and `internal_run(Subject, RE, Options, FirstCall)`.
pub fn run(c: &mut Ctx, a: &[Term]) -> R {
    let opts = match a.get(2) {
        Some(t) => t.to_vec().ok_or_else(|| c.badarg())?,
        None => Vec::new(),
    };
    let mut flags = Flags::default();
    let o = run_options(c, &opts, &mut flags)?;
    // A pattern given as text is compiled with the compile options among the run options.
    let owned;
    let re = match compiled_of(&a[1]) {
        Some(re) => re,
        None => {
            let pattern = text(&a[1], flags.unicode).ok_or_else(|| c.badarg())?;
            owned = match build(&pattern, flags) {
                Ok(r) => r,
                Err(_) => return Err(c.badarg()),
            };
            &owned
        }
    };
    let subject = text(&a[0], re.unicode).ok_or_else(|| c.badarg())?;
    if o.offset > subject.len() {
        return Err(c.badarg());
    }
    let anchored = o.anchored || flags.anchored;
    if !o.global {
        let skip = o.notempty_atstart.then_some(o.offset);
        return Ok(match find(re, &subject, o.offset, anchored, skip, o.notempty) {
            None => c.atom("nomatch"),
            Some(_) if matches!(o.spec, Spec::None) => c.atom("match"),
            Some(g) => {
                let caps = captures(c, re, &subject, &g, &o)?;
                Term::tuple(alloc::vec![c.atom("match"), caps])
            }
        });
    }
    // Global: after an empty match, look for a non-empty one anchored at the same place, then
    // move on a character (PCRE's documented loop).
    let mut all = Vec::new();
    let mut pos = o.offset;
    let mut after_empty = false;
    while pos <= subject.len() {
        let found = if after_empty {
            find(re, &subject, pos, true, Some(pos), o.notempty)
                .or_else(|| {
                    let next = pos + char_len(re, &subject, pos);
                    (next <= subject.len()).then(|| find(re, &subject, next, anchored, None, o.notempty)).flatten()
                })
        } else {
            find(re, &subject, pos, anchored, o.notempty_atstart.then_some(o.offset).filter(|_| all.is_empty()), o.notempty)
        };
        let Some(g) = found else { break };
        let (s, e) = g[0].expect("a match has group 0");
        all.push(g);
        after_empty = s == e;
        pos = e;
    }
    if all.is_empty() {
        return Ok(c.atom("nomatch"));
    }
    if matches!(o.spec, Spec::None) {
        return Ok(c.atom("match"));
    }
    let mut items = Vec::new();
    for g in &all {
        items.push(captures(c, re, &subject, g, &o)?);
    }
    Ok(Term::tuple(alloc::vec![c.atom("match"), Term::list(items)]))
}

/// `inspect(MP, namelist)` → `{namelist, [Name]}`, names sorted as PCRE lists them.
pub fn inspect(c: &mut Ctx, a: &[Term]) -> R {
    let Some(re) = compiled_of(&a[0]) else { return Err(c.badarg()) };
    if atom(&a[1]) != Some("namelist") {
        return Err(c.badarg());
    }
    let mut names: Vec<&str> = re.names.iter().filter_map(|n| n.as_deref()).collect();
    names.sort();
    let list = Term::list(names.into_iter().map(|n| Term::binary(n.as_bytes())).collect::<Vec<_>>());
    Ok(Term::tuple(alloc::vec![c.atom("namelist"), list]))
}

pub fn version(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::binary(b"regex-automata 0.4 (beamlet)"))
}

#[cfg(test)]
mod tests {
    use super::translate;

    #[test]
    fn pcre_spellings() {
        assert_eq!(translate("[[\\]()#;?]*"), "[\\[\\]()#;?]*");
        assert_eq!(translate("[[:alpha:]]+"), "[[:alpha:]]+");
        assert_eq!(translate("[]a]"), "[]a]");
        assert_eq!(translate("[^]a]"), "[^]a]");
        assert_eq!(translate("[a&&b]"), "[a&\\&b]");
        assert_eq!(translate("\\e\\h"), "\\x1B[ \\t]");
        assert_eq!(translate("a[b-c]d"), "a[b-c]d");
    }

    #[test]
    fn braces_are_quantifiers_only_when_counted() {
        assert_eq!(translate("a{2}b{2,}c{2,5}"), "a{2}b{2,}c{2,5}");
        assert_eq!(translate("x{,3}"), "x{0,3}");
        assert_eq!(translate("{atom, keyword}"), "\\{atom, keyword}");
        assert_eq!(translate("%\\{\\}"), "%\\{\\}");
        assert_eq!(translate("a{}b{,}c{x}"), "a\\{}b\\{,}c\\{x}");
        assert_eq!(translate("[{]"), "[{]");
        assert_eq!(translate("\\p{Lu}\\P{Latin}\\x{263A}{2}"), "\\p{Lu}\\P{Latin}\\x{263A}{2}");
        assert_eq!(translate("#Function\\<.+\\>"), "#Function<.+>");
    }
}
