//! The docs checker: holds the pages to the style guide's rules C1..C12 (status lines, tests,
//! milestones, process references, rule IDs, links, the security register, no binaries,
//! templates, wire tables, code comments, the table of contents).
//!
//! Markdown is read line by line: fenced blocks (```` ``` ```` or `~~~`) are tracked, and a
//! heading is a line starting `#`..`######` and a space, outside a fence. Patterns are matched
//! by hand on word tokens (runs of letters, digits and `_`), which is what `\b` bounds.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// What to check. `pages`: report only findings on these paths (all rules still run, since
/// several need every page). `code`: also check code comments and case descriptions (C11).
pub struct Scope {
    pub pages: Option<Vec<PathBuf>>,
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// Repository-relative, `/`-separated.
    pub path: String,
    pub line: usize,
    pub rule: u8,
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: C{}: {}", self.path, self.line, self.rule, self.message)
    }
}

const MILESTONES: [(&str, &str); 5] = [
    ("M1", "separation and containment"),
    ("M2", "usable shell"),
    ("M3", "files in and out"),
    ("M4", "self-hosted development"),
    ("M5", "persist, install, share"),
];
const EXEMPT: [&str; 4] = ["Purpose", "Residual risks", "Why", "How to use it"];
const EXCLUDED: [&str; 3] = ["docs/legacy", "docs/inventory", "docs/theme"];
const ROOT_PAGES: [&str; 3] = ["README.md", "GETTING-STARTED.md", "CONTRIBUTING.md"];
const PACKAGES: [&str; 17] =
    ["SV", "IPC", "DOC", "HIST", "OD", "K", "D", "B", "E", "C", "W", "A", "L", "T", "V", "G", "S"];
const BINARY: [&str; 9] = ["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "pdf"];
const STATUS_TESTED: &str = " · tested: ";

pub fn check(root: &Path, scope: Scope) -> Vec<Finding> {
    let mut c = Ctx { root, out: Vec::new(), crates: None };
    let mut pages: Vec<Page> = page_paths(root).iter().filter_map(|p| Page::load(root, p)).collect();
    for p in &mut pages {
        read_statuses(&mut c, p);
    }
    let defs = definitions(&mut c, &pages);
    for p in &pages {
        status_lines(&mut c, p);
        milestones_and_process(&mut c, p);
        citations(&mut c, p, &defs);
        links(&mut c, p);
        template(&mut c, p);
    }
    security(&mut c, &pages, &defs);
    no_binaries(&mut c);
    wire_tables(&mut c, &pages);
    summary(&mut c, &pages);
    if scope.code {
        code(&mut c, &pages, &defs);
    }
    let mut out = c.out;
    if let Some(keep) = scope.pages {
        let keep: BTreeSet<String> =
            keep.iter().map(|p| slashed(p).trim_start_matches("./").to_string()).collect();
        out.retain(|f| keep.contains(&f.path));
    }
    out.sort();
    out.dedup();
    out
}

struct Ctx<'a> {
    root: &'a Path,
    out: Vec<Finding>,
    /// Package name to crate directory and the names of its `#[test]` functions (C2).
    crates: Option<BTreeMap<String, (String, BTreeSet<String>)>>,
}

impl Ctx<'_> {
    fn err(&mut self, rule: u8, path: &str, line: usize, message: String) {
        self.out.push(Finding { path: path.to_string(), line, rule, message });
    }

    fn exists(&self, rel: &str) -> bool { self.root.join(rel).exists() }
}

// ---- Pages, fences and headings ----

struct Page {
    path: String,
    lines: Vec<String>,
    fenced: Vec<bool>,
    heads: Vec<Head>,
}

struct Head {
    line: usize,
    level: usize,
    text: String,
    /// Index of the nearest earlier heading of lower level.
    parent: Option<usize>,
    status: Option<Status>,
}

