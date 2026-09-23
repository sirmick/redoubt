//! The trace format: a sequence of kernel events and their expected results, as ASCII text.
//! README.md ("Trace format") is the specification; this module writes and reads it, and
//! [`check`] replays a trace on the model and requires identical output.
//!
//! Everything here is `no_std` + `alloc` and works on `&str`, so a kernel-side replayer can use
//! [`tokens`] and [`Token`] directly.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::kernel::{Boot, Costs, DeviceSpec, Kernel, Limits, Note, Step};
use crate::mutation::Mutation;
use crate::spec::*;
use crate::syscall::*;

pub const HEADER: &str = "redoubt-model-trace 2";

// ---------------------------------------------------------------------------------------------
// Tokens.

/// Kinds of names: values the kernel chooses, which a replayer binds to its own values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// `h:N`: a handle index in the process the line is about.
    Handle,
    /// `a:BASE+OFF`: an address `OFF` bytes into a region whose base the kernel returned.
    Addr,
    /// `p:N`, `t:N`, `m:N`, `pa:N`: pid, tid, message id, physical address.
    Pid,
    Tid,
    Msg,
    Phys,
    /// `tm:N`: a time the kernel read (`time_now`); compared only for monotonicity.
    Time,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    /// A literal number, decimal or `0x` hex.
    Int(u64),
    Name {
        kind: NameKind,
        value: u64,
        offset: u64,
    },
    /// `-`: none.
    None,
    /// `forever`: `FOREVER`.
    Forever,
    /// `[x,y,...]`: a list of simple tokens.
    List(Vec<Token<'a>>),
    /// `BASE@PAGES`: a lend or transfer range; `BASE` is a number or a name.
    Range(alloc::boxed::Box<Token<'a>>, u64),
    /// `key=value`: the value is a simple token or a list.
    Field(&'a str, alloc::boxed::Box<Token<'a>>),
    /// Anything else: a call name, a keyword, an error name.
    Word(&'a str),
}

fn int(s: &str) -> Option<u64> {
    if let Some(h) = s.strip_prefix("0x") { u64::from_str_radix(h, 16).ok() } else { s.parse().ok() }
}

/// A simple token: `-`, `forever`, a number, a name or a word. Never a list, field or range.
fn simple(s: &str) -> Result<Token<'_>, String> {
    if s == "-" {
        return Ok(Token::None);
    }
    if s == "forever" {
        return Ok(Token::Forever);
    }
    if s.contains(['[', ']', ',', '=', '@']) {
        return Err(format!("`{s}`: lists, fields and ranges do not nest"));
    }
    if let Some(n) = int(s) {
        return Ok(Token::Int(n));
    }
    for (prefix, kind) in [
        ("pa:", NameKind::Phys),
        ("tm:", NameKind::Time),
        ("h:", NameKind::Handle),
        ("a:", NameKind::Addr),
        ("p:", NameKind::Pid),
        ("t:", NameKind::Tid),
        ("m:", NameKind::Msg),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let (v, off) = match rest.split_once('+') {
                Some((v, o)) => (v, int(o).ok_or_else(|| format!("bad offset in {s}"))?),
                None => (rest, 0),
            };
            let value = int(v).ok_or_else(|| format!("bad name {s}"))?;
            value.checked_add(off).ok_or_else(|| format!("{s} overflows"))?;
            return Ok(Token::Name { kind, value, offset: off });
        }
    }
    Ok(Token::Word(s))
}

fn list_token(s: &str) -> Option<Result<Token<'_>, String>> {
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return Some(Ok(Token::List(Vec::new())));
    }
    Some(inner.split(',').map(simple).collect::<Result<Vec<_>, _>>().map(Token::List))
}

/// One token. Nesting is fixed and shallow (a field's value may be a list; a list holds simple
/// tokens), so a hostile line cannot make the parser recurse.
pub fn token(s: &str) -> Result<Token<'_>, String> {
    if let Some(l) = list_token(s) {
        return l;
    }
    if let Some((k, v)) = s.split_once('=') {
        let v = match list_token(v) {
            Some(l) => l?,
            None => simple(v)?,
        };
        return Ok(Token::Field(k, alloc::boxed::Box::new(v)));
    }
    if let Some((base, pages)) = s.split_once('@') {
        let pages = int(pages).ok_or_else(|| format!("bad page count in {s}"))?;
        return Ok(Token::Range(alloc::boxed::Box::new(simple(base)?), pages));
    }
    simple(s)
}

