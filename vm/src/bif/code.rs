//! The code path: directories of the VM's own file system where modules are looked for when the
//! platform does not have them (`code:add_patha/1`, `code:get_path/0`, ...). OTP keeps this in
//! the `code_server` process; here it is VM state, and the platform's modules always come first.

use alloc::string::String;
use alloc::vec::Vec;

use super::Ctx;
use crate::platform::FileKind;
use crate::process::Exception;
use crate::term::Term;

type R = Result<Term, Exception>;

/// Most directories on the code path.
const MAX_PATHS: usize = 1024;

fn string(s: &str) -> Term {
    Term::list(s.chars().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>())
}

/// A directory argument, resolved against the working directory: `Err(())` if it is not a
/// directory the file system has.
fn directory(c: &mut Ctx, t: &Term) -> Result<Result<String, ()>, Exception> {
    let name = super::file::name_bytes(t).ok_or_else(|| c.badarg())?;
    let Ok(path) = super::file::resolve(&c.sys.cwd, &name) else { return Ok(Err(())) };
    let is_dir = c.sys.platform.files().and_then(|f| f.info(&path, true).ok()).is_some_and(|i| i.kind == FileKind::Directory);
    Ok(if is_dir { Ok(path) } else { Err(()) })
}

fn add(c: &mut Ctx, t: &Term, front: bool) -> Result<bool, Exception> {
    let Ok(dir) = directory(c, t)? else { return Ok(false) };
    let paths = &mut c.sys.code_path;
    paths.retain(|p| *p != dir);
    if paths.len() >= MAX_PATHS {
        return Err(c.system_limit());
    }
    if front {
        paths.insert(0, dir);
    } else {
        paths.push(dir);
    }
    Ok(true)
}

fn added(c: &mut Ctx, ok: bool) -> Term {
    if ok {
        c.bool(true)
    } else {
        Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), c.atom("bad_directory")])
    }
}

/// `add_patha(Dir)`: search `Dir` first; `{error, bad_directory}` if it is not one.
pub fn add_patha(c: &mut Ctx, a: &[Term]) -> R {
    let ok = add(c, &a[0], true)?;
    Ok(added(c, ok))
}

pub fn add_pathz(c: &mut Ctx, a: &[Term]) -> R {
    let ok = add(c, &a[0], false)?;
    Ok(added(c, ok))
}

/// `add_pathsa(Dirs)`: each goes first, so the list ends up in front in reverse order, as
/// OTP does. Directories that do not exist are skipped.
pub fn add_pathsa(c: &mut Ctx, a: &[Term]) -> R {
    for d in a[0].to_vec().ok_or_else(|| c.badarg())? {
        add(c, &d, true)?;
    }
    Ok(c.ok())
}

pub fn add_pathsz(c: &mut Ctx, a: &[Term]) -> R {
    for d in a[0].to_vec().ok_or_else(|| c.badarg())? {
        add(c, &d, false)?;
    }
    Ok(c.ok())
}

/// `del_path(Dir)`: `true` if it was on the path.
pub fn del_path(c: &mut Ctx, a: &[Term]) -> R {
    let name = super::file::name_bytes(&a[0]).ok_or_else(|| c.badarg())?;
    let Ok(dir) = super::file::resolve(&c.sys.cwd, &name) else { return Ok(c.bool(false)) };
    let before = c.sys.code_path.len();
    c.sys.code_path.retain(|p| *p != dir);
    let removed = c.sys.code_path.len() != before;
    Ok(c.bool(removed))
}

pub fn del_paths(c: &mut Ctx, a: &[Term]) -> R {
    for d in a[0].to_vec().ok_or_else(|| c.badarg())? {
        del_path(c, &[d])?;
    }
    Ok(c.ok())
}

pub fn get_path(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::list(c.sys.code_path.iter().map(|p| string(p)).collect::<Vec<_>>()))
}

/// `set_path(Dirs)`: `true`, or `{error, bad_directory}` (leaving the path as it was).
pub fn set_path(c: &mut Ctx, a: &[Term]) -> R {
    let mut paths = Vec::new();
    for d in a[0].to_vec().ok_or_else(|| c.badarg())? {
        match directory(c, &d)? {
            Ok(p) if !paths.contains(&p) => paths.push(p),
            Ok(_) => {}
            Err(()) => return Ok(added(c, false)),
        }
    }
    if paths.len() > MAX_PATHS {
        return Err(c.system_limit());
    }
    c.sys.code_path = paths;
    Ok(c.bool(true))
}

/// `which(Module)`: the file it is (or would be) loaded from in the VM's file system,
/// `preloaded` for modules the platform supplies, or `non_existing`.
pub fn which(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    if crate::vm::RUNTIME_MODULES.contains(&m.as_str()) {
        return Ok(c.atom("non_existing"));
    }
    if c.sys.platform.load_module(m.as_str()).is_some() {
        return Ok(c.atom("preloaded"));
    }
    let name = String::from(m.as_str());
    Ok(match c.sys.find_in_code_path(&name) {
        Some((path, _)) => string(&path),
        None => c.atom("non_existing"),
    })
}

/// `all_available()`: `{Name, File, Loaded}` for every loaded module and every `.beam` file on
/// the VM's code path. Modules the platform could supply but that are not loaded are not
/// listed: a platform has no directory to enumerate.
pub fn all_available(c: &mut Ctx, _a: &[Term]) -> R {
    let mut seen = alloc::collections::BTreeSet::new();
    let mut out = Vec::new();
    for m in c.sys.loaded_modules() {
        seen.insert(String::from(m.as_str()));
        out.push(Term::tuple(alloc::vec![string(m.as_str()), c.atom("preloaded"), c.bool(true)]));
    }
    let dirs = c.sys.code_path.clone();
    for dir in dirs {
        let Some(names) = c.sys.platform.files().and_then(|f| f.list_dir(&dir).ok()) else { continue };
        for n in names {
            let Some(module) = core::str::from_utf8(&n).ok().and_then(|n| n.strip_suffix(".beam")) else { continue };
            if seen.insert(String::from(module)) {
                let file = string(&alloc::format!("{}/{}.beam", dir.trim_end_matches('/'), module));
                out.push(Term::tuple(alloc::vec![string(module), file, c.bool(false)]));
            }
        }
    }
    Ok(Term::list(out))
}