impl Page {
    fn load(root: &Path, path: &str) -> Option<Page> {
        let text = fs::read_to_string(root.join(path)).ok()?;
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let fenced = fences(&lines);
        let mut heads: Vec<Head> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if let (false, Some((level, text))) = (fenced[i], heading(l)) {
                let parent = heads.iter().rposition(|h| h.level < level);
                heads.push(Head { line: i, level, text: text.to_string(), parent, status: None });
            }
        }
        Some(Page { path: path.to_string(), lines, fenced, heads })
    }

    /// The heading whose section holds line `i` most closely.
    fn head_of(&self, i: usize) -> Option<usize> { self.heads.iter().rposition(|h| h.line < i) }

    /// The status that covers heading `h`: its own or its nearest ancestor's.
    fn cover(&self, h: usize) -> Option<&Status> {
        let mut at = Some(h);
        while let Some(i) = at {
            if let Some(s) = &self.heads[i].status {
                return Some(s);
            }
            at = self.heads[i].parent;
        }
        None
    }

    /// Line index after heading `h`'s section: the next heading of the same or higher level.
    fn section_end(&self, h: usize) -> usize {
        let level = self.heads[h].level;
        self.heads[h + 1..].iter().find(|x| x.level <= level).map_or(self.lines.len(), |x| x.line)
    }

    fn slugs(&self) -> BTreeSet<String> {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut out = BTreeSet::new();
        for h in &self.heads {
            let base: String = h
                .text
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
                .map(|c| if c == ' ' { '-' } else { c })
                .collect();
            let n = seen.entry(base.clone()).or_insert(0);
            out.insert(if *n == 0 { base } else { format!("{base}-{n}") });
            *n += 1;
        }
        out
    }
}

fn fences(lines: &[String]) -> Vec<bool> {
    let mut open: Option<(char, usize)> = None;
    let mut out = Vec::with_capacity(lines.len());
    for l in lines {
        let t = l.trim_start();
        let c = t.chars().next().unwrap_or(' ');
        let run = if c == '`' || c == '~' { t.chars().take_while(|&x| x == c).count() } else { 0 };
        match open {
            None if run >= 3 => open = Some((c, run)),
            Some((oc, on)) if c == oc && run >= on && t[run..].trim().is_empty() => open = None,
            None => {
                out.push(false);
                continue;
            }
            Some(_) => {}
        }
        out.push(true);
    }
    out
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    let rest = line[level..].strip_prefix(' ')?;
    (1..=6).contains(&level).then(|| (level, rest.trim().trim_end_matches('#').trim()))
}

fn page_paths(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, "docs", &|p| EXCLUDED.contains(&p), &mut out);
    out.retain(|p| p.ends_with(".md"));
    out.extend(ROOT_PAGES.iter().filter(|p| root.join(p).is_file()).map(|p| p.to_string()));
    out.sort();
    out
}

/// Every file under `dir` (repository-relative), skipping directories `skip` names.
fn walk(root: &Path, dir: &str, skip: &dyn Fn(&str) -> bool, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let rel = format!("{dir}/{}", e.file_name().to_string_lossy());
        if e.path().is_dir() {
            if !skip(&rel) {
                walk(root, &rel, skip, out);
            }
        } else {
            out.push(rel);
        }
    }
}

fn slashed(p: &Path) -> String { p.to_string_lossy().replace('\\', "/") }