/// A line's tokens, split at single spaces. `->` separates an event from its result.
pub fn tokens(line: &str) -> Result<Vec<Token<'_>>, String> {
    line.split(' ').filter(|t| !t.is_empty()).map(token).collect()
}

// ---------------------------------------------------------------------------------------------
// Writing.

/// Formats values, remembering the address regions the kernel returned to each process so that
/// addresses inside them are written relative to their base.
#[derive(Default)]
struct Names {
    /// pid -> (base -> pages).
    regions: BTreeMap<u64, BTreeMap<u64, u64>>,
}

impl Names {
    /// A handle index is a name; 0 ("no handle") and values that cannot be indices are
    /// literals (they fail at decoding, the same on every kernel).
    fn handle(v: u64) -> String {
        if v != NO_HANDLE && v <= U32_MAX { format!("h:{v}") } else { format!("{v}") }
    }

    fn addr(&self, pid: u64, a: u64) -> String {
        if let Some(r) = self.regions.get(&pid) {
            if let Some((base, pages)) = r.range(..=a).next_back() {
                if a - base < pages * PAGE_SIZE {
                    return format!("a:{base:#x}+{:#x}", a - base);
                }
            }
        }
        format!("{a:#x}")
    }

    fn bind(&mut self, pid: u64, base: u64, pages: u64) {
        self.regions.entry(pid).or_default().insert(base, pages);
    }

    fn list(v: impl IntoIterator<Item = String>) -> String {
        let v: Vec<String> = v.into_iter().collect();
        format!("[{}]", v.join(","))
    }

    fn time(t: u64) -> String { if t == FOREVER { "forever".to_string() } else { format!("{t}") } }

    fn call(&self, pid: u64, c: &Syscall) -> String {
        use Syscall as S;
        let a = |x: u64| self.addr(pid, x);
        let h = Names::handle;
        let hs = |v: &[u64]| Names::list(v.iter().map(|x| h(*x)));
        let words = |w: &[u64; WORDS]| Names::list(w.iter().map(|x| format!("{x}")));
        let range = |b: &Option<Buffer>| b.map_or("-".to_string(), |b| format!("{}@{}", a(b.addr), b.npages));
        let args = match c {
            S::MapAnon { len, flags } => format!("{len:#x} {flags}"),
            S::Unmap { addr, len } => format!("{} {len:#x}", a(*addr)),
            S::SetFlags { addr, len, flags } => format!("{} {len:#x} {flags}", a(*addr)),
            S::MapDevice { h: x } => h(*x),
            S::DmaAlloc { h: x, npages } => format!("{} {npages}", h(*x)),
            S::ThreadCreate { entry, sp, arg } => format!("{entry:#x} {sp:#x} {arg}"),
            S::ThreadExit | S::EndpointCreate | S::TimeNow | S::Random => String::new(),
            S::ProcessExit { code } => format!("{code}"),
            S::ProcessCreate { budget, exit_endpoint } => format!("{} {}", h(*budget), h(*exit_endpoint)),
            S::ProcessMap { process, src, dst, len, flags } => {
                format!("{} {} {} {len:#x} {flags}", h(*process), a(*src), a(*dst))
            }
            S::ProcessStart { process, entry, sp, arg, handles } => {
                format!("{} {entry:#x} {sp:#x} {} {}", h(*process), a(*arg), hs(handles))
            }
            S::Mint { source, badge, budget } => {
                let src = match source {
                    MintSource::Message(m) => format!("m:{m}"),
                    MintSource::Handle(x) => h(*x),
                };
                format!("{src} {badge} {}", budget.map_or("-".to_string(), h))
            }
            S::Call { h: x, words: w, handles, lend, timeout } => {
                format!("{} {} {} {} {}", h(*x), words(w), hs(handles), range(lend), Names::time(*timeout))
            }
            S::Send { h: x, words: w, handles, transfer, timeout } => {
                format!(
                    "{} {} {} {} {}",
                    h(*x),
                    words(w),
                    hs(handles),
                    range(transfer),
                    Names::time(*timeout)
                )
            }
            S::Receive { h: x, timeout, max_transfer } => {
                format!("{} {} {max_transfer}", x.map_or("-".to_string(), h), Names::time(*timeout))
            }
            S::Reply { msg_id, words: w, handles } => format!("m:{msg_id} {} {}", words(w), hs(handles)),
            S::Serve { msg_id } => format!("m:{msg_id}"),
            S::HandleClose { h: x } | S::BudgetDestroy { h: x } | S::BudgetUsage { h: x } => h(*x),
            S::BudgetCreate { parent, pages, processes, weight, labels, account, deadline } => {
                format!(
                    "{} {pages} {processes} {weight} {} {account} {}",
                    h(*parent),
                    Names::list(labels.iter().map(|l| format!("{l}"))),
                    Names::time(*deadline)
                )
            }
            S::SystemReset { h: x, kind } => format!("{} {kind}", h(*x)),
        };
        if args.is_empty() { c.name().to_string() } else { format!("{} {args}", c.name()) }
    }

