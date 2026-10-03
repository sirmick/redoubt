//! The size budget: each trusted crate's lines of Rust against a ceiling that only falls
//! (tenet 1: the trusted computing base is budgeted, not observed).
//!
//! A line counts when it holds code: blank lines, `//` comments (doc comments included) and lines
//! wholly inside a `/* */` comment do not. Every `.rs` file under a crate's paths counts, the same
//! way every time, except its tests: a `#[cfg(test)]` item counts for nothing, and a file only test
//! modules reach counts for nothing either. A file any other module declaration reaches counts,
//! whatever else names it, and a module file the case cannot find fails it, as does a form it
//! does not follow (`mod r#name;`, a macro's `mod $name;`, the word `include`).
//!
//! Raising a ceiling needs a reason in the commit that does it. The case reads the branch's own
//! commits that changed the case file, merges included; where one raised a ceiling over the file
//! in its first parent, dropped a crate (a rename drops the old name) or narrowed a crate's paths,
//! a line `Size budget: <crate>: <reason>` must name each such crate, in its message or, for a
//! merge, in a commit it brings in. A raise not yet committed fails outright. The unsafe budget
//! shares this history check (`ratchet`), with its own prefix.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::de::DeserializeOwned;

use crate::case::{SizeBudget, SizeCrate};

/// An out-of-line module a file declares (`mod name;`), with the paths its `#[path]` and
/// `#[cfg_attr(..., path = ...)]` attributes give.
struct Module {
    name: String,
    paths: Vec<String>,
    /// A path is given under `cfg_attr`, so the default file may be the one built instead.
    conditional: bool,
    /// The inline modules (`mod outer { ... }`) it is declared in, outermost first.
    inline: Vec<String>,
    /// Declared inside a `#[cfg(test)]` item.
    test: bool,
}

/// One file's text: its lines of code outside `#[cfg(test)]` items, and its out-of-line modules.
struct Scan {
    lines: usize,
    modules: Vec<Module>,
}

#[derive(Clone, Copy, PartialEq)]
enum Lex {
    Code,
    LineComment,
    /// Inside `/* */`, nested this deep.
    Block(usize),
    Str,
    /// Inside a raw string closed by `"` and this many `#`.
    Raw(usize),
}

/// Where the lexer is in a `#[cfg(test)]` item.
#[derive(Clone, Copy)]
enum Skip {
    /// Inside it, this many brackets deep.
    Item(usize),
    /// Just past its closing brace: a `;` straight after still belongs to it.
    Closed,
}

/// Whether a line holds code: it is not blank, a `//` comment or wholly inside a `/* */` comment.
/// The rule is textual, so an assembly comment in a `global_asm!` string is a comment too.
fn code_line(line: &str, in_block: &mut bool) -> bool {
    let mut rest = line.trim();
    while !rest.is_empty() {
        if *in_block {
            let Some(at) = rest.find("*/") else { break };
            *in_block = false;
            rest = rest[at + 2..].trim_start();
        } else if rest.starts_with("//") {
            break;
        } else if let Some(after) = rest.strip_prefix("/*") {
            *in_block = true;
            rest = after;
        } else {
            return true;
        }
    }
    false
}