/// `target` relative to the directory of `from`, normalised; `None` if it leaves the root.
fn resolve(from: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

// ---- Tokens and patterns ----

fn is_word(c: char) -> bool { c.is_alphanumeric() || c == '_' }

/// The maximal word runs of `s`, with their byte offsets.
fn words(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
        match (is_word(c), start) {
            (true, None) => start = Some(i),
            (false, Some(b)) => {
                out.push((b, &s[b..i]));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// `r` is one or more ASCII digits, then (if `letter`) at most one lowercase letter.
fn digits_then(r: &str, letter: bool) -> bool {
    let d = r.bytes().take_while(u8::is_ascii_digit).count();
    let tail = &r.as_bytes()[d..];
    d > 0 && (tail.is_empty() || (letter && tail.len() == 1 && tail[0].is_ascii_lowercase()))
}

/// C5's citation pattern: `R\d+[a-z]?` or `I\d+`, as a whole token.
fn is_rule_id(w: &str) -> bool {
    match w.as_bytes().first() {
        Some(b'R') => digits_then(&w[1..], true),
        Some(b'I') => digits_then(&w[1..], false),
        _ => false,
    }
}

/// `phrase` (lowercase) occurs in `lower` with a word boundary on each side.
fn has_phrase(lower: &str, phrase: &str) -> bool {
    lower.match_indices(phrase).any(|(i, _)| {
        !lower[..i].chars().next_back().is_some_and(is_word)
            && !lower[i + phrase.len()..].chars().next().is_some_and(is_word)
    })
}

/// C4's patterns; commit hashes only when `hashes`. With `checker`, `C1`..`C12` are this
/// checker's rule names, not package IDs (the page that documents the checker). One message per
/// match.
fn process_refs(s: &str, hashes: bool, checker: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (at, w) in words(s) {
        let rest = &s[at + w.len()..];
        let lw = w.to_lowercase();
        let upper = w.bytes().take_while(u8::is_ascii_uppercase).count();
        let dash_lower =
            rest.strip_prefix('-').is_some_and(|r| r.starts_with(|c: char| c.is_ascii_lowercase()));
        if w == "WP"
            && rest.strip_prefix('-').is_some_and(|r| r.starts_with(|c: char| c.is_ascii_uppercase()))
        {
            out.push("process reference `WP-`".into());
        } else if matches!(lw.as_str(), "answer" | "answers" | "question" | "questions")
            && rest.starts_with(char::is_whitespace)
            && rest.trim_start().starts_with(|c: char| c.is_ascii_digit())
        {
            out.push(format!("answer or question number after `{w}`"));
        } else if w == "ANSWERS" || w == "QUESTIONS" {
            out.push(format!("process reference `{w}`"));
        } else if checker
            && w.strip_prefix('C')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=12).contains(&n) && !w.starts_with("C0"))
        {
        } else if PACKAGES.iter().any(|p| w.strip_prefix(p).is_some_and(|r| digits_then(r, true))) {
            out.push(format!("package ID `{w}`"));
        } else if (1..=4).contains(&upper) && digits_then(&w[upper..], true) && dash_lower {
            out.push(format!("thread name `{w}-...`"));
        } else if lw == "wash" {
            out.push("process reference `wash`".into());
        } else if hashes
            && (7..=40).contains(&w.len())
            && w.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && w.bytes().any(|b| b.is_ascii_digit())
            && w.bytes().any(|b| b.is_ascii_alphabetic())
        {
            out.push(format!("commit hash `{w}`"));
        }
    }
    let lower = s.to_lowercase();
    for phrase in ["review round", "qa thread"] {
        if has_phrase(&lower, phrase) {
            out.push(format!("process reference `{phrase}`"));
        }
    }
    out
}

/// `[text](url)` reduced to `text`.
fn strip_links(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find("](").and_then(|m| after[m + 2..].find(')').map(|e| (m, m + 2 + e))) {
            Some((m, e)) if !after[..m].contains('[') => {
                out.push_str(&after[..m]);
                rest = &after[e + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `s` with inline code spans removed.
fn strip_code(s: &str) -> String { s.split('`').step_by(2).collect::<Vec<_>>().join(" ") }

/// The targets of the inline links `[..](target)` on a line.
fn link_targets(s: &str) -> Vec<String> {
    let s = strip_code(s);
    let mut out = Vec::new();
    for (i, _) in s.match_indices("](") {
        if !s[..i].contains('[') {
            continue;
        }
        let t = &s[i + 2..];
        if let Some(end) = t.find(')') {
            let target = t[..end].split_whitespace().next().unwrap_or("");
            out.push(target.trim_start_matches('<').trim_end_matches('>').to_string());
        }
    }
    out
}

fn is_url(t: &str) -> bool {
    match t.find(':') {
        Some(i) if i > 0 => {
            t[..i].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                && !t[..i].contains(['/', '#'])
        }
        _ => false,
    }
}

// ---- C1, C2: status lines and their tests ----

enum Status {
    Built(Vec<String>),
    Partly(Vec<String>),
    Planned(usize),
}

impl Status {
    fn tests(&self) -> &[String] {
        match self {
            Status::Built(t) | Status::Partly(t) => t,
            Status::Planned(_) => &[],
        }
    }

    /// The form of this status in a SECURITY.md Status cell (C7).
    fn cell(&self) -> String {
        match self {
            Status::Built(_) => "built".into(),
            Status::Partly(_) => "built, partly tested".into(),
            Status::Planned(m) => format!("planned · {}", milestone(*m)),
        }
    }
}

fn milestone(m: usize) -> String { format!("{} ({})", MILESTONES[m].0, MILESTONES[m].1) }

/// The S3 grammar, exactly.
fn parse_status(line: &str) -> Option<Status> {
    let s = line.strip_prefix("Status: ")?;
    if let Some(r) = s.strip_prefix("built · tested: ") {
        return parse_tests(r).map(Status::Built);
    }
    if let Some(r) = s.strip_prefix("built · partly tested: ") {
        let (text, tests) = match r.find(STATUS_TESTED) {
            Some(i) => (&r[..i], parse_tests(&r[i + STATUS_TESTED.len()..])?),
            None => (r, Vec::new()),
        };
        return (!text.trim().is_empty()).then_some(Status::Partly(tests));
    }
    let m = s.strip_prefix("planned · ")?;
    (0..MILESTONES.len()).find(|&i| m == milestone(i)).map(Status::Planned)
}

fn parse_tests(r: &str) -> Option<Vec<String>> {
    let name = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'));
    r.split(", ")
        .map(|t| {
            let ok = match t.split_once(':') {
                Some(("bench", c)) => name(c),
                Some(("host", r)) => r.split_once("::").is_some_and(|(p, f)| name(p) && name(f)),
                Some(("mutation", v)) => name(v),
                Some(("fuzz", r)) => r.split_once('/').is_some_and(|(p, f)| name(p) && name(f)),
                _ => false,
            };
            ok.then(|| t.to_string())
        })
        .collect()
}

fn needs_status(path: &str) -> bool {
    ["docs/kernel/", "docs/servers/", "docs/userland/"].iter().any(|d| path.starts_with(d))
        || path == "docs/testbench.md"
}

/// C1(a) and C2: records each heading's own status (C5 and C7 read them) and checks its tests.
fn read_statuses(c: &mut Ctx, p: &mut Page) {
    if !needs_status(&p.path) {
        return;
    }
    for i in 0..p.lines.len() {
        if p.fenced[i] || !p.lines[i].starts_with("Status:") {
            continue;
        }
        let h = p.head_of(i).filter(|&h| {
            p.heads[h].level >= 2 && p.lines[p.heads[h].line + 1..i].iter().all(|l| l.trim().is_empty())
        });
        let Some(h) = h else {
            c.err(1, &p.path, i + 1, "status line is not the first line of a section".into());
            continue;
        };
        let Some(s) = parse_status(p.lines[i].trim_end()) else {
            c.err(1, &p.path, i + 1, "malformed status".into());
            continue;
        };
        for t in s.tests() {
            if !test_exists(c, t) {
                c.err(2, &p.path, i + 1, format!("test `{t}` not found"));
            }
        }
        p.heads[h].status = Some(s);
    }
}

fn status_lines(c: &mut Ctx, p: &Page) {
    if !needs_status(&p.path) {
        return;
    }
    let exempt = |mut h: usize| loop {
        if EXEMPT.contains(&p.heads[h].text.as_str()) {
            return true;
        }
        match p.heads[h].parent {
            Some(up) if p.heads[up].level >= 2 => h = up,
            _ => return false,
        }
    };
    for (h, head) in p.heads.iter().enumerate().filter(|(_, x)| x.level >= 2) {
        let inherited = head.parent.and_then(|up| p.cover(up)).is_some();
        let body_end = p.heads.get(h + 1).map_or(p.lines.len(), |x| x.line);
        let claim = (head.line + 1..body_end)
            .any(|i| !p.fenced[i] && !p.lines[i].trim().is_empty() && !p.lines[i].starts_with("Status:"));
        let line = head.line + 1;
        if head.status.is_some() && inherited {
            c.err(1, &p.path, line, "double status: this section and an ancestor both have one".into());
        } else if claim && !exempt(h) && !inherited && head.status.is_none() {
            c.err(1, &p.path, line, format!("no status line covers `{}`", head.text));
        }
        if let Some(Status::Planned(_)) = head.status {
            let opens = (head.line + 1..p.section_end(h)).filter(|&i| is_open(p, i)).count();
            if opens != 1 {
                c.err(1, &p.path, line, format!("planned section has {opens} `**Open:**` lines, not 1"));
            }
        }
    }
    for i in (0..p.lines.len()).filter(|&i| is_open(p, i)) {
        if !matches!(p.head_of(i).and_then(|h| p.cover(h)), Some(Status::Planned(_))) {
            c.err(1, &p.path, i + 1, "`**Open:**` outside a planned section".into());
        }
    }
}

fn is_open(p: &Page, i: usize) -> bool { !p.fenced[i] && p.lines[i].trim_start().starts_with("**Open:**") }

fn test_exists(c: &mut Ctx, t: &str) -> bool {
    let crates = c.crates.get_or_insert_with(|| crates(c.root));
    match t.split_once(':') {
        Some(("bench", case)) => c.root.join(format!("tests/{case}.toml")).is_file(),
        Some(("host", r)) => {
            r.split_once("::").is_some_and(|(p, f)| crates.get(p).is_some_and(|(_, tests)| tests.contains(f)))
        }
        Some(("mutation", v)) => mutations(c.root).contains(v),
        Some(("fuzz", r)) => r.split_once('/').is_some_and(|(p, f)| {
            crates
                .get(p)
                .is_some_and(|(dir, _)| c.root.join(format!("{dir}/fuzz/fuzz_targets/{f}.rs")).is_file())
        }),
        _ => false,
    }
}

/// The crates C2 searches: the workspace members, `model` and `userland/otp` (and the members
/// of any of those that is itself a workspace), each with its `#[test]` function names.
fn crates(root: &Path) -> BTreeMap<String, (String, BTreeSet<String>)> {
    let manifest = |dir: &str| fs::read_to_string(root.join(dir).join("Cargo.toml")).unwrap_or_default();
    let mut dirs: Vec<String> = toml_list(&manifest("."), "members");
    dirs.extend(["model".to_string(), "userland/otp".to_string()]);
    let mut out = BTreeMap::new();
    let mut seen = BTreeSet::new();
    while let Some(dir) = dirs.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let text = manifest(&dir);
        if text.contains("[workspace]") && dir != "." {
            dirs.extend(toml_list(&text, "members").into_iter().map(|m| format!("{dir}/{m}")));
        }
        if let Some(name) = package_name(&text) {
            let mut files = Vec::new();
            walk(root, &dir, &|p| p.ends_with("/fuzz") || p.ends_with("/target"), &mut files);
            let mut tests = BTreeSet::new();
            for f in files.iter().filter(|f| f.ends_with(".rs")) {
                let src = fs::read_to_string(root.join(f)).unwrap_or_default();
                let lines: Vec<&str> = src.lines().collect();
                for (i, _) in lines.iter().enumerate().filter(|(_, l)| l.trim() == "#[test]") {
                    for l in lines.iter().skip(i + 1).take(3) {
                        if let Some(at) = l.find("fn ") {
                            let rest = &l[at + 3..];
                            if let Some(end) = rest.find('(') {
                                tests.insert(rest[..end].to_string());
                            }
                        }
                    }
                }
            }
            out.insert(name, (dir, tests));
        }
    }
    out
}

/// The quoted strings of `key = [ ... ]` in a manifest.
fn toml_list(text: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let t = line.trim_start();
        if !inside && t.starts_with(key) && t[key.len()..].trim_start().starts_with('=') {
            inside = true;
        }
        if inside {
            out.extend(line.split('"').skip(1).step_by(2).map(str::to_string));
            if line.contains(']') {
                break;
            }
        }
    }
    out
}

fn package_name(text: &str) -> Option<String> {
    let pkg = &text[text.find("[package]")? + 9..];
    let pkg = &pkg[..pkg.find("\n[").unwrap_or(pkg.len())];
    let line = pkg.lines().find(|l| l.trim_start().starts_with("name") && l.contains('='))?;
    line.split('"').nth(1).map(str::to_string)
}

/// The variants of `pub enum Mutation` in `model/src/mutation.rs`.
fn mutations(root: &Path) -> BTreeSet<String> {
    let src = fs::read_to_string(root.join("model/src/mutation.rs")).unwrap_or_default();
    let mut out = BTreeSet::new();
    let Some(start) = src.find("pub enum Mutation") else { return out };
    let mut depth = 0;
    for line in src[start..].lines() {
        let t = line.trim();
        if depth == 1 && t.starts_with(|c: char| c.is_ascii_uppercase()) {
            out.insert(t.split(|c: char| !is_word(c)).next().unwrap_or("").to_string());
        }
        depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
        if depth == 0 && t.contains('}') {
            break;
        }
    }
    out
}

// ---- C3, C4: milestones and process references ----

fn milestones_and_process(c: &mut Ctx, p: &Page) {
    let process = p.path != "docs/SWARM.md" && p.path != "docs/PROJECT.md";
    for (i, l) in p.lines.iter().enumerate() {
        for (at, w) in words(l) {
            let Some(m) = MILESTONES.iter().position(|(id, _)| *id == w) else { continue };
            let before = &l[..at];
            let beyond = before.ends_with("beyond ") || before.ends_with("Beyond ");
            if !beyond && !l[at + 2..].starts_with(&format!(" ({})", MILESTONES[m].1)) {
                c.err(3, &p.path, i + 1, format!("`{w}` must read `{}`", milestone(m)));
            }
        }
        if process {
            for m in process_refs(l, !p.fenced[i], p.path == "docs/testbench.md") {
                c.err(4, &p.path, i + 1, m);
            }
        }
    }
}

// ---- C5: rule IDs ----

struct Def {
    name: String,
    path: String,
    head: usize,
}

/// `### <ID> (<name>)` where C5 allows a definition; C5(a) duplicates.
fn definitions(c: &mut Ctx, pages: &[Page]) -> BTreeMap<String, Def> {
    let mut defs: BTreeMap<String, Def> = BTreeMap::new();
    for p in pages {
        let anywhere = p.path == "docs/kernel/invariants.md" || p.path == "docs/testbench.md";
        let props = p.path.starts_with("docs/kernel/") || p.path.starts_with("docs/servers/");
        for (h, head) in p.heads.iter().enumerate() {
            let in_props = || {
                let up = head.parent.map(|u| &p.heads[u]);
                up.is_some_and(|u| u.level == 2 && u.text == "Security properties")
            };
            let Some((id, name)) = def_heading(head) else { continue };
            if !(anywhere || (props && in_props())) {
                continue;
            }
            if let Some(d) = defs.get(&id) {
                let msg = format!("{id} is defined twice (also in {})", d.path);
                c.err(5, &p.path, head.line + 1, msg);
                continue;
            }
            defs.insert(id, Def { name, path: p.path.clone(), head: h });
        }
    }
    defs
}

fn def_heading(head: &Head) -> Option<(String, String)> {
    let (id, name) = head.text.split_once(" (")?;
    let name = name.strip_suffix(')')?;
    (head.level == 3 && (is_rule_id(id) || id == "Rule F")).then(|| (id.to_string(), name.to_string()))
}

/// C5(b) and (c).
fn citations(c: &mut Ctx, p: &Page, defs: &BTreeMap<String, Def>) {
    let own: BTreeSet<usize> = p.heads.iter().filter(|h| def_heading(h).is_some()).map(|h| h.line).collect();
    let mut seen = BTreeSet::new();
    for (i, l) in p.lines.iter().enumerate().filter(|(i, _)| !own.contains(i)) {
        let s = strip_links(l);
        for (at, w) in words(&s).into_iter().filter(|(_, w)| is_rule_id(w)) {
            let Some(d) = defs.get(w) else {
                c.err(5, &p.path, i + 1, format!("{w} is not defined"));
                continue;
            };
            if d.name == "withdrawn" {
                c.err(5, &p.path, i + 1, format!("{w} is withdrawn and may not be cited"));
            } else if d.path != p.path
                && seen.insert(w.to_string())
                && !s[at + w.len()..].starts_with(&format!(" ({})", d.name))
            {
                c.err(5, &p.path, i + 1, format!("first citation must read `{w} ({})`", d.name));
            }
        }
    }
}

// ---- C6: links ----

fn links(c: &mut Ctx, p: &Page) {
    for (i, l) in p.lines.iter().enumerate().filter(|(i, _)| !p.fenced[*i]) {
        for t in link_targets(l) {
            if let Some(m) = bad_link(c.root, &p.path, &t) {
                c.err(6, &p.path, i + 1, format!("link `{t}`: {m}"));
            }
        }
    }
}

fn bad_link(root: &Path, from: &str, t: &str) -> Option<String> {
    if is_url(t) {
        return None;
    }
    let (path, anchor) = t.split_once('#').map_or((t, None), |(p, a)| (p, Some(a)));
    let target = if path.is_empty() { Some(from.to_string()) } else { resolve(from, path) };
    let Some(target) = target else { return Some("leaves the repository".into()) };
    if ["docs/legacy", "docs/inventory"].iter().any(|x| target == *x || target.starts_with(&format!("{x}/")))
    {
        return Some("links into an excluded directory".into());
    }
    if !root.join(&target).exists() {
        return Some("no such file".into());
    }
    match (anchor, target.ends_with(".md")) {
        (Some(a), true) if !Page::load(root, &target).is_some_and(|p| p.slugs().contains(a)) => {
            Some(format!("no heading `#{a}`"))
        }
        _ => None,
    }
}

// ---- C7: SECURITY.md ----

const REGISTER: [&str; 6] = ["Property", "Rule", "Enforced in", "Tested by", "Status", "Residual risks"];

fn cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|x| x.trim().to_string()).collect()
}

fn security(c: &mut Ctx, pages: &[Page], defs: &BTreeMap<String, Def>) {
    const PATH: &str = "docs/SECURITY.md";
    if !c.exists("docs") {
        return;
    }
    let Some(sec) = pages.iter().find(|p| p.path == PATH) else {
        return c.err(7, PATH, 1, "docs/SECURITY.md is missing".into());
    };
    let headers: Vec<usize> = (0..sec.lines.len()).filter(|&i| cells(&sec.lines[i]) == REGISTER).collect();
    if headers.len() != 1 {
        return c.err(7, PATH, 1, format!("{} register tables, not 1", headers.len()));
    }
    let mut rows = BTreeSet::new();
    for i in headers[0] + 1..sec.lines.len() {
        let row = cells(&sec.lines[i]);
        if !sec.lines[i].trim_start().starts_with('|') {
            break;
        }
        if row.iter().all(|x| x.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
            continue;
        }
        if row.len() != 6 {
            c.err(7, PATH, i + 1, "register row does not have 6 cells".into());
            continue;
        }
        for (_, id) in words(&strip_links(&row[1])).into_iter().filter(|(_, w)| is_rule_id(w)) {
            rows.insert(id.to_string());
            let Some(d) = defs.get(id) else {
                c.err(7, PATH, i + 1, format!("{id} is not defined"));
                continue;
            };
            let page = pages.iter().find(|p| p.path == d.path).expect("defined on a loaded page");
            let Some(st) = page.cover(d.head) else {
                c.err(7, PATH, i + 1, format!("{id} has no status on its page"));
                continue;
            };
            let cell = &row[4];
            let valid = cell == "built"
                || cell == "built, partly tested"
                || (0..MILESTONES.len()).any(|m| *cell == format!("planned · {}", milestone(m)));
            if !valid {
                c.err(7, PATH, i + 1, format!("malformed status `{cell}`"));
            } else if *cell != st.cell() {
                c.err(7, PATH, i + 1, format!("status `{cell}` disagrees with {id}'s `{}`", st.cell()));
            }
            let listed: BTreeSet<String> = row[3]
                .split(',')
                .map(|t| t.trim().trim_matches('`').to_string())
                .filter(|t| !matches!(t.as_str(), "" | "-" | "—"))
                .collect();
            if listed != st.tests().iter().cloned().collect() {
                c.err(7, PATH, i + 1, format!("Tested by differs from {id}'s status line"));
            }
        }
        for path in row[2].split('`').skip(1).step_by(2) {
            let path = path.split(':').next().unwrap_or("");
            if !c.exists(path) {
                c.err(7, PATH, i + 1, format!("`{path}` does not exist"));
            }
        }
    }
    for id in defs.iter().filter(|(k, d)| *k != "Rule F" && d.name != "withdrawn").map(|(k, _)| k) {
        if !rows.contains(id) {
            c.err(7, PATH, headers[0] + 1, format!("{id} has no register row"));
        }
    }
}

// ---- C8: no binaries ----

fn no_binaries(c: &mut Ctx) {
    let mut files = Vec::new();
    walk(c.root, "docs", &|p| p == "docs/legacy" || p == "docs/inventory", &mut files);
    for f in files {
        let ext = f.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_lowercase());
        let theme_js = f.strip_prefix("docs/theme/").is_some_and(|r| !r.contains('/') && ext == "js");
        if BINARY.contains(&ext.as_str()) {
            c.err(8, &f, 1, "image or binary file under docs/".into());
        } else if !(ext == "md" || f == "docs/book.toml" || theme_js) {
            c.err(8, &f, 1, "only Markdown, docs/book.toml and docs/theme/*.js belong under docs/".into());
        }
        let bytes = fs::read(c.root.join(&f)).unwrap_or_default();
        if bytes.iter().take(8192).any(|&b| b == 0) {
            c.err(8, &f, 1, "NUL byte: a binary file".into());
        }
    }
}

// ---- C9: templates ----

fn template(c: &mut Ctx, p: &Page) {
    const KERNEL: &[&str] = &[
        "Purpose",
        "Interface",
        "Authority",
        "Security properties",
        "Failure and restart",
        "Residual risks",
        "Why",
    ];
    const REFERENCE: [&str; 4] = ["memory-layout.md", "abi.md", "invariants.md", "model.md"];
    let Some((set, file)) = p.path.strip_prefix("docs/").and_then(|r| r.split_once('/')) else { return };
    if file.contains('/') || file == "README.md" || (set == "kernel" && REFERENCE.contains(&file)) {
        return;
    }
    let want: &[&str] = match set {
        "kernel" | "servers" => KERNEL,
        "userland" => &["Purpose", "How to use it", "What it can and cannot do", "Why"],
        "plan" => &["Goal", "Attack suite", "Remaining work", "Progress"],
        "todo" => &["What", "Why it matters", "Where", "Done when"],
        "beyond" => &["Idea", "Why it is not a goal", "What it would need"],
        _ => return,
    };
    let have: Vec<&str> = p.heads.iter().filter(|h| h.level == 2).map(|h| h.text.as_str()).collect();
    if have != want {
        c.err(9, &p.path, 1, format!("`##` headings must be exactly: {}", want.join(", ")));
    }
}

// ---- C10: wire tables ----

fn wire_tables(c: &mut Ctx, pages: &[Page]) {
    let mut tables = Vec::new();
    walk(c.root, "libs/wire/tables", &|_| true, &mut tables);
    tables.retain(|t| t.ends_with(".md") && !t.ends_with("/example.md"));
    let mut uses: BTreeMap<String, usize> = BTreeMap::new();
    for p in pages {
        for (i, l) in p.lines.iter().enumerate() {
            let Some(inc) = l.split("{{#include ").nth(1).and_then(|r| r.split("}}").next()) else {
                continue;
            };
            let Some(target) = resolve(&p.path, inc.trim().split(':').next().unwrap_or("")) else { continue };
            if !tables.contains(&target) {
                continue;
            }
            *uses.entry(target.clone()).or_default() += 1;
            let prev = p.lines[..i].iter().rev().find(|l| !l.trim().is_empty());
            let linked = prev.is_some_and(|l| {
                link_targets(l).iter().any(|t| resolve(&p.path, t).as_ref() == Some(&target))
            });
            if !linked {
                c.err(10, &p.path, i + 1, format!("include of `{target}` is not preceded by a link to it"));
            }
            if uses[&target] > 1 {
                c.err(10, &p.path, i + 1, format!("`{target}` is included by more than one page"));
            }
        }
    }
    for t in tables.iter().filter(|t| !uses.contains_key(*t)) {
        c.err(10, t, 1, "wire table is not included by any page".into());
    }
}

// ---- C11: code comments and case descriptions ----

fn code(c: &mut Ctx, pages: &[Page], defs: &BTreeMap<String, Def>) {
    const SKIP: [&str; 3] = ["vendor", "bios", "userland/otp"];
    let current: BTreeSet<&str> = pages.iter().map(|p| p.path.rsplit('/').next().unwrap_or("")).collect();
    let mut legacy = Vec::new();
    walk(c.root, "docs/legacy", &|_| true, &mut legacy);
    let legacy: Vec<String> = legacy
        .iter()
        .filter_map(|p| p.rsplit('/').next())
        .filter(|n| n.ends_with(".md") && !current.contains(n))
        .map(str::to_string)
        .collect();
    let mut files = Vec::new();
    let skip = |p: &str| {
        let name = p.rsplit('/').next().unwrap_or("");
        name.starts_with('.') || name == "target" || SKIP.contains(&p) || p == "tools/doccheck"
    };
    for top in fs::read_dir(c.root).into_iter().flatten().flatten() {
        let name = top.file_name().to_string_lossy().to_string();
        if top.path().is_dir() && !skip(&name) {
            walk(c.root, &name, &skip, &mut files);
        }
    }
    for f in files {
        let case = f.starts_with("tests/") && f.ends_with(".toml") && !f[6..].contains('/');
        if !case && !f.ends_with(".rs") {
            continue;
        }
        let src = fs::read_to_string(c.root.join(&f)).unwrap_or_default();
        for (i, l) in src.lines().enumerate() {
            let text = if case {
                l.trim_start().strip_prefix("description").and_then(|r| r.trim_start().strip_prefix('='))
            } else {
                l.find("//").map(|at| &l[at + 2..])
            };
            let Some(text) = text else { continue };
            let mut msgs = process_refs(text, true, false);
            msgs.extend(
                legacy.iter().filter(|n| text.contains(n.as_str())).map(|n| format!("legacy doc name `{n}`")),
            );
            if text.contains("docs/legacy") {
                msgs.push("names docs/legacy".into());
            }
            for (_, w) in words(text).into_iter().filter(|(_, w)| is_rule_id(w)) {
                match defs.get(w) {
                    None => msgs.push(format!("{w} is not defined")),
                    Some(d) if d.name == "withdrawn" => msgs.push(format!("{w} is withdrawn")),
                    _ => {}
                }
            }
            for m in msgs {
                c.err(11, &f, i + 1, m);
            }
        }
    }
}

// ---- C12: SUMMARY.md ----

fn summary(c: &mut Ctx, pages: &[Page]) {
    const PATH: &str = "docs/SUMMARY.md";
    if !c.exists("docs") {
        return;
    }
    let Some(sum) = pages.iter().find(|p| p.path == PATH) else {
        return c.err(12, PATH, 1, "docs/SUMMARY.md is missing".into());
    };
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for (i, l) in sum.lines.iter().enumerate().filter(|(i, _)| !sum.fenced[*i]) {
        for t in link_targets(l).iter().filter(|t| !is_url(t)) {
            let target = resolve(PATH, t.split('#').next().unwrap_or("")).unwrap_or_default();
            if !c.exists(&target) || target.is_empty() {
                c.err(12, PATH, i + 1, format!("link `{t}`: no such page"));
            }
            *count.entry(target.clone()).or_default() += 1;
            if count[&target] == 2 {
                c.err(12, PATH, i + 1, format!("`{target}` is linked more than once"));
            }
        }
    }
    for p in pages.iter().filter(|p| p.path.starts_with("docs/") && p.path != PATH) {
        if !count.contains_key(&p.path) {
            c.err(12, &p.path, 1, "page is not linked from docs/SUMMARY.md".into());
        }
    }
}