    /// A result, binding any address region it hands to `pid` (`call` gives the length of a
    /// `map_anon` region).
    fn result(&mut self, pid: u64, call: Option<&Syscall>, r: &Result<Ret, Error>, k: &Kernel) -> String {
        let r = match r {
            Err(e) => return format!("err {}", e.name()),
            Ok(r) => r,
        };
        let h = Names::handle;
        match r {
            Ret::Unit => "ok".into(),
            Ret::Addr(a) => {
                let pages = match call {
                    Some(Syscall::MapAnon { len, .. }) => len / PAGE_SIZE,
                    Some(Syscall::MapDevice { h: x }) => device_pages(k, pid, *x),
                    _ => 1,
                };
                self.bind(pid, *a, pages);
                format!("ok a:{a:#x}")
            }
            Ret::AddrPhys { addr, phys } => {
                let pages = match call {
                    Some(Syscall::DmaAlloc { npages, .. }) => *npages,
                    _ => 1,
                };
                self.bind(pid, *addr, pages);
                format!("ok a:{addr:#x} pa:{phys:#x}")
            }
            Ret::Tid(t) => format!("ok t:{t}"),
            Ret::Handle(x) => format!("ok {}", h(*x)),
            Ret::Call(c) => {
                let status = c.status.map_or_else(|e| e.name(), |_| "ok");
                let lend = match c.lend {
                    LendDisposition::None => "none",
                    LendDisposition::Returned => "returned",
                    LendDisposition::Consumed => "consumed",
                };
                let reply = c.reply.as_ref().map_or_else(
                    || "absent".to_string(),
                    |r| {
                        format!(
                            "present words={} handles={}",
                            Names::list(r.words.iter().map(|w| format!("{w}"))),
                            Names::list(r.handles.iter().map(|x| h(*x)))
                        )
                    },
                );
                format!("call status={status} lend={lend} reply={reply}")
            }
            Ret::Replied { delivered, installed_mask } => format!(
                "ok reply delivery={} mask={installed_mask}",
                if *delivered { "delivered" } else { "discarded" }
            ),
            Ret::Message(m) => {
                let buffer = match m.buffer {
                    None => "-".to_string(),
                    Some(b) => {
                        self.bind(pid, b.addr, b.pages);
                        let kind = if b.kind == BufferKind::Lend { "lend" } else { "transfer" };
                        format!("[{kind},a:{:#x},{}]", b.addr, b.pages)
                    }
                };
                format!(
                    "ok message {} m:{} badge={} account={} labels={} words={} handles={} buffer={buffer}",
                    if m.kind == MsgKind::Call { "call" } else { "send" },
                    m.msg_id,
                    m.badge,
                    m.account,
                    Names::list(m.labels.iter().map(|l| format!("{l}"))),
                    Names::list(m.words.iter().map(|w| format!("{w}"))),
                    Names::list(m.handles.iter().map(|x| h(*x)))
                )
            }
            Ret::Interrupt { h: x } => format!("ok interrupt {}", h(*x)),
            Ret::ExitNotice { pid, cause, code, blamed_account, blamed_labels } => format!(
                "ok exit p:{pid} cause={} code={code} blamed={blamed_account} blamed_labels={}",
                cause.name(),
                Names::list(blamed_labels.iter().map(|l| format!("{l}")))
            ),
            Ret::Abandoned { msg_id } => format!("ok abandoned m:{msg_id}"),
            Ret::Usage(c) => {
                format!(
                    "ok usage [{},{},{},{},{},{}]",
                    c.pages_limit,
                    c.pages_usage,
                    c.processes_limit,
                    c.processes_usage,
                    c.weight_limit,
                    c.weight_usage
                )
            }
            Ret::Time(t) => format!("ok time tm:{t}"),
            Ret::Random => "ok random".into(),
            Ret::Word(w) => format!("ok word {w}"),
        }
    }
}

