//! Ports to other programs: `open_port/2` with `{spawn, Command}` or `{spawn_executable, File}`,
//! over [`Programs`](crate::platform::Programs).
//!
//! A port is a process marked as a port (see [`Pid`]) running the embedded driver
//! `beamlet_port`, which turns the program's output into `{Port, {data, ...}}` messages (framing
//! lines and packets) and handles the messages Erlang code sends a port. Links, monitors, exit
//! signals and registered names are then those of processes. What only the VM can do is here:
//! starting the program, writing to it (`port_command/2`, so data is written before a
//! following `port_close/1` takes effect), closing it, and `port_info/1,2`.
//!
//! Starting programs is a capability the platform grants or not; without it `open_port/2`
//! fails with `eacces`. The console is a port too, as in BEAM: `{fd, 0, 1}` (or `{fd, 0, 2}`)
//! writes to it (console input reaches the `user` I/O server another way, see `beamlet_io`).
//! There are no port drivers: `{spawn_driver, _}` is `badarg`.

use alloc::string::String;
use alloc::vec::Vec;

use super::Ctx;
use crate::platform::{FileError, Program, ProgramEvent, Spawn};
use crate::process::Exception;
use crate::term::{Heap, Pid, Term};

type R = Result<Term, Exception>;

/// Most ports one VM may have open at once (`system_limit` beyond).
pub const MAX_PORTS: usize = 1024;

/// What the VM keeps for an open port.
pub struct PortState {
    /// The platform's handle for the program; `None` for the console.
    pub handle: Option<u64>,
    pub os_pid: Option<u64>,
    /// The connected process: the one that gets the port's messages.
    pub owner: Pid,
    /// The command or executable, for `port_info(Port, name)`.
    pub name: String,
    /// Bytes of length header on each packet (`{packet, N}`), else 0.
    pub packet: u8,
    /// Whether the port may be written to (not opened with only `in`).
    pub writable: bool,
    /// Bytes received from and sent to the program.
    pub input: u64,
    pub output: u64,
}

/// A port argument: a port, or the registered name of one.
fn port_arg(c: &Ctx, t: &Term) -> Option<Pid> {
    let pid = match t {
        Term::Pid(p) => *p,
        Term::Atom(name) => *c.sys().registered.get(name.as_str())?,
        _ => return None,
    };
    pid.port.then_some(pid)
}

/// An open port, or `badarg`.
fn open_port_arg(c: &Ctx, t: &Term) -> Result<Pid, Exception> {
    port_arg(c, t)
        .filter(|p| c.sys().ports.contains_key(p))
        .ok_or_else(|| c.badarg())
}