/// Lexes the text to find which lines belong to `#[cfg(test)]` items alone, and the modules it
/// declares: strings, characters and comments are skipped, so the brackets that end an item and
/// the `mod`s found are the code's own.
fn scan(text: &str) -> Result<Scan> {
    let chars: Vec<char> = text.chars().collect();
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, c)| chars.get(i + k) == Some(&c));
    // Per line: whether it holds anything outside a test item, and whether it holds any inside one.
    let mut flags = Vec::new();
    let (mut outside, mut test) = (false, false);
    let mut lex = Lex::Code;
    let mut skip = None;
    // Modules: the paths waiting for their `mod` (and whether one is conditional), the inline
    // modules open (each with the brace depth outside it), and the name of one whose `{` is next.
    let (mut modules, mut paths, mut conditional) = (Vec::new(), Vec::new(), false);
    let (mut inline, mut opening) = (Vec::new(), None);
    let mut braces = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            // A line that ends inside a test item is the item's, whatever it holds.
            flags.push((outside, test || skip.is_some()));
            (outside, test) = (false, false);
            if lex == Lex::LineComment {
                lex = Lex::Code;
            }
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        match lex {
            Lex::LineComment => continue,
            Lex::Block(depth) => {
                if at(start, "*/") {
                    lex = if depth == 1 { Lex::Code } else { Lex::Block(depth - 1) };
                    i += 1;
                } else if at(start, "/*") {
                    lex = Lex::Block(depth + 1);
                    i += 1;
                }
                continue;
            }
            Lex::Str => {
                if c == '\\' && chars.get(i) != Some(&'\n') {
                    i += 1;
                } else if c == '"' {
                    lex = Lex::Code;
                }
            }
            Lex::Raw(hashes) => {
                if c == '"' && (0..hashes).all(|k| chars.get(i + k) == Some(&'#')) {
                    lex = Lex::Code;
                    i += hashes;
                }
            }
            Lex::Code if c.is_whitespace() => continue,
            Lex::Code if at(start, "//") => {
                lex = Lex::LineComment;
                continue;
            }
            Lex::Code if at(start, "/*") => {
                lex = Lex::Block(1);
                i += 1;
                continue;
            }
            Lex::Code => {
                match c {
                    '{' => {
                        if let Some(name) = opening.take() {
                            inline.push((name, braces));
                        }
                        braces += 1;
                        (paths, conditional) = (Vec::new(), false);
                    }
                    '}' => {
                        braces = braces.saturating_sub(1);
                        if inline.last().is_some_and(|&(_, depth)| depth == braces) {
                            inline.pop();
                        }
                        (paths, conditional) = (Vec::new(), false);
                    }
                    ';' => (paths, conditional) = (Vec::new(), false),
                    _ => {}
                }
                if let Some(Skip::Closed) = skip {
                    skip = None;
                    if c == ';' {
                        test = true;
                        continue;
                    }
                }
                if skip.is_none() && at(start, "#[cfg(test)]") {
                    skip = Some(Skip::Item(0));
                    i = start + "#[cfg(test)]".len();
                } else if at(start, "#[path") || at(start, "#[cfg_attr(") {
                    // Read ahead for its path; the attribute is then lexed as any code is.
                    let given = path_attribute(&chars, start).with_context(|| {
                        format!("line {}: an attribute the case cannot read", flags.len() + 1)
                    })?;
                    if let Some(given) = given {
                        paths.push(given);
                        conditional |= at(start, "#[cfg_attr(");
                    }
                } else if c == '"' {
                    lex = Lex::Str;
                } else if c == '\'' {
                    // A character literal, or else a lifetime or label.
                    if chars.get(i) == Some(&'\\') {
                        i += 2;
                        while chars.get(i).is_some_and(|&c| c != '\'') {
                            i += 1;
                        }
                        i += 1;
                    } else if chars.get(i + 1) == Some(&'\'') {
                        i += 2;
                    }
                } else if c.is_alphabetic() || c == '_' {
                    while chars.get(i).is_some_and(|&c| c.is_alphanumeric() || c == '_') {
                        i += 1;
                    }
                    let word: String = chars[start..i].iter().collect();
                    let line = flags.len() + 1;
                    // A file reached by a form the case does not follow could hide behind a test's
                    // `#[path]` to it, so such a form fails the case.
                    ensure!(
                        word != "include",
                        "line {line}: `include` is not followed, however it is invoked"
                    );
                    if word == "mod" {
                        let names = inline.iter().map(|(name, _)| String::clone(name)).collect();
                        let (name, next) = module_after(&chars, i).with_context(|| {
                            format!("line {line}: a `mod` not followed by a plain name and `;` or `{{`")
                        })?;
                        if next == ';' {
                            let (test, paths) = (skip.is_some(), std::mem::take(&mut paths));
                            modules.push(Module { name, paths, conditional, inline: names, test });
                        } else {
                            ensure!(
                                paths.is_empty(),
                                "line {line}: a #[path] on the inline module {name} is not resolved"
                            );
                            opening = Some(name);
                        }
                    } else if word == "r"
                        && chars.get(i) == Some(&'#')
                        && chars.get(i + 1).is_some_and(|&c| c.is_alphabetic() || c == '_')
                    {
                        // A raw identifier (`r#mod`) is a name, not the keyword; `r#include` is
                        // still `include`.
                        i += 1;
                        let name = i;
                        while chars.get(i).is_some_and(|&c| c.is_alphanumeric() || c == '_') {
                            i += 1;
                        }
                        ensure!(
                            chars[name..i].iter().collect::<String>() != "include",
                            "line {line}: `include` is not followed, however it is invoked"
                        );
                    }
                    let hashes = chars[i..].iter().take_while(|&&c| c == '#').count();
                    if matches!(word.as_str(), "r" | "br" | "cr") && chars.get(i + hashes) == Some(&'"') {
                        lex = Lex::Raw(hashes);
                        i += hashes + 1;
                    } else if matches!(word.as_str(), "b" | "c") && chars.get(i) == Some(&'"') {
                        lex = Lex::Str;
                        i += 1;
                    }
                } else if let Some(Skip::Item(depth)) = skip {
                    // The item ends at a `;` or `,` outside its brackets or after its closing brace.
                    match c {
                        '(' | '[' | '{' => skip = Some(Skip::Item(depth + 1)),
                        '}' if depth == 1 => skip = Some(Skip::Closed),
                        ')' | ']' | '}' if depth > 0 => skip = Some(Skip::Item(depth - 1)),
                        // A bracket closing what holds the item ends it too, and is not the item's.
                        ')' | ']' | '}' => {
                            skip = None;
                            outside = true;
                            continue;
                        }
                        ';' | ',' if depth == 0 => {
                            skip = None;
                            test = true;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
        }
        if skip.is_some() {
            test = true;
        } else {
            outside = true;
        }
    }
    if !chars.is_empty() && chars.last() != Some(&'\n') {
        flags.push((outside, test || skip.is_some()));
    }

    let (mut lines, mut in_block) = (0, false);
    for (line, &(outside, test)) in text.lines().zip(&flags) {
        lines += usize::from(code_line(line, &mut in_block) && (outside || !test));
    }
    Ok(Scan { lines, modules })
}

/// The path an attribute starting at `start` gives, `#[path = "..."]` or `#[cfg_attr(..., path =
/// "...")]`, if any. `None` if the attribute cannot be read.
fn path_attribute(chars: &[char], start: usize) -> Option<Option<String>> {
    let skip_space = |mut i: usize| {
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        i
    };
    // `path = "..."` at `i`: the path and where it ends.
    let path_at = |i: usize| -> Option<(String, usize)> {
        let mut i = skip_space(i + "path".len());
        (chars.get(i) == Some(&'=')).then_some(())?;
        i = skip_space(i + 1);
        (chars.get(i) == Some(&'"')).then_some(())?;
        let end = i + 1 + chars[i + 1..].iter().position(|&c| c == '"' || c == '\\')?;
        (chars[end] == '"').then_some((chars[i + 1..end].iter().collect(), end + 1))
    };
    let ident = |c: Option<&char>| c.is_some_and(|&c| c.is_alphanumeric() || c == '_');
    let (mut given, mut i, mut depth) = (None, start + 2, 0);
    loop {
        match *chars.get(i)? {
            '"' => i += 1 + chars[i + 1..].iter().position(|&c| c == '"')? + 1,
            '(' | '[' => (depth, i) = (depth + 1, i + 1),
            ']' if depth == 0 => return Some(given),
            ')' | ']' => (depth, i) = (depth - 1, i + 1),
            // The word `path`; a second one in one attribute is not read.
            _ if chars[i..].starts_with(&['p', 'a', 't', 'h'])
                && !ident(chars.get(i + 4))
                && !ident(i.checked_sub(1).and_then(|k| chars.get(k))) =>
            {
                let (path, end) = path_at(i).filter(|_| given.is_none())?;
                (given, i) = (Some(path), end);
            }
            _ => i += 1,
        }
    }
}

/// After the word `mod` at `i`: the module's name, a plain identifier (not `r#` or a macro's
/// `$`), and what follows it, `;` or `{`.
fn module_after(chars: &[char], mut i: usize) -> Option<(String, char)> {
    while chars.get(i).is_some_and(|c| c.is_whitespace()) {
        i += 1;
    }
    let start = i;
    chars.get(i).is_some_and(|&c| c.is_alphabetic() || c == '_').then_some(())?;
    while chars.get(i).is_some_and(|&c| c.is_alphanumeric() || c == '_') {
        i += 1;
    }
    let name: String = chars[start..i].iter().collect();
    while chars.get(i).is_some_and(|c| c.is_whitespace()) {
        i += 1;
    }
    let next = *chars.get(i)?;
    (!name.is_empty() && (next == ';' || next == '{')).then_some((name, next))
}

/// `path` with its `.` and `..` components resolved, as the files listed under a crate are named.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if out.file_name().is_some() => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}

/// The files a module declared in `file` may live in, each with whether it must exist: every path
/// its attributes give, and its default file unless a path is given unconditionally.
/// `owns_directory` says whether `file` is a crate root, a `mod.rs` or a file a path names: then
/// its modules live beside it, not in a directory named after it.
fn module_files(
    file: &Path,
    module: &Module,
    owns_directory: bool,
    files: &HashMap<PathBuf, Scan>,
) -> Vec<(PathBuf, bool)> {
    let dir = file.parent().unwrap_or(Path::new(""));
    let mut found: Vec<_> = module.paths.iter().map(|path| (normalize(&dir.join(path)), true)).collect();
    if module.paths.is_empty() || module.conditional {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let mut base = if owns_directory { dir.to_path_buf() } else { dir.join(stem) };
        base.extend(&module.inline);
        let flat = base.join(format!("{}.rs", module.name));
        let default = if files.contains_key(&flat) { flat } else { base.join(&module.name).join("mod.rs") };
        found.push((default, module.paths.is_empty()));
    }
    found
}

/// Lines of code under `path`, leaving out `#[cfg(test)]` items and every file only test modules
/// reach: a file any other module declaration reaches counts, from a file that counts.
fn count_path(path: &Path) -> Result<usize> {
    let files = crate::budget::rust_files(path)?;
    ensure!(!files.is_empty(), "no Rust source files in {}", path.display());
    let mut scans = HashMap::new();
    for file in &files {
        let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        scans.insert(file.clone(), scan(&text).with_context(|| format!("scanning {}", file.display()))?);
    }
    let named: HashSet<PathBuf> = scans
        .iter()
        .flat_map(|(file, s)| {
            s.modules.iter().flat_map(|m| &m.paths).filter_map(move |path| Some(file.parent()?.join(path)))
        })
        .map(|p| normalize(&p))
        .collect();
    // Each file's module declarations: the file each lives in, and whether it is a test's.
    let mut declared: HashMap<&PathBuf, Vec<(PathBuf, bool)>> = HashMap::new();
    for (file, s) in &scans {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let owns_directory = matches!(stem, "lib" | "main" | "mod") || named.contains(file);
        for m in &s.modules {
            for (to, required) in module_files(file, m, owns_directory, &scans) {
                ensure!(
                    !required || scans.contains_key(&to),
                    "{}: module {}'s file {} is not under {}",
                    file.display(),
                    m.name,
                    to.display(),
                    path.display()
                );
                if scans.contains_key(&to) {
                    declared.entry(file).or_default().push((to, m.test));
                }
            }
        }
    }
    let reach = |from: Vec<PathBuf>, tests_too: bool| {
        let (mut seen, mut pending) = (HashSet::new(), from);
        while let Some(file) = pending.pop() {
            if seen.insert(file.clone()) {
                let modules = declared.get(&file).into_iter().flatten();
                pending.extend(modules.filter(|(_, test)| tests_too || !test).map(|(to, _)| to.clone()));
            }
        }
        seen
    };
    // What test modules reach, and what any other declaration reaches from a file outside that.
    let tests = reach(
        declared.values().flatten().filter(|(_, test)| *test).map(|(to, _)| to.clone()).collect(),
        true,
    );
    let counted = reach(scans.keys().filter(|f| !tests.contains(*f)).cloned().collect(), false);
    Ok(scans.iter().filter(|(file, _)| counted.contains(*file)).map(|(_, s)| s.lines).sum())
}

/// One entry of a budget file whose limits only fall: a trusted crate's ceiling, a directory's
/// `unsafe`. The history check is the same for every such file.
pub trait Limit: DeserializeOwned {
    /// The array of tables the entries are listed in (`[[crate]]`, `[[budget]]`).
    const TABLE: &'static str;
    fn name(&self) -> &str;
    fn paths(&self) -> &[String];
    /// Each limit, with its key.
    fn limits(&self) -> Vec<(&'static str, usize)>;
}

impl Limit for SizeCrate {
    const TABLE: &'static str = "crate";

    fn name(&self) -> &str { &self.name }

    fn paths(&self) -> &[String] { &self.paths }

    fn limits(&self) -> Vec<(&'static str, usize)> { vec![("max_lines", self.max_lines)] }
}

/// The entries in one version of a budget file.
fn entries<T: Limit>(text: &str) -> Result<Vec<T>> {
    let mut file: toml::Table = toml::from_str(text).context("parsing the budget file")?;
    let list = file.remove(T::TABLE).with_context(|| format!("no [[{}]] in the budget file", T::TABLE))?;
    list.try_into().context("parsing the budget file")
}

/// The entries `now` raises a limit of over `before`, drops, or counts fewer paths of, each with
/// what changed.
fn raised<T: Limit>(before: &[T], now: &[T]) -> Vec<(String, String)> {
    before
        .iter()
        .filter_map(|old| {
            let name = old.name().to_string();
            let Some(new) = now.iter().find(|n| n.name() == old.name()) else {
                let limits: Vec<_> = old.limits().iter().map(|(key, was)| format!("{key} = {was}")).collect();
                return Some((name, format!("dropped ({})", limits.join(", "))));
            };
            let mut limits = old.limits().into_iter().zip(new.limits());
            if let Some(((key, was), (_, is))) = limits.find(|((_, was), (_, is))| is > was) {
                return Some((name, format!("{key} raised from {was} to {is}")));
            }
            let gone: Vec<_> =
                old.paths().iter().filter(|p| !new.paths().contains(p)).map(String::as_str).collect();
            (!gone.is_empty())
                .then(|| (name, format!("paths narrowed (no longer counts {})", gone.join(", "))))
        })
        .collect()
}

/// Whether a commit message gives `name` a reason, a line `<prefix>: <name>: <reason>`. The name is
/// matched whole, so a name that itself holds `: ` (`kernel: core`) is still found.
fn explains(message: &str, prefix: &str, name: &str) -> bool {
    message.lines().any(|l| {
        let why = l
            .trim()
            .strip_prefix(prefix)
            .and_then(|l| l.strip_prefix(": ")?.strip_prefix(name)?.strip_prefix(": "));
        why.is_some_and(|why| !why.trim().is_empty())
    })
}

fn git(workspace: &Path, args: &[&str]) -> Result<Option<String>> {
    let out = Command::new("git").current_dir(workspace).args(args).output().context("running git")?;
    Ok(out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// The branch a package branch is judged against: the local main branch, or else, in a fresh
/// clone, the remote's default branch.
const MAIN_BRANCHES: [&str; 2] = ["redoubt", "origin/HEAD"];

/// Whether a budget file's limits only fell, or each raise, drop and narrowing carries a line
/// `<prefix>: <name>: <reason>`: in the commit that made it or, for a merge judged against its
/// first parent, in a commit the merge brings in. A branch is judged on its own commits, those
/// since its merge-base with the main branch, so merged history is never read again; on the main
/// branch itself only the file as it stands is. A commit whose parent lacks the file is judged
/// against the last version before it, the main branch's at the start, and where there is none,
/// as a new or renamed file, every entry needs its reason; a commit that deletes the file fails. A raise not
/// yet committed fails, and uncommitted changes do not stop the committed ones being judged. `Some` says what
/// is wrong.
pub fn ratchet<T: Limit>(workspace: &Path, file: &str, prefix: &str) -> Result<Option<String>> {
    let now = std::fs::read_to_string(workspace.join(file)).with_context(|| format!("reading {file}"))?;
    if let Some(committed) = git(workspace, &["show", &format!("HEAD:{file}")])?.filter(|c| *c != now) {
        if let Some((name, what)) =
            raised(&entries::<T>(&committed)?, &entries::<T>(&now)?).into_iter().next()
        {
            return Ok(Some(format!(
                "{name}: {what} and not committed; commit it with `{prefix}: {name}: <reason>`"
            )));
        }
    }
    let mut bases = MAIN_BRANCHES.iter().map(|branch| git(workspace, &["merge-base", "HEAD", branch]));
    let Some(base) = bases.find_map(|base| base.transpose()).transpose()? else {
        bail!("no merge-base with {} to judge {file}'s commits from", MAIN_BRANCHES.join(" or "));
    };
    let base = base.trim();
    let Some(commits) =
        git(workspace, &["log", "--reverse", "--format=%H", &format!("{base}..HEAD"), "--", file])?
    else {
        bail!("git log failed for {file}");
    };
    // The last version of the file before the commit judged: the main branch's, to start with.
    let mut last = git(workspace, &["show", &format!("{base}:{file}")])?;
    for commit in commits.lines() {
        let Some(text) = git(workspace, &["show", &format!("{commit}:{file}")])? else {
            return Ok(Some(format!("{file} deleted in {commit}; a budget file is never deleted")));
        };
        let before = last.replace(text.clone());
        let parent = git(workspace, &["show", &format!("{commit}^:{file}")])?.or(before);
        let now = entries::<T>(&text)?;
        let changed = match parent {
            Some(parent) => raised(&entries::<T>(&parent)?, &now),
            // A file the main branch lacks, new or renamed: every limit in it is a raise from none.
            None => now
                .iter()
                .map(|entry| {
                    let limits: Vec<_> =
                        entry.limits().iter().map(|(key, is)| format!("{key} = {is}")).collect();
                    (
                        entry.name().to_string(),
                        format!("added ({}) in a file the main branch lacks", limits.join(", ")),
                    )
                })
                .collect(),
        };
        let message =
            git(workspace, &["log", "--format=%B", &format!("{commit}^..{commit}")])?.unwrap_or_default();
        let unexplained = changed.into_iter().find(|(name, _)| !explains(&message, prefix, name));
        if let Some((name, what)) = unexplained {
            return Ok(Some(format!(
                "{name}: {what} in {commit} without a `{prefix}: {name}: <reason>` line"
            )));
        }
    }
    Ok(None)
}

/// Count every crate against its ceiling, then check the ceilings only fell. Returns the first
/// failure, if any, and a summary.
pub fn check(workspace: &Path, file: &str, budget: &SizeBudget) -> Result<(Option<String>, String)> {
    ensure!(!budget.crates.is_empty(), "no crates in the size budget");
    let mut summary = Vec::new();
    let mut failure = None;
    for c in &budget.crates {
        ensure!(!c.paths.is_empty(), "size budget {}: no paths", c.name);
        let mut lines = 0;
        for path in &c.paths {
            lines += count_path(&workspace.join(path)).with_context(|| format!("size budget {}", c.name))?;
        }
        summary.push(format!("{}: {lines} of {} lines", c.name, c.max_lines));
        if lines > c.max_lines {
            failure.get_or_insert(format!("{}: {lines} lines, ceiling is {}", c.name, c.max_lines));
        }
    }
    if failure.is_none() {
        failure = ratchet::<SizeCrate>(workspace, file, "Size budget")?;
    }
    Ok((failure, summary.join("\n      ")))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn only_code_lines_count() {
        let text = "//! Doc.\n\n/// Item doc.\nfn f() {} // trailing\n/* one\n   two */\n/* a */ let x = 1;\n  // indented\n";
        assert_eq!(scan(text).unwrap().lines, 2);
    }

    /// A `#[cfg(test)]` item counts for nothing, whatever brackets its strings and characters
    /// hold; what follows it counts again.
    #[test]
    fn a_test_module_does_not_count() {
        let text = r##"fn f() {}
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let s = "}}"; let r = r#"{"#; let c = '}'; let e = '\'';
        /* } /* { */ } */
        fn g<'a>(x: &'a str) -> &'a str { x }
    }
}
#[cfg(test)]
use std::{fmt, io};
struct S {
    #[cfg(test)]
    seen: [u8; 2],
    a: u8,
}
fn h() {}
"##;
        let scan = scan(text).unwrap();
        assert_eq!(scan.lines, 5);
        assert!(scan.modules.is_empty());
    }

    /// A test module declared out of line leaves its file out, and every module that file declares.
    #[test]
    fn a_test_module_file_does_not_count() {
        let dir = std::env::temp_dir().join(format!("redoubt-size-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
        file("lib.rs", "mod real;\n#[cfg(test)]\nmod tests;\nfn f() {}\n");
        file("real.rs", "fn g() {}\n");
        file("tests.rs", "#[path = \"common_tests.rs\"]\nmod common;\nfn t() {}\n");
        file("common_tests.rs", "fn c() {}\nfn d() {}\n");
        assert_eq!(count_path(&dir).unwrap(), 3);
        std::fs::remove_file(dir.join("common_tests.rs")).unwrap();
        assert!(count_path(&dir).is_err_and(|e| e.to_string().contains("common_tests.rs")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file a shipped module reaches counts, though a test module names it too by `#[path]`; a
    /// module declared inside an inline one lives in that module's directory, test or not; a path
    /// under `cfg_attr` counts, its default file only if it exists; a module file the case cannot
    /// find fails it.
    #[test]
    fn a_shipped_file_always_counts() {
        let dir = std::env::temp_dir().join(format!("redoubt-size-reach-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("outer")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        let file = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
        let lib =
            "mod real;\n#[cfg(test)]\n#[path = \"real.rs\"]\nmod alias;\nmod outer {\n    mod inner;\n}\n";
        let backend = "#[cfg_attr(\n    feature = \"x\",\n    path = \"backend_x.rs\"\n)]\nmod backend;\n";
        file("lib.rs", &format!("{lib}{backend}#[cfg(test)]\nmod tests {{\n    mod x;\n}}\n"));
        file("real.rs", "fn g() {}\n");
        file("outer/inner.rs", "fn i() {}\n");
        file("backend_x.rs", "fn b() {}\n");
        file("tests/x.rs", "fn x() {}\nfn y() {}\n");
        // lib.rs's nine lines outside its tests, real.rs, inner.rs and backend_x.rs: not tests/x.rs.
        assert_eq!(count_path(&dir).unwrap(), 12);
        std::fs::remove_file(dir.join("outer/inner.rs")).unwrap();
        assert!(count_path(&dir).is_err_and(|e| e.to_string().contains("module inner's file")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A module the case cannot follow fails it: a raw name, a macro's `$name`, and the word
    /// `include` however it is invoked. A raw identifier that is not a module is a name like any
    /// other, and `include_str!` is another word.
    #[test]
    fn a_form_the_case_cannot_follow_fails() {
        for text in [
            "mod r#real;\n",
            "macro_rules! m {\n    ($n:ident) => { mod $n; };\n}\n",
            "include!(\"real.rs\");\n",
            "include !(\"real.rs\");\n",
            "use core::include as inc;\ninc!(\"real.rs\");\n",
            "r#include!(\"real.rs\");\n",
        ] {
            assert!(scan(text).is_err(), "{text}");
        }
        assert_eq!(scan("fn f(r#mod: u8) {}\n").unwrap().lines, 1);
        assert_eq!(scan("const S: &str = include_str!(\"s.txt\"); // include!\n").unwrap().lines, 1);
    }

    fn crates(list: &[(&str, &[&str], usize)]) -> Vec<SizeCrate> {
        list.iter()
            .map(|(name, paths, max)| SizeCrate {
                name: name.to_string(),
                paths: paths.iter().map(|p| p.to_string()).collect(),
                max_lines: *max,
            })
            .collect()
    }

    /// A raise, a dropped crate and a narrowed path set each need a reason; a fall, a new crate
    /// and a widened path set do not.
    #[test]
    fn a_raise_needs_its_reason() {
        let before = crates(&[("kernel", &["k"], 100), ("loader", &["l"], 50)]);
        let now = crates(&[("kernel", &["k"], 120), ("loader", &["l", "l2"], 40), ("new", &["n"], 9)]);
        let one = |name: &str, what: &str| vec![(name.to_string(), what.to_string())];
        assert_eq!(raised(&before, &now), one("kernel", "max_lines raised from 100 to 120"));
        let renamed = crates(&[("kernel", &["k"], 100), ("boot", &["l"], 50)]);
        assert_eq!(raised(&before, &renamed), one("loader", "dropped (max_lines = 50)"));
        let narrowed = crates(&[("kernel", &["k/src"], 100), ("loader", &["l"], 50)]);
        assert_eq!(raised(&before, &narrowed), one("kernel", "paths narrowed (no longer counts k)"));
        let message = "x\nSize budget: kernel: the timer wheel\nSize budget: loader:\n";
        assert!(explains(message, "Size budget", "kernel"));
        assert!(!explains(message, "Size budget", "loader"), "a reason is never empty");
        assert!(!explains(message, "Size budget", "kern"), "a name is matched whole");
        // Budget names that hold `: ` themselves, as the unsafe budget's kernel and runtime ones do.
        let message = "Unsafe budget: kernel: core: a new trap path\n";
        assert!(explains(message, "Unsafe budget", "kernel: core"));
        assert!(!explains(message, "Unsafe budget", "kernel: Sv39, SBI and PLIC backends"));
        let rt =
            "redoubt-rt (native runtime): the heap's free lists, page buffers and lends, the startup page";
        assert!(explains(&format!("Unsafe budget: {rt}: the fixed arena's run list\n"), "Unsafe budget", rt));
    }

    /// A throwaway git repository holding one budget file, `b.toml`, on the main branch `redoubt`.
    pub struct Scratch(pub std::path::PathBuf);

    impl Scratch {
        pub fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("redoubt-size-{tag}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let scratch = Self(dir);
            scratch.run(&["init", "-q", "-b", "redoubt"]);
            scratch.run(&["config", "user.email", "t@t"]);
            scratch.run(&["config", "user.name", "t"]);
            scratch
        }

        pub fn run(&self, args: &[&str]) {
            assert!(
                Command::new("git").current_dir(&self.0).args(args).status().unwrap().success(),
                "{args:?}"
            )
        }

        pub fn write(&self, text: &str) { std::fs::write(self.0.join("b.toml"), text).unwrap(); }

        fn ceiling(&self, max: usize) {
            self.write(&format!("[[crate]]\nname = \"k\"\npaths = [\"k\"]\nmax_lines = {max}\n"));
        }

        fn ratchet(&self) -> Option<String> {
            ratchet::<SizeCrate>(&self.0, "b.toml", "Size budget").unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
    }

    /// On a package branch: a committed raise without its reason fails, with it passes, an
    /// uncommitted raise fails, and a raise without its reason fails behind a later commit too.
    #[test]
    fn the_ratchet_reads_the_commit_that_raised() {
        let repo = Scratch::new("commits");
        repo.ceiling(10);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "first"]);
        repo.run(&["checkout", "-qb", "wp"]);
        assert_eq!(repo.ratchet(), None);
        repo.ceiling(12);
        assert!(repo.ratchet().is_some_and(|w| w.contains("not committed")));
        repo.run(&["commit", "-qam", "grow"]);
        assert!(repo.ratchet().is_some_and(|w| w.contains("without")));
        repo.run(&["commit", "-q", "--amend", "-m", "grow\n\nSize budget: k: it needs it\n"]);
        assert_eq!(repo.ratchet(), None);
        repo.ceiling(8);
        assert_eq!(repo.ratchet(), None);
        repo.run(&["commit", "-qam", "shrink"]);
        repo.ceiling(9);
        repo.run(&["commit", "-qam", "grow again"]);
        repo.ceiling(7);
        repo.run(&["commit", "-qam", "shrink again"]);
        assert!(repo.ratchet().is_some_and(|w| w.contains("from 8 to 9")));
        // An uncommitted fall does not hide it.
        repo.ceiling(6);
        assert!(repo.ratchet().is_some_and(|w| w.contains("from 8 to 9")));
    }

    /// A budget file the main branch lacks, new or renamed, needs a reason for every entry.
    #[test]
    fn a_new_budget_file_needs_every_reason() {
        let repo = Scratch::new("new");
        std::fs::write(repo.0.join("x"), "x").unwrap();
        repo.run(&["add", "x"]);
        repo.run(&["commit", "-qm", "first"]);
        repo.run(&["checkout", "-qb", "wp"]);
        repo.write("[[crate]]\nname = \"k\"\npaths = [\"k\"]\nmax_lines = 10\n[[crate]]\nname = \"l\"\npaths = [\"l\"]\nmax_lines = 5\n");
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "budget\n\nSize budget: k: the kernel\n"]);
        assert!(repo.ratchet().is_some_and(|w| w.contains("l: added (max_lines = 5)")));
        repo.run(&[
            "commit",
            "-q",
            "--amend",
            "-m",
            "budget\n\nSize budget: k: the kernel\nSize budget: l: the loader\n",
        ]);
        assert_eq!(repo.ratchet(), None);
    }

    /// Deleting the file on a branch fails, whatever comes after: the file added back raised, or
    /// added back as it was and then raised with a reason.
    #[test]
    fn a_deleted_budget_file_fails() {
        let repo = Scratch::new("deleted");
        repo.ceiling(10);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "first"]);
        repo.run(&["checkout", "-qb", "wp"]);
        repo.run(&["rm", "-q", "b.toml"]);
        repo.run(&["commit", "-qm", "drop"]);
        repo.ceiling(12);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "back"]);
        assert!(repo.ratchet().is_some_and(|w| w.contains("deleted in")));
        repo.run(&["reset", "-q", "--hard", "redoubt"]);
        repo.run(&["rm", "-q", "b.toml"]);
        repo.run(&["commit", "-qm", "drop"]);
        repo.run(&["checkout", "-q", "redoubt", "--", "b.toml"]);
        repo.run(&["commit", "-qm", "back"]);
        repo.ceiling(12);
        repo.run(&["commit", "-qam", "grow\n\nSize budget: k: it needs it\n"]);
        assert!(repo.ratchet().is_some_and(|w| w.contains("deleted in")));
    }

    /// A merge that brings in a raise with its reason passes; a raise made in the merge itself
    /// needs the reason in the merge's message.
    #[test]
    fn a_merge_is_judged_against_its_first_parent() {
        let repo = Scratch::new("merges");
        repo.ceiling(10);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "first"]);
        repo.run(&["checkout", "-qb", "wp"]);
        repo.run(&["checkout", "-qb", "side"]);
        repo.ceiling(12);
        repo.run(&["commit", "-qam", "grow\n\nSize budget: k: it needs it\n"]);
        repo.run(&["checkout", "-q", "wp"]);
        repo.run(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
        assert_eq!(repo.ratchet(), None);
        repo.run(&["checkout", "-qb", "other", "HEAD~1"]);
        std::fs::write(repo.0.join("x"), "x").unwrap();
        repo.run(&["add", "x"]);
        repo.run(&["commit", "-qm", "unrelated"]);
        repo.run(&["checkout", "-q", "wp"]);
        repo.run(&["merge", "-q", "--no-ff", "--no-commit", "other"]);
        repo.ceiling(14);
        repo.run(&["commit", "-qam", "merge other"]);
        assert!(repo.ratchet().is_some_and(|w| w.contains("from 12 to 14")));
    }

    /// A raise already on the main branch is not read again, on it or on a branch from it; with no
    /// main branch to measure from, the check fails.
    #[test]
    fn merged_history_is_not_read_again() {
        let repo = Scratch::new("merged");
        repo.ceiling(10);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "first"]);
        repo.ceiling(12);
        repo.run(&["commit", "-qam", "grow"]);
        assert_eq!(repo.ratchet(), None);
        repo.run(&["checkout", "-qb", "wp"]);
        assert_eq!(repo.ratchet(), None);
        repo.ceiling(13);
        repo.run(&["commit", "-qam", "grow again"]);
        assert!(repo.ratchet().is_some_and(|w| w.contains("from 12 to 13")));
        repo.run(&["branch", "-qm", "redoubt", "trunk"]);
        let error = ratchet::<SizeCrate>(&repo.0, "b.toml", "Size budget").unwrap_err();
        assert!(error.to_string().contains("no merge-base"), "{error}");
    }
}