fn device_pages(k: &Kernel, pid: u64, h: u64) -> u64 {
    use crate::kernel::{DeviceKind, Object};
    match k.processes.get(&pid).and_then(|p| p.handles.get(&h)).map(|x| x.object) {
        Some(Object::Device(d)) => match k.devices[&d].kind {
            DeviceKind::Mmio { pages, .. } => pages,
            _ => 1,
        },
        _ => 1,
    }
}

fn limits(l: Limits) -> String { format!("[{},{},{}]", l.pages, l.processes, l.weight) }

fn boot_lines(boot: &Boot) -> Vec<String> {
    let c = boot.costs;
    let mut out = alloc::vec![
        HEADER.to_string(),
        format!(
            "boot root={} system={} users={}",
            limits(boot.root),
            limits(boot.system),
            limits(boot.users)
        ),
        format!(
            "costs budget={} process={} contexts={} thread={} endpoint={} handles_per_page={} page_table={} open_call={}",
            c.budget,
            c.process,
            c.contexts,
            c.thread,
            c.endpoint,
            c.handles_per_page,
            c.page_table,
            c.open_call
        ),
    ];
    for d in &boot.devices {
        out.push(match d {
            DeviceSpec::Mmio { base, pages, dma } => {
                format!("device mmio base={base:#x} pages={pages} dma={}", *dma as u8)
            }
            DeviceSpec::Irq { n } => format!("device irq n={n}"),
            DeviceSpec::Reset => "device reset".to_string(),
        });
    }
    out
}

/// Run `ops` on a fresh model and write the trace. Ops the model rejects as illegal events are
/// left out (so a shrunk sequence records cleanly). `Err` if `boot` is not a valid boot.
pub fn record(boot: &Boot, ops: &[Op], mutation: Option<Mutation>) -> Result<String, String> {
    let mut k = Kernel::boot(boot, mutation)?;
    let mut names = Names::default();
    let mut lines = boot_lines(boot);
    let init_tid = *k.processes[&crate::kernel::INIT_PID].threads.first().unwrap();
    lines.push(format!("start p:{} t:{init_tid}", crate::kernel::INIT_PID));
    for op in ops {
        if k.unsupported_receive_output(op) {
            return Err("question 171: late-invalid receive output is outside the model oracle".into());
        }
        let Some(step) = k.step(op) else { continue };
        lines.extend(step_lines(&mut names, op, &step, &k));
    }
    let mut s = lines.join("\n");
    s.push('\n');
    Ok(s)
}