/// Text in a port setting: a string or a binary, as UTF-8.
fn text(heap: &Heap, t: &Term) -> Option<String> {
    let bytes = super::file::name_bytes(heap, *t)?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// How the driver frames the program's output.
enum Framing {
    Stream,
    Packet(u8),
    Line(i64),
}

pub fn open_port(c: &mut Ctx, a: &[Term]) -> R {
    if let Some(&[Term::Atom(k), Term::Int(0), Term::Int(out @ (1 | 2))]) = c.heap().as_tuple(a[0])
    {
        if k.as_str() == "fd" {
            return open_console(c, out, &a[1]);
        }
    }
    let Some(&[kind, what]) = c.heap().as_tuple(a[0]) else {
        return Err(c.badarg());
    };
    let shell = match kind {
        Term::Atom(k) if k.as_str() == "spawn" => true,
        Term::Atom(k) if k.as_str() == "spawn_executable" => false,
        _ => return Err(c.badarg()),
    };
    let h = c.heap();
    let name = text(h, &what)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| c.badarg())?;

    let mut framing = Framing::Stream;
    let (mut binary, mut eof, mut exit_status, mut stderr_to_stdout) = (false, false, false, false);
    let (mut input, mut output) = (false, false);
    let mut env: Vec<(String, Option<String>)> = Vec::new();
    let (mut cd, mut args, mut arg0) = (None, None, None);
    let opts = h.to_vec(a[1]).ok_or_else(|| c.badarg())?;
    for o in &opts {
        match *o {
            Term::Atom(o) => match o.as_str() {
                "stream" => framing = Framing::Stream,
                "binary" => binary = true,
                "eof" => eof = true,
                "exit_status" => exit_status = true,
                "stderr_to_stdout" => stderr_to_stdout = true,
                "in" => input = true,
                "out" => output = true,
                "use_stdio" | "hide" | "overlapped_io" => {}
                _ => return Err(c.badarg()),
            },
            Term::Tuple(_) => match *h.as_tuple(*o).expect("a tuple") {
                [Term::Atom(k), Term::Int(n)]
                    if k.as_str() == "packet" && matches!(n, 1 | 2 | 4) =>
                {
                    framing = Framing::Packet(n as u8)
                }
                [Term::Atom(k), Term::Int(n)] if k.as_str() == "line" && n > 0 => {
                    framing = Framing::Line(n)
                }
                [Term::Atom(k), dir] if k.as_str() == "cd" => {
                    cd = Some(text(h, &dir).ok_or_else(|| c.badarg())?)
                }
                [Term::Atom(k), list] if k.as_str() == "args" && !shell => {
                    let list = h.to_vec(list).ok_or_else(|| c.badarg())?;
                    args = Some(
                        list.iter()
                            .map(|t| text(h, t))
                            .collect::<Option<Vec<_>>>()
                            .ok_or_else(|| c.badarg())?,
                    );
                }
                [Term::Atom(k), s] if k.as_str() == "arg0" && !shell => {
                    arg0 = Some(text(h, &s).ok_or_else(|| c.badarg())?)
                }
                [Term::Atom(k), list] if k.as_str() == "env" => {
                    for pair in h.to_vec(list).ok_or_else(|| c.badarg())? {
                        let Some(&[k, v]) = h.as_tuple(pair) else {
                            return Err(c.badarg());
                        };
                        let k = text(h, &k)
                            .filter(|k| !k.is_empty() && !k.contains('='))
                            .ok_or_else(|| c.badarg())?;
                        // `false` (or `[]`) removes the variable.
                        let v = match v {
                            Term::Atom(f) if f.as_str() == "false" => None,
                            Term::Nil => None,
                            v => Some(text(h, &v).ok_or_else(|| c.badarg())?),
                        };
                        env.push((k, v));
                    }
                }
                [Term::Atom(k), _]
                    if matches!(
                        k.as_str(),
                        "parallelism" | "busy_limits_port" | "busy_limits_msgq"
                    ) => {}
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    if !input && !output {
        (input, output) = (true, true);
    }
    if c.sys().ports.len() >= MAX_PORTS {
        return Err(c.system_limit());
    }

    let resolve = |cwd: &str, name: &str| super::file::resolve(cwd, name.as_bytes());
    let program = if shell {
        Program::Shell(name.clone())
    } else {
        let cwd = c.sys().cwd.clone();
        let path = resolve(&cwd, &name).map_err(|e| posix(c, e))?;
        Program::Executable {
            path,
            arg0,
            args: args.unwrap_or_default(),
        }
    };
    let here = c.sys().cwd.clone();
    let cwd = match &cd {
        Some(dir) => resolve(&here, dir).map_err(|e| posix(c, e))?,
        None => here,
    };
    let mut vars = c.sys().env.clone();
    for (k, v) in env {
        match v {
            Some(v) => vars.insert(k, v),
            None => vars.remove(&k),
        };
    }
    // The port's `out` is the program's input; its `in` is the program's output.
    let spawn = Spawn {
        program,
        env: vars.into_iter().collect(),
        cwd,
        input: output,
        output: input,
        stderr_to_stdout,
    };

    let entry = driver(c)?;
    let spawned = c
        .sys()
        .platform
        .programs()
        .map_or(Err(FileError::Eacces), |programs| programs.spawn(&spawn));
    let spawned = spawned.map_err(|e| posix(c, e))?;
    let packet = if let Framing::Packet(n) = framing {
        n
    } else {
        0
    };
    let framing = match framing {
        Framing::Stream => c.atom("stream"),
        Framing::Packet(n) => {
            let tag = c.atom("packet");
            c.tuple(&[tag, Term::Int(n as i64)])
        }
        Framing::Line(n) => {
            let tag = c.atom("line");
            c.tuple(&[tag, Term::Int(n)])
        }
    };
    let st = PortState {
        handle: Some(spawned.handle),
        os_pid: spawned.os_pid,
        owner: c.p.pid,
        name,
        packet,
        writable: output,
        input: 0,
        output: 0,
    };
    let settings = settings_of(c, framing, binary, eof, exit_status);
    Ok(Term::Pid(start_driver(c, entry, settings, st)?))
}

/// The driver's settings: #{framing => stream | {packet, N} | {line, L}, binary, eof, exit_status}.
fn settings_of(c: &mut Ctx, framing: Term, binary: bool, eof: bool, exit_status: bool) -> Term {
    let pairs: Vec<(Term, Term)> = [
        ("framing", framing),
        ("binary", c.bool(binary)),
        ("eof", c.bool(eof)),
        ("exit_status", c.bool(exit_status)),
    ]
    .into_iter()
    .map(|(k, v)| (c.atom(k), v))
    .collect();
    c.map_from(pairs)
}

/// The code a port runs.
fn driver(c: &mut Ctx) -> Result<crate::process::Cp, Exception> {
    let (m, f) = {
        let mut sys = c.sys();
        (sys.atom("beamlet_port"), sys.atom("init"))
    };
    let found = c.sys().resolve(&m, &f, 1);
    match found {
        Some(crate::vm::Target::Code(cp)) => Ok(cp),
        _ => Err(c.badarg()),
    }
}

/// Start the process behind a port, linked to the caller, and record the port.
fn start_driver(
    c: &mut Ctx,
    entry: crate::process::Cp,
    settings: Term,
    st: PortState,
) -> Result<Pid, Exception> {
    // Spawned and set up under one lock, so no scheduler runs the port before it is ready.
    let mut sys = c.sys();
    let port = match sys.spawn_copy(entry, &c.p.heap, &[settings], true) {
        Ok(port) => port,
        Err(e) => {
            if let (Some(h), Some(programs)) = (st.handle, sys.platform.programs()) {
                programs.close(h);
            }
            return Err(e);
        }
    };
    // Linked to the process that opened it, as every port is.
    if let Some(p) = sys.procs.get_mut(port) {
        p.links.insert(c.p.pid);
        p.group_leader = c.p.group_leader;
    }
    if let Some(h) = st.handle {
        sys.program_ports.insert(h, port);
    }
    sys.ports.insert(port, st);
    drop(sys);
    c.p.links.insert(port);
    Ok(port)
}

/// `open_port({fd, 0, Out}, Opts)`: the console, for output (`stream` only).
fn open_console(c: &mut Ctx, out: i64, opts: &Term) -> R {
    let mut binary = false;
    for o in c.list_arg(*opts)? {
        match &o {
            Term::Atom(o) if o.as_str() == "binary" => binary = true,
            Term::Atom(o) if matches!(o.as_str(), "out" | "in" | "stream" | "eof" | "hide") => {}
            _ => return Err(c.badarg()),
        }
    }
    if c.sys().ports.len() >= MAX_PORTS {
        return Err(c.system_limit());
    }
    let entry = driver(c)?;
    let name = alloc::format!("0/{out}");
    let st = PortState {
        handle: None,
        os_pid: None,
        owner: c.p.pid,
        name,
        packet: 0,
        writable: true,
        input: 0,
        output: 0,
    };
    let stream = c.atom("stream");
    let settings = settings_of(c, stream, binary, false, false);
    Ok(Term::Pid(start_driver(c, entry, settings, st)?))
}

/// A failure to start a program: `error:Reason` with the POSIX name, as BEAM raises.
fn posix(c: &mut Ctx, e: FileError) -> Exception {
    Exception::error(c.atom(e.name()))
}

/// `port_command(Port, Data)`: write `Data` to the program, framed if the port uses packets.
pub fn port_command(c: &mut Ctx, a: &[Term]) -> R {
    let port = open_port_arg(c, &a[0])?;
    let data = c.heap().iodata_bytes(a[1]).ok_or_else(|| c.badarg())?;
    let mut sys = c.sys();
    let st = &sys.ports[&port];
    if !st.writable {
        return Err(c.badarg());
    }
    let (handle, packet) = (st.handle, st.packet);
    let mut framed = Vec::with_capacity(data.len() + packet as usize);
    if packet > 0 {
        let len = data.len() as u64;
        if packet < 8 && len >> (8 * packet as u32) != 0 {
            return Err(c.badarg());
        }
        framed.extend_from_slice(&len.to_be_bytes()[8 - packet as usize..]);
    }
    framed.extend_from_slice(&data);
    if let Some(st) = sys.ports.get_mut(&port) {
        st.output += framed.len() as u64;
    }
    match handle {
        None => sys.platform.console_write(&framed),
        // A program that has gone away takes no more input; the port learns that from its events.
        Some(h) => {
            if let Some(programs) = sys.platform.programs() {
                let _ = programs.write(h, &framed);
            }
        }
    }
    Ok(Term::Atom(c.atoms.true_))
}

/// `port_command(Port, Data, Options)`: `force` and `nosuspend` change nothing here (a port is
/// never busy), so this is `port_command/2`.
pub fn port_command3(c: &mut Ctx, a: &[Term]) -> R {
    for o in c.list_arg(a[2])? {
        if !matches!(&o, Term::Atom(o) if matches!(o.as_str(), "force" | "nosuspend")) {
            return Err(c.badarg());
        }
    }
    port_command(c, &a[..2])
}

/// Stop talking to the program and forget the port.
fn close(c: &mut Ctx, port: Pid) {
    let found = c.sys().ports.remove(&port).and_then(|st| st.handle);
    if let Some(h) = found {
        c.sys().program_ports.remove(&h);
        if let Some(programs) = c.sys().platform.programs() {
            programs.close(h);
        }
    }
}

/// `port_close(Port)`: close it now; its links get `{'EXIT', Port, normal}` and its monitors
/// `'DOWN'`, after any messages it has already sent.
pub fn port_close(c: &mut Ctx, a: &[Term]) -> R {
    let port = open_port_arg(c, &a[0])?;
    close(c, port);
    if port != c.p.pid {
        let normal = alloc::sync::Arc::new(crate::term::OwnedTerm::immediate(Term::Atom(
            c.atoms.normal,
        )));
        c.sys().exits.push_back(crate::vm::ExitSignal {
            target: port,
            from: c.p.pid,
            reason: normal,
            from_link: false,
            forced: true,
        });
        // As `exit/2`: the port is gone before the caller runs again.
        c.p.budget = c.p.budget.min(1);
    }
    Ok(Term::Atom(c.atoms.true_))
}

/// `port_connect(Port, Pid)`: `Pid` becomes the connected process, and is linked to the port.
pub fn port_connect(c: &mut Ctx, a: &[Term]) -> R {
    let port = open_port_arg(c, &a[0])?;
    let Term::Pid(new) = a[1] else {
        return Err(c.badarg());
    };
    if new.port || !(new == c.p.pid || c.sys().procs.is_alive(new)) {
        return Err(c.badarg());
    }
    if let Some(st) = c.sys().ports.get_mut(&port) {
        st.owner = new;
    }
    if new == c.p.pid {
        c.p.links.insert(port);
    } else {
        c.sys().procs.update(new, move |p| {
            p.links.insert(port);
        });
    }
    if port == c.p.pid {
        c.p.links.insert(new);
    } else {
        c.sys().procs.update(port, move |p| {
            p.links.insert(new);
        });
    }
    Ok(Term::Atom(c.atoms.true_))
}

/// `port_control/3` and `port_call/2,3`: only drivers answer these, and there are none.
pub fn no_driver(c: &mut Ctx, _a: &[Term]) -> R {
    Err(c.badarg())
}

/// `erlang:ports()`.
pub fn ports(c: &mut Ctx, _a: &[Term]) -> R {
    let ports: Vec<Term> = c.sys().ports.keys().map(|&p| Term::Pid(p)).collect();
    Ok(c.list(ports))
}

const INFO_ITEMS: [&str; 7] = [
    "name",
    "links",
    "id",
    "connected",
    "input",
    "output",
    "os_pid",
];

fn info_item(c: &mut Ctx, port: Pid, item: &str) -> Option<Term> {
    let mut sys = c.sys();
    let st = sys.ports.get(&port)?;
    let (owner, id, input, output, os_pid, name) = (
        st.owner,
        port.serial,
        st.input,
        st.output,
        st.os_pid,
        st.name.clone(),
    );
    // The port's own process, unless it is the one asking.
    let (links, watchers): (Vec<Pid>, Vec<Pid>) = {
        let p: &crate::process::Process = if port == c.p.pid {
            c.p
        } else {
            sys.procs.get_mut(port)?
        };
        (
            p.links.iter().copied().collect(),
            p.monitored_by.values().map(|m| m.watcher).collect(),
        )
    };
    drop(sys);
    let pids =
        |c: &mut Ctx, set: Vec<Pid>| c.list(set.into_iter().map(Term::Pid).collect::<Vec<_>>());
    let value = match item {
        "name" => c.string(&name),
        "id" => Term::Int(id as i64),
        "connected" => Term::Pid(owner),
        "input" => Term::Int(input as i64),
        "output" => Term::Int(output as i64),
        "os_pid" => match os_pid {
            Some(n) => Term::Int(n as i64),
            None => c.atom("undefined"),
        },
        "links" => pids(c, links),
        "monitored_by" => pids(c, watchers),
        "monitors" => Term::Nil,
        "registered_name" => {
            let name = c
                .sys()
                .registered
                .iter()
                .find(|(_, &p)| p == port)
                .map(|(n, _)| String::from(n.as_str()));
            match name {
                Some(n) => c.atom(&n),
                None => Term::Nil,
            }
        }
        "queue_size" | "memory" => Term::Int(0),
        "parallelism" => c.bool(false),
        "locking" => c.atom("port_level"),
        _ => return None,
    };
    Some(value)
}

/// `port_info(Port)`: the usual items, or `undefined` once it is closed.
pub fn port_info1(c: &mut Ctx, a: &[Term]) -> R {
    let port = port_arg(c, &a[0]).ok_or_else(|| c.badarg())?;
    if !c.sys().ports.contains_key(&port) {
        return Ok(c.atom("undefined"));
    }
    if c.sys().procs.is_running(port) && port != c.p.pid {
        return c.retry();
    }
    let mut items = Vec::new();
    // A registered port gives its name first, as BEAM's does.
    if let Some(name) = info_item(c, port, "registered_name").filter(|n| !matches!(n, Term::Nil)) {
        let tag = c.atom("registered_name");
        items.push(c.tuple(&[tag, name]));
    }
    for item in INFO_ITEMS {
        if item == "os_pid" && c.sys().ports[&port].os_pid.is_none() {
            continue;
        }
        // Every item of an open port is there, unless another scheduler just started running
        // it (or it closed): then ask again later.
        let Some(v) = info_item(c, port, item) else {
            return c.retry();
        };
        let tag = c.atom(item);
        items.push(c.tuple(&[tag, v]));
    }
    Ok(c.list(items))
}

/// `port_info(Port, Item)`: `{Item, Value}`, or `undefined` once it is closed.
pub fn port_info2(c: &mut Ctx, a: &[Term]) -> R {
    let port = port_arg(c, &a[0]).ok_or_else(|| c.badarg())?;
    let Term::Atom(item) = &a[1] else {
        return Err(c.badarg());
    };
    let item = *item;
    if !c.sys().ports.contains_key(&port) {
        return Ok(c.atom("undefined"));
    }
    if c.sys().procs.is_running(port) && port != c.p.pid {
        return c.retry();
    }
    match info_item(c, port, item.as_str()) {
        // A port with no registered name answers `[]`, as BEAM's does.
        Some(v) => Ok(c.tuple(&[Term::Atom(item), v])),
        None if INFO_ITEMS.contains(&item.as_str()) || item.as_str() == "registered_name" => {
            c.retry()
        }
        None => Err(c.badarg()),
    }
}

/// `port_to_list(Port)`: `"#Port<0.N>"`.
pub fn port_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Pid(p) = &a[0] else {
        return Err(c.badarg());
    };
    if !p.port {
        return Err(c.badarg());
    }
    let s = alloc::format!("#Port<0.{}>", p.serial);
    Ok(c.string(&s))
}

/// `list_to_port("#Port<0.N>")`: an open port with that number, or one that no longer
/// exists (which behaves as a closed port).
pub fn list_to_port(c: &mut Ctx, a: &[Term]) -> R {
    let s = text(c.heap(), &a[0]).ok_or_else(|| c.badarg())?;
    let n: u32 = s
        .strip_prefix("#Port<0.")
        .and_then(|r| r.strip_suffix('>'))
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| c.badarg())?;
    let found = c.sys().ports.keys().find(|p| p.serial == n).copied();
    // A closed port's slot is unknown; any index names no live process.
    Ok(Term::Pid(found.unwrap_or(Pid {
        serial: n,
        index: u32::MAX,
        port: true,
    })))
}

impl crate::vm::System {
    /// Pass what programs have done to their ports' drivers, as
    /// `{'$beamlet_program', {data, Bytes} | eof | {exit_status, N}}`.
    pub(crate) fn poll_programs(&mut self) {
        if self.program_ports.is_empty() {
            return;
        }
        while let Some((handle, event)) = self.platform.programs().and_then(|p| p.poll()) {
            let Some(&port) = self.program_ports.get(&handle) else {
                continue;
            };
            if let (ProgramEvent::Output(bytes), Some(st)) = (&event, self.ports.get_mut(&port)) {
                st.input += bytes.len() as u64;
            }
            let [data, eof, exit_status, tag] = ["data", "eof", "exit_status", "$beamlet_program"]
                .map(|n| Term::Atom(self.atom(n)));
            self.send_with(port, |h| {
                let event = match &event {
                    ProgramEvent::Output(bytes) => {
                        let b = h.binary(bytes);
                        h.tuple(&[data, b])
                    }
                    ProgramEvent::Eof => eof,
                    ProgramEvent::Exit(n) => h.tuple(&[exit_status, Term::Int(*n as i64)]),
                };
                h.tuple(&[tag, event])
            });
        }
    }

    /// A port's process has ended: close its program.
    pub(crate) fn port_ended(&mut self, port: Pid) {
        if let Some(h) = self.ports.remove(&port).and_then(|st| st.handle) {
            self.program_ports.remove(&h);
            if let Some(programs) = self.platform.programs() {
                programs.close(h);
            }
        }
    }
}
