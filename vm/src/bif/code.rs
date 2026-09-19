//! The code path: directories of the VM's own file system where modules are looked for, as well
//! as the platform (`code:add_patha/1`, `code:get_path/0`, ...). OTP keeps this in the
//! `code_server` process; here it is VM state. Directories added in front (`add_patha`) are
//! searched before the platform's modules, as in BEAM (so a consolidated protocol written to
//! one replaces the platform's); the others after them. Code that can change the path can load
//! any code anyway (`code:load_binary/3`), so this grants nothing.

use alloc::string::String;
use alloc::vec::Vec;

use super::Ctx;
use crate::platform::FileKind;
use crate::process::Exception;
use crate::term::Term;
use crate::vm::Found;

type R = Result<Term, Exception>;

/// Most directories on the code path.
const MAX_PATHS: usize = 1024;

/// A directory argument, resolved against the working directory: `Err(())` if it is not a
/// directory the file system has.
fn directory(c: &mut Ctx, t: &Term) -> Result<Result<String, ()>, Exception> {
    let name = super::file::name_bytes(c.heap(), *t).ok_or_else(|| c.badarg())?;
    let Ok(path) = super::file::resolve(&c.sys.cwd, &name) else { return Ok(Err(())) };
    let is_dir = c.sys.platform.files().and_then(|f| f.info(&path, true).ok()).is_some_and(|i| i.kind == FileKind::Directory);
    Ok(if is_dir { Ok(path) } else { Err(()) })
}

fn add(c: &mut Ctx, t: &Term, front: bool) -> Result<bool, Exception> {
    let Ok(dir) = directory(c, t)? else { return Ok(false) };
    remove(c, &dir);
    if c.sys.code_path.len() >= MAX_PATHS {
        return Err(c.system_limit());
    }
    if front {
        c.sys.code_path.insert(0, dir);
        c.sys.platform_at += 1;
    } else {
        c.sys.code_path.push(dir);
    }
    Ok(true)
}

/// Take `dir` off the path: whether it was on it.
fn remove(c: &mut Ctx, dir: &str) -> bool {
    let Some(i) = c.sys.code_path.iter().position(|p| p == dir) else { return false };
    c.sys.code_path.remove(i);
    if i < c.sys.platform_at {
        c.sys.platform_at -= 1;
    }
    true
}

