//! The `unsafe` ratchet: counts uses of the keyword in the trusted computing base and how
//! many of them lack a `// SAFETY:` justification, and fails if either exceeds its budget.
//! Budgets only ever get lowered. Raising one needs a reason in the commit that does it.

use std::path::Path;

use anyhow::{Context, Result};

use crate::case::Budget;

#[derive(Default)]
pub struct Count {
    pub total: usize,
    pub undocumented: usize,
}

/// How far above an `unsafe` block a `// SAFETY:` comment may start and still count.
const SAFETY_COMMENT_REACH: usize = 6;
/// How far above an `unsafe fn` / `unsafe impl` its `# Safety` doc section may start.
const SAFETY_DOC_REACH: usize = 16;

fn count_file(path: &Path, count: &mut Count) -> Result<()> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    for (number, line) in lines.iter().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let uses = code.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|word| *word == "unsafe").count();
        if uses == 0 {
            continue;
        }
        count.total += uses;
        // A block is justified by a `// SAFETY:` comment; a declaration states its contract
        // in a `# Safety` doc section instead.
        let is_declaration = ["unsafe fn", "unsafe impl", "unsafe extern", "unsafe trait"].iter().any(|d| code.contains(d));
        let (reach, marker) = if is_declaration { (SAFETY_DOC_REACH, "# Safety") } else { (SAFETY_COMMENT_REACH, "SAFETY:") };
        let above = &lines[number.saturating_sub(reach)..number];
        let documented = above.iter().any(|l| l.trim_start().starts_with("//") && (l.contains(marker) || l.contains("SAFETY:")))
            || line.contains("SAFETY:");
        if !documented {
            count.undocumented += uses;
        }
    }
    Ok(())
}

fn count_path(path: &Path, count: &mut Count) -> Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            count_path(&entry?.path(), count)?;
        }
    } else if path.extension().is_some_and(|e| e == "rs") {
        count_file(path, count)?;
    }
    Ok(())
}

/// Returns a description of the first budget that is exceeded, if any, and a summary line.
pub fn check(workspace: &Path, budgets: &[Budget]) -> Result<(Option<String>, String)> {
    let mut summary = Vec::new();
    let mut failure = None;
    for budget in budgets {
        let mut count = Count::default();
        for path in &budget.paths {
            count_path(&workspace.join(path), &mut count)?;
        }
        let name = &budget.name;
        summary.push(format!("{name}: {} unsafe, {} undocumented", count.total, count.undocumented));
        if count.total > budget.max_unsafe {
            failure.get_or_insert(format!("{name}: {} uses of unsafe, budget is {}", count.total, budget.max_unsafe));
        }
        if count.undocumented > budget.max_undocumented {
            failure.get_or_insert(format!(
                "{name}: {} unsafe without a SAFETY comment, budget is {}",
                count.undocumented, budget.max_undocumented
            ));
        }
        if count.total < budget.max_unsafe || count.undocumented < budget.max_undocumented {
            summary.push(format!("  (budget can be lowered to {} / {})", count.total, count.undocumented));
        }
    }
    Ok((failure, summary.join("\n      ")))
}