fn step_lines(names: &mut Names, op: &Op, step: &Step, after: &Kernel) -> Vec<String> {
    let mut out = Vec::new();
    let outcome = |names: &mut Names, pid: u64, call: Option<&Syscall>, o: &Outcome| match o {
        Outcome::Done(r) => names.result(pid, call, r, after),
        Outcome::Blocked => "blocked".to_string(),
        Outcome::Gone => "gone".to_string(),
    };
    let head = match op {
        Op::Record { pid, tid, record } => format!(
            "record p:{pid} t:{tid} {}",
            match record {
                Record::Owned => "owned".into(),
                Record::Unmapped => "unmapped".into(),
                Record::ReadOnly => "readonly".into(),
                Record::Borrowed => "borrowed".into(),
                Record::Device => "device".into(),
                Record::CopyFault => "copyfault".into(),
                Record::Memory(a) => format!("{a}"),
            }
        ),
        Op::Sys { pid, tid, call } => {
            let c = names.call(*pid, call);
            format!("do p:{pid} t:{tid} {c} -> {}", outcome(names, *pid, Some(call), &step.outcome))
        }
        Op::Write { pid, tid, addr, value } => {
            let o = if step.outcome == Outcome::Gone { "fault".to_string() } else { "ok".to_string() };
            format!("write p:{pid} t:{tid} {} {value} -> {o}", names.addr(*pid, *addr))
        }
        Op::Read { pid, tid, addr } => {
            let o = match &step.outcome {
                Outcome::Done(Ok(Ret::Word(w))) => format!("ok word {w}"),
                _ => "fault".to_string(),
            };
            format!("read p:{pid} t:{tid} {} -> {o}", names.addr(*pid, *addr))
        }
        Op::Exec { pid, tid, addr } => {
            let o = if step.outcome == Outcome::Gone { "fault" } else { "ok" };
            format!("exec p:{pid} t:{tid} {} -> {o}", names.addr(*pid, *addr))
        }
        Op::Fault { pid, tid } => format!("fault p:{pid} t:{tid}"),
        Op::Irq { n } => format!("irq {n}"),
        Op::Tick { dt } => format!("tick {dt}"),
    };
    out.push(head);
    for n in &step.notes {
        out.push(match n {
            Note::Process { creator, h, pid } => format!("note process p:{creator} h:{h} p:{pid}"),
            Note::Thread { pid, tid } => format!("note thread p:{pid} t:{tid}"),
        });
    }
    for w in &step.wakes {
        let r = names.result(w.pid, None, &w.result, after);
        out.push(format!("wake p:{} t:{} -> {r}", w.pid, w.tid));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Reading.

/// A model value for a name or literal token (the model's names are its own values).
fn value(t: &Token) -> Result<u64, String> {
    match t {
        Token::Int(v) => Ok(*v),
        Token::Name { value, offset, .. } => Ok(value + offset),
        Token::Forever => Ok(FOREVER),
        t => Err(format!("expected a value, got {t:?}")),
    }
}

fn list(t: &Token) -> Result<Vec<u64>, String> {
    match t {
        Token::List(v) => v.iter().map(value).collect(),
        t => Err(format!("expected a list, got {t:?}")),
    }
}

fn words(t: &Token) -> Result<[u64; WORDS], String> {
    let v = list(t)?;
    v.try_into().map_err(|_| "expected WORDS words".to_string())
}

fn range(t: &Token) -> Result<Option<Buffer>, String> {
    match t {
        Token::None => Ok(None),
        Token::Range(base, npages) => Ok(Some(Buffer { addr: value(base)?, npages: *npages })),
        t => Err(format!("expected a range, got {t:?}")),
    }
}

/// The system call on a `do` line (tokens after `do p:N t:N`, up to `->`).
pub fn parse_call(t: &[Token]) -> Result<Syscall, String> {
    use Syscall as S;
    let Some(Token::Word(name)) = t.first() else { return Err("missing call name".into()) };
    let a = &t[1..];
    let n = |i: usize| a.get(i).ok_or_else(|| format!("{name}: missing argument {i}"));
    let v = |i: usize| n(i).and_then(value);
    let opt = |i: usize| match n(i)? {
        Token::None => Ok(None),
        t => value(t).map(Some),
    };
    Ok(match *name {
        "map_anon" => S::MapAnon { len: v(0)?, flags: v(1)? },
        "unmap" => S::Unmap { addr: v(0)?, len: v(1)? },
        "set_flags" => S::SetFlags { addr: v(0)?, len: v(1)?, flags: v(2)? },
        "map_device" => S::MapDevice { h: v(0)? },
        "dma_alloc" => S::DmaAlloc { h: v(0)?, npages: v(1)? },
        "thread_create" => S::ThreadCreate { entry: v(0)?, sp: v(1)?, arg: v(2)? },
        "thread_exit" => S::ThreadExit,
        "process_exit" => S::ProcessExit { code: v(0)? },
        "process_create" => S::ProcessCreate { budget: v(0)?, exit_endpoint: v(1)? },
        "process_map" => S::ProcessMap { process: v(0)?, src: v(1)?, dst: v(2)?, len: v(3)?, flags: v(4)? },
        "process_start" => {
            S::ProcessStart { process: v(0)?, entry: v(1)?, sp: v(2)?, arg: v(3)?, handles: list(n(4)?)? }
        }
        "endpoint_create" => S::EndpointCreate,
        "mint" => {
            let source = match n(0)? {
                Token::Name { kind: NameKind::Msg, value, .. } => MintSource::Message(*value),
                t => MintSource::Handle(value(t)?),
            };
            S::Mint { source, badge: v(1)?, budget: opt(2)? }
        }
        "call" => S::Call {
            h: v(0)?,
            words: words(n(1)?)?,
            handles: list(n(2)?)?,
            lend: range(n(3)?)?,
            timeout: v(4)?,
        },
        "send" => S::Send {
            h: v(0)?,
            words: words(n(1)?)?,
            handles: list(n(2)?)?,
            transfer: range(n(3)?)?,
            timeout: v(4)?,
        },
        "receive" => S::Receive { h: opt(0)?, timeout: v(1)?, max_transfer: v(2)? },
        "reply" => S::Reply { msg_id: v(0)?, words: words(n(1)?)?, handles: list(n(2)?)? },
        "serve" => S::Serve { msg_id: v(0)? },
        "handle_close" => S::HandleClose { h: v(0)? },
        "budget_create" => S::BudgetCreate {
            parent: v(0)?,
            pages: v(1)?,
            processes: v(2)?,
            weight: v(3)?,
            labels: list(n(4)?)?,
            account: v(5)?,
            deadline: v(6)?,
        },
        "budget_destroy" => S::BudgetDestroy { h: v(0)? },
        "budget_usage" => S::BudgetUsage { h: v(0)? },
        "time_now" => S::TimeNow,
        "random" => S::Random,
        "system_reset" => S::SystemReset { h: v(0)?, kind: v(1)? },
        other => return Err(format!("unknown call {other}")),
    })
}

fn limits_field(t: &[Token], key: &str) -> Result<Limits, String> {
    let v = t
        .iter()
        .find_map(|x| match x {
            Token::Field(k, v) if *k == key => Some(list(v)),
            _ => None,
        })
        .unwrap_or_else(|| Err(format!("missing {key}=")))?;
    match v[..] {
        [pages, processes, weight] => Ok(Limits { pages, processes, weight }),
        _ => Err(format!("{key}= needs [pages,processes,weight]")),
    }
}

fn field<'a>(t: &'a [Token<'a>], key: &str) -> Result<u64, String> {
    t.iter()
        .find_map(|x| match x {
            Token::Field(k, v) if *k == key => Some(value(v)),
            _ => None,
        })
        .unwrap_or_else(|| Err(format!("missing {key}=")))
}

/// Parse a trace into its boot configuration and the events it contains (results are not
/// needed to replay on the model: [`check`] regenerates and compares them).
pub fn parse(text: &str) -> Result<(Boot, Vec<Op>), String> {
    let mut lines = text.lines().filter(|l| !l.is_empty() && !l.starts_with('#'));
    if lines.next() != Some(HEADER) {
        return Err(format!("not a trace: the first line must be `{HEADER}`"));
    }
    let mut boot = Boot { devices: Vec::new(), ..Boot::default() };
    let mut ops = Vec::new();
    for (i, line) in lines.enumerate() {
        let (event, _) = line.split_once(" -> ").unwrap_or((line, ""));
        let t = tokens(event).map_err(|e| format!("line {}: {e}", i + 2))?;
        let err = |e: String| format!("line {}: {e}", i + 2);
        let pt = |j: usize| {
            t.get(j).ok_or_else(|| err("missing pid or tid".into())).and_then(|x| value(x).map_err(err))
        };
        match t.first() {
            Some(Token::Word("boot")) => {
                boot.root = limits_field(&t, "root")?;
                boot.system = limits_field(&t, "system")?;
                boot.users = limits_field(&t, "users")?;
            }
            Some(Token::Word("costs")) => {
                boot.costs = Costs {
                    budget: field(&t, "budget")?,
                    process: field(&t, "process")?,
                    contexts: field(&t, "contexts")?,
                    thread: field(&t, "thread")?,
                    endpoint: field(&t, "endpoint")?,
                    handles_per_page: field(&t, "handles_per_page")?,
                    page_table: field(&t, "page_table")?,
                    open_call: field(&t, "open_call")?,
                }
            }
            Some(Token::Word("device")) => boot.devices.push(match t.get(1) {
                Some(Token::Word("mmio")) => DeviceSpec::Mmio {
                    base: field(&t, "base")?,
                    pages: field(&t, "pages")?,
                    dma: field(&t, "dma")? != 0,
                },
                Some(Token::Word("irq")) => DeviceSpec::Irq { n: field(&t, "n")? },
                Some(Token::Word("reset")) => DeviceSpec::Reset,
                _ => return Err(err("unknown device".into())),
            }),
            Some(Token::Word("record")) => {
                let r = match t.get(3) {
                    Some(Token::Word("owned")) => Record::Owned,
                    Some(Token::Word("unmapped")) => Record::Unmapped,
                    Some(Token::Word("readonly")) => Record::ReadOnly,
                    Some(Token::Word("borrowed")) => Record::Borrowed,
                    Some(Token::Word("device")) => Record::Device,
                    Some(Token::Word("copyfault")) => Record::CopyFault,
                    Some(x) => Record::Memory(value(x)?),
                    _ => return Err("missing record validity".into()),
                };
                ops.push(Op::Record { pid: value(&t[1])?, tid: value(&t[2])?, record: r });
            }
            Some(Token::Word("do")) => {
                ops.push(Op::Sys { pid: pt(1)?, tid: pt(2)?, call: parse_call(&t[3..]).map_err(err)? })
            }
            Some(Token::Word("write")) => {
                ops.push(Op::Write { pid: pt(1)?, tid: pt(2)?, addr: pt(3)?, value: pt(4)? })
            }
            Some(Token::Word("read")) => ops.push(Op::Read { pid: pt(1)?, tid: pt(2)?, addr: pt(3)? }),
            Some(Token::Word("exec")) => ops.push(Op::Exec { pid: pt(1)?, tid: pt(2)?, addr: pt(3)? }),
            Some(Token::Word("fault")) => ops.push(Op::Fault { pid: pt(1)?, tid: pt(2)? }),
            Some(Token::Word("irq")) => ops.push(Op::Irq { n: pt(1)? }),
            Some(Token::Word("tick")) => ops.push(Op::Tick { dt: pt(1)? }),
            Some(Token::Word("start" | "note" | "wake")) => {}
            _ => return Err(err(format!("unknown record `{line}`"))),
        }
    }
    Ok((boot, ops))
}

/// Replay a trace on the model and require exactly the same text back: every result, wake and
/// note. `Ok` means the trace is what the model (with `mutation`) does.
pub fn check(text: &str, mutation: Option<Mutation>) -> Result<(), String> {
    let (boot, ops) = parse(text)?;
    let again = record(&boot, &ops, mutation)?;
    let want: Vec<&str> = text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let got: Vec<&str> = again.lines().collect();
    for (i, (w, g)) in want.iter().zip(got.iter()).enumerate() {
        if w != g {
            return Err(format!("record {}: trace says\n  {w}\nthe model says\n  {g}", i + 1));
        }
    }
    if want.len() != got.len() {
        return Err(format!("the trace has {} records, the model {}", want.len(), got.len()));
    }
    Ok(())
}