fn added(c: &mut Ctx, ok: bool) -> Term {
    if ok {
        c.bool(true)
    } else {
        { let e = [Term::Atom(c.sys.atoms.error.clone()), c.atom("bad_directory")]; c.tuple(&e) }
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
    for d in c.list_arg(a[0])? {
        add(c, &d, true)?;
    }
    Ok(c.ok())
}

pub fn add_pathsz(c: &mut Ctx, a: &[Term]) -> R {
    for d in c.list_arg(a[0])? {
        add(c, &d, false)?;
    }
    Ok(c.ok())
}

/// `del_path(Dir)`: `true` if it was on the path.
pub fn del_path(c: &mut Ctx, a: &[Term]) -> R {
    let name = super::file::name_bytes(c.heap(), a[0]).ok_or_else(|| c.badarg())?;
    let Ok(dir) = super::file::resolve(&c.sys.cwd, &name) else { return Ok(c.bool(false)) };
    let removed = remove(c, &dir);
    Ok(c.bool(removed))
}

pub fn del_paths(c: &mut Ctx, a: &[Term]) -> R {
    for d in c.list_arg(a[0])? {
        del_path(c, &[d])?;
    }
    Ok(c.ok())
}

pub fn get_path(c: &mut Ctx, _a: &[Term]) -> R {
    let paths = c.sys.code_path.clone();
    let v: Vec<Term> = paths.iter().map(|p| c.string(p)).collect();
    Ok(c.list(v))
}

/// `set_path(Dirs)`: `true`, or `{error, bad_directory}` (leaving the path as it was).
pub fn set_path(c: &mut Ctx, a: &[Term]) -> R {
    let mut paths = Vec::new();
    for d in c.list_arg(a[0])? {
        match directory(c, &d)? {
            Ok(p) if !paths.contains(&p) => paths.push(p),
            Ok(_) => {}
            Err(()) => return Ok(added(c, false)),
        }
    }
    if paths.len() > MAX_PATHS {
        return Err(c.system_limit());
    }
    // A path set whole comes after the platform's modules.
    c.sys.code_path = paths;
    c.sys.platform_at = 0;
    Ok(c.bool(true))
}

/// `which(Module)`: the file it is (or would be) loaded from in the VM's file system,
/// `preloaded` for modules the platform supplies, or `non_existing`.
pub fn which(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    if crate::vm::RUNTIME_MODULES.contains(&m.as_str()) {
        return Ok(c.atom("non_existing"));
    }
    if c.sys.is_loaded(m) {
        if let Some(file) = c.sys.module_files.get(m.as_str()).cloned() {
            return Ok(c.copy_in(&file));
        }
    }
    let name = String::from(m.as_str());
    Ok(match c.sys.locate_module(&name) {
        Some(Found::Path(path, _)) => c.string(&path),
        Some(Found::Platform(_)) => match c.sys.platform.module_file(&name) {
            Some(path) => c.string(&path),
            None => c.atom("preloaded"),
        },
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
        out.push({ let e = [c.string(m.as_str()), c.atom("preloaded"), c.bool(true)]; c.tuple(&e) });
    }
    let dirs = c.sys.code_path.clone();
    for dir in dirs {
        let Some(names) = c.sys.platform.files().and_then(|f| f.list_dir(&dir).ok()) else { continue };
        for n in names {
            let Some(module) = core::str::from_utf8(&n).ok().and_then(|n| n.strip_suffix(".beam")) else { continue };
            if seen.insert(String::from(module)) {
                let file = c.string(&alloc::format!("{}/{}.beam", dir.trim_end_matches('/'), module));
                out.push({ let e = [c.string(module), file, c.bool(false)]; c.tuple(&e) });
            }
        }
    }
    Ok({ let v = out; c.list(v) })
}

/// The directory of application `app`: `Root/App` or the highest `Root/App-Vsn` in the first
/// lib root that has one.
fn lib_dir_of(c: &mut Ctx, app: &str) -> Option<String> {
    let roots = c.sys.lib_roots.clone();
    let files = c.sys.platform.files()?;
    for root in roots {
        let Ok(names) = files.list_dir(&root) else { continue };
        let mut best: Option<(Vec<u64>, String)> = None;
        for n in names {
            let Ok(n) = core::str::from_utf8(&n) else { continue };
            let vsn = if n == app {
                Some(Vec::new())
            } else {
                n.strip_prefix(app).and_then(|r| r.strip_prefix('-')).map(|v| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect())
            };
            if let Some(vsn) = vsn {
                if best.as_ref().is_none_or(|(b, _)| vsn > *b) {
                    best = Some((vsn, alloc::format!("{}/{}", root.trim_end_matches('/'), n)));
                }
            }
        }
        if let Some((_, dir)) = best {
            return Some(dir);
        }
    }
    None
}

fn app_name(c: &Ctx, t: &Term) -> Result<String, Exception> {
    match t {
        Term::Atom(a) => Ok(String::from(a.as_str())),
        _ => super::file::name_bytes(c.heap(), *t).and_then(|b| String::from_utf8(b).ok()).ok_or_else(|| c.badarg()),
    }
}

fn bad_name(c: &mut Ctx) -> Term {
    { let e = [Term::Atom(c.sys.atoms.error.clone()), c.atom("bad_name")]; c.tuple(&e) }
}

/// `lib_dir(App)`, and `lib_dir(App, SubDir)`: `{error, bad_name}` if there is no such
/// application.
pub fn lib_dir(c: &mut Ctx, a: &[Term]) -> R {
    let app = app_name(c, &a[0])?;
    let sub = match a.get(1) {
        Some(s) => Some(app_name(c, s)?),
        None => None,
    };
    Ok(match (lib_dir_of(c, &app), sub) {
        (Some(d), None) => c.string(&d),
        (Some(d), Some(s)) => c.string(&alloc::format!("{d}/{s}")),
        (None, _) => bad_name(c),
    })
}

pub fn priv_dir(c: &mut Ctx, a: &[Term]) -> R {
    let app = app_name(c, &a[0])?;
    Ok(match lib_dir_of(c, &app) {
        Some(d) => c.string(&alloc::format!("{d}/priv")),
        None => bad_name(c),
    })
}

/// `root_dir()`: the parent of the first lib root (OTP's installation directory), or `/`.
pub fn root_dir(c: &mut Ctx, _a: &[Term]) -> R {
    let root = c.sys.lib_roots.first().map(|r| {
        let r = r.trim_end_matches('/');
        match r.rfind('/') {
            Some(0) | None => String::from("/"),
            Some(i) => String::from(&r[..i]),
        }
    });
    Ok(c.string(root.as_deref().unwrap_or("/")))
}

/// Load `bytes` as `module` and remember the file it came from.
fn load_from(c: &mut Ctx, module: &crate::atom::Atom, bytes: &[u8], file: Term) -> Term {
    match c.sys.load_bytes(bytes) {
        Ok(name) if &name == module => {
            let file = c.own(file);
            c.sys.module_files.insert(String::from(name.as_str()), file);
            { let e = [c.atom("module"), Term::Atom(name)]; c.tuple(&e) }
        }
        Ok(_) | Err(_) => { let e = [Term::Atom(c.sys.atoms.error.clone()), c.atom("badfile")]; c.tuple(&e) },
    }
}

/// `load_file(Module)`: (re)load `Module` from the platform or the VM's code path.
pub fn load_file(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    let m = m.clone();
    Ok(match c.sys.locate_module(m.as_str()) {
        Some(Found::Path(path, bytes)) => {
            let file = c.string(&path);
            load_from(c, &m, &bytes, file)
        }
        Some(Found::Platform(bytes)) => {
            let file = match c.sys.platform.module_file(m.as_str()) {
                Some(p) => c.string(&p),
                None => c.atom("preloaded"),
            };
            load_from(c, &m, &bytes, file)
        }
        None => { let e = [Term::Atom(c.sys.atoms.error.clone()), c.atom("nofile")]; c.tuple(&e) },
    })
}

/// `load_abs(File)`: load `File.beam` from the VM's file system.
pub fn load_abs(c: &mut Ctx, a: &[Term]) -> R {
    let name = super::file::name_bytes(c.heap(), a[0]).ok_or_else(|| c.badarg())?;
    let nofile = |c: &mut Ctx| { let e = [Term::Atom(c.sys.atoms.error.clone()), c.atom("nofile")]; c.tuple(&e) };
    let Ok(path) = super::file::resolve(&c.sys.cwd, &name) else { return Ok(nofile(c)) };
    let file = alloc::format!("{path}.beam");
    let max = c.sys.limits.max_binary_bits / 8;
    let bytes = match c.sys.platform.files().map(|f| super::read_whole_file(f, &file, max)) {
        Some(Ok(b)) => b,
        _ => return Ok(nofile(c)),
    };
    let module = path.rsplit('/').next().unwrap_or("");
    let Some(m) = c.sys.atom_table.existing(module).or_else(|| c.sys.atom_table.intern(module).ok()) else {
        return Ok(nofile(c));
    };
    let file = c.string(&file);
    Ok(load_from(c, &m, &bytes, file))
}
