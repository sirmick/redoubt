//! `erofsd-client`: a `servers` entry that uses `erofsd`s started by `init` through the root badges
//! its entry is handed, and prints on its console exactly what each `erofsd` answered: the
//! `Rerror` text is the server's, so the case's verdict is the system's. Its first argument names
//! what it checks:
//!
//! - `corrupt ENDPOINT PATH [ENDPOINT PATH...]`: on each volume, attaches (twice, if the attach is refused:
//!   the server is still up), walks to `PATH` and reads it, and says the first refusal. Passes when every one
//!   is `corrupt`.
//! - `readonly ENDPOINT`: on the stage `tests/data/erofsd/stage`, a create, an open for writing or
//!   truncating, and a remove are each refused; `lib` lists its three files in order; `..` from the root,
//!   sent as it is, stays at the root; `tail.txt`, whose last block is inline, reads whole and byte-equal to
//!   what was staged.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_init_programs::Out;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::{WORDS_9P, mode};
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::ninep::{Body, Message, NOFID, Names, Qid, VERSION, stats};

redoubt_rt::entry!(run);

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let mut args = startup.args();
    let checked = match args.next() {
        Some("corrupt") => corrupt(startup, &mut out, args),
        Some("readonly") => match args.next() {
            Some(at) => readonly(startup, &mut out, at),
            None => Err("no endpoint".into()),
        },
        check => Err(format!("no such check: {check:?}")),
    };
    let line = match checked {
        Ok(()) => String::from("erofsd-client TEST PASSED\n"),
        Err(why) => format!("erofsd-client TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

/// One 9P exchange with `to`: the reply's body, or the server's `Rerror` text as it sent it. A
/// failure of the call itself is said in brackets, so it is never taken for a server's word.
fn rpc<'l>(to: &Endpoint, lend: &'l mut Lend, body: Body<'_>) -> Result<Body<'l>, String> {
    let pages = lend.pages().map_err(|e| format!("[lend: {e:?}]"))?;
    Message { tag: 1, body }.encode(pages).map_err(|e| format!("[encode: {e:?}]"))?;
    let (reply, _) =
        lend.call(to, &WORDS_9P, &[], FOREVER).into_result().map_err(|e| format!("[call: {e:?}]"))?;
    if reply.words != WORDS_9P {
        return Err("[not a 9P reply]".into());
    }
    match Message::decode(lend.bytes()).map_err(|e| format!("[decode: {e:?}]"))?.body {
        Body::Rerror { ename } => Err(ename.into()),
        body => Ok(body),
    }
}

/// `Tversion` then `Tattach` on fid 0: the root's qid.
fn attach(to: &Endpoint, lend: &mut Lend) -> Result<Qid, String> {
    rpc(to, lend, Body::Tversion { msize: redoubt_rt::wire::MSIZE as u32, version: VERSION })?;
    match rpc(to, lend, Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" })? {
        Body::Rattach { qid } => Ok(qid),
        other => Err(format!("[attach: {other:?}]")),
    }
}

/// Walks `newfid` from `fid` by `names`, sent exactly as given: the last qid.
fn walk(to: &Endpoint, lend: &mut Lend, fid: u32, newfid: u32, names: &[&str]) -> Result<Qid, String> {
    let wnames = Names::new(names).map_err(|_| String::from("[names]"))?;
    match rpc(to, lend, Body::Twalk { fid, newfid, wnames })? {
        Body::Rwalk { qids } if qids.as_slice().len() == names.len() => {
            Ok(qids.as_slice().last().copied().unwrap_or_default())
        }
        Body::Rwalk { .. } => Err("[a partial walk]".into()),
        other => Err(format!("[walk: {other:?}]")),
    }
}

fn open(to: &Endpoint, lend: &mut Lend, fid: u32, m: u8) -> Result<(), String> {
    rpc(to, lend, Body::Topen { fid, mode: m }).map(|_| ())
}

/// The whole of the file or directory open on `fid`.
fn read_all(to: &Endpoint, lend: &mut Lend, fid: u32) -> Result<Vec<u8>, String> {
    let mut all = Vec::new();
    loop {
        let count = lend.iounit() as u32;
        match rpc(to, lend, Body::Tread { fid, offset: all.len() as u64, count })? {
            Body::Rread { data: [] } => return Ok(all),
            Body::Rread { data } => all.extend_from_slice(data),
            other => return Err(format!("[read: {other:?}]")),
        }
    }
}

fn endpoint(startup: &Startup, at: &str) -> Result<Endpoint, String> {
    startup.handle(at).map(Endpoint::from_handle).ok_or_else(|| format!("no {at} handle"))
}

/// `erofs-corrupt`: what each volume's `erofsd` refuses, and with what.
fn corrupt<'a>(
    startup: &Startup,
    out: &mut Out,
    mut args: impl Iterator<Item = &'a str>,
) -> Result<(), String> {
    let mut all = true;
    while let (Some(at), Some(path)) = (args.next(), args.next()) {
        let to = endpoint(startup, at)?;
        let said = match attach(&to, &mut out.lend) {
            Err(first) => {
                let again = attach(&to, &mut out.lend).err().unwrap_or_else(|| "[attached]".into());
                all &= first == "corrupt" && again == "corrupt";
                format!("attach refused: {first}, and again: {again}")
            }
            Ok(_) => {
                // One name a walk, so a refusal past the first name is the server's `Rerror`,
                // not a walk cut short (intro(5)).
                let refused = walk(&to, &mut out.lend, 0, 1, &[])
                    .and_then(|_| {
                        path.split('/')
                            .try_for_each(|name| walk(&to, &mut out.lend, 1, 1, &[name]).map(|_| ()))
                    })
                    .map_err(|e| format!("walk to {path} refused: {e}"))
                    .and_then(|_| {
                        open(&to, &mut out.lend, 1, mode::OREAD).map_err(|e| format!("open refused: {e}"))
                    })
                    .and_then(|()| read_all(&to, &mut out.lend, 1).map_err(|e| format!("read refused: {e}")));
                match refused {
                    Err(why) => {
                        all &= why.ends_with(": corrupt");
                        why
                    }
                    Ok(bytes) => {
                        all = false;
                        format!("{path} read, {} bytes", bytes.len())
                    }
                }
            }
        };
        out.say(&format!("erofsd-client: {at}: {said}\n")).map_err(|e| format!("say: {e:?}"))?;
    }
    if all { Ok(()) } else { Err("not every volume was refused as corrupt".into()) }
}

/// What `tests/data/erofsd/stage/tail.txt` holds.
fn tail() -> Vec<u8> { (0..260).flat_map(|i| format!("erofs tail line {i:04}\n").into_bytes()).collect() }

/// `erofs-read-only`.
fn readonly(startup: &Startup, out: &mut Out, at: &str) -> Result<(), String> {
    let to = endpoint(startup, at)?;
    let lend = &mut out.lend;
    let root = attach(&to, lend)?;
    // Every way of writing: each answer is the server's.
    let mut refused = Vec::new();
    walk(&to, lend, 0, 1, &["motd"])?;
    for m in [mode::OWRITE, mode::ORDWR, mode::OREAD | mode::OTRUNC] {
        refused.push(open(&to, lend, 1, m).err().unwrap_or_else(|| "[opened]".into()));
    }
    walk(&to, lend, 0, 2, &[])?;
    let created = rpc(&to, lend, Body::Tcreate { fid: 2, name: "new", perm: 0o644, mode: mode::OWRITE });
    refused.push(created.err().unwrap_or_else(|| "[created]".into()));
    refused.push(rpc(&to, lend, Body::Tremove { fid: 1 }).err().unwrap_or_else(|| "[removed]".into()));
    let said = refused.join(", ");
    out.say(&format!("erofsd-client: write, read-write, truncate, create, remove: {said}\n"))
        .map_err(|e| format!("say: {e:?}"))?;
    let lend = &mut out.lend;
    // A directory lists exactly its children, in order.
    walk(&to, lend, 0, 3, &["lib"])?;
    open(&to, lend, 3, mode::OREAD)?;
    let listing = read_all(&to, lend, 3)?;
    let names: Result<Vec<String>, String> = stats(&listing)
        .map(|s| s.map(|s| String::from(s.name)).map_err(|e| format!("[stat: {e:?}]")))
        .collect();
    let names = names?.join(" ");
    // `..`, sent as it is, never leaves the root.
    let up = walk(&to, lend, 0, 4, &["..", ".."])?;
    let back = walk(&to, lend, 0, 5, &["lib", "..", "..", "lib", ".."])?;
    let stays = up.path == root.path && back.path == root.path;
    // A file whose last block is inline reads whole and byte-equal.
    walk(&to, lend, 0, 6, &["tail.txt"])?;
    open(&to, lend, 6, mode::OREAD)?;
    let read = read_all(&to, lend, 6)?;
    let equal = read == tail();
    out.say(&format!(
        "erofsd-client: lib lists: {names}; .. from the root {}; tail.txt read {} bytes, {}\n",
        if stays { "stays at the root" } else { "LEFT the root" },
        read.len(),
        if equal { "byte-equal" } else { "DIFFERENT" },
    ))
    .map_err(|e| format!("say: {e:?}"))?;
    let read_only = refused.iter().all(|r| r == "read-only volume");
    if read_only && names == "a.beam b.beam c.beam" && stays && equal {
        Ok(())
    } else {
        Err("a check failed: see the lines above".into())
    }
}
