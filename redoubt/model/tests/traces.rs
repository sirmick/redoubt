//! The trace format: what the model writes, it reads back and replays to the same text; a
//! trace from a kernel that breaks a rule does not replay on the specified model; and the example
//! trace in `traces/` is what the model does.

mod common;

use redoubt_model::check;
use redoubt_model::gen::Gen;
use redoubt_model::kernel::{Boot, Kernel, Note};
use redoubt_model::mutation::Mutation;
use redoubt_model::spec::{FLAG_R, FLAG_W, FOREVER, PAGE_SIZE};
use redoubt_model::syscall::{Buffer, MintSource, Op, Syscall};
use redoubt_model::trace;

fn random_ops(seed: u64) -> Vec<Op> {
    let mut k = Kernel::boot(&Boot::testing(), None).unwrap();
    let mut g = Gen::new(seed);
    let mut ops = Vec::new();
    for _ in 0..150 {
        if k.halted.is_some() {
            break;
        }
        let op = g.next_op(&k);
        k.step(&op).unwrap();
        ops.push(op);
    }
    ops
}

#[test]
fn traces_round_trip() {
    for seed in 0..3000 {
        let ops = random_ops(seed);
        let text = trace::record(&Boot::testing(), &ops, None).unwrap();
        let (boot, parsed) = trace::parse(&text).unwrap_or_else(|e| panic!("seed {seed}: {e}\n{text}"));
        assert_eq!(boot, Boot::testing());
        assert_eq!(parsed, ops, "seed {seed}: parsing changed the ops\n{text}");
        trace::check(&text, None).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        for line in text.lines() {
            trace::tokens(line).unwrap_or_else(|e| panic!("seed {seed}: `{line}`: {e}"));
        }
    }
}

/// What WP-C1 does, with a broken model standing in for the kernel: random traces of the
/// specified model, each ending with the epilogue, replayed on a kernel that breaks one rule.
/// For every mutation of R1-R11, some trace among the first 2,000 must fail to replay.
///
/// Two breaks are not visible in results, so trace replay cannot see them and other tests must:
/// R12 (scheduling shows only in timing; WP-K5's tests), and `R5NoMaskOnFire` (a source left
/// unmasked re-fires into the kernel, but since `receive` unmasks and a pending line fires then
/// anyway, every result is the same; the model's own R5 check catches it, and WP-K3 must test
/// the mask directly). The three breaks of `MAX_OPEN_CALLS` need a flood to reach the limit (`check::flood`).
#[test]
fn a_rule_breaking_kernel_fails_replay() {
    common::quiet_panics();
    let texts: Vec<String> = (0..2000)
        .map(|seed| {
            let mut ops = random_ops(seed);
            let mut k = Kernel::boot(&Boot::testing(), None).unwrap();
            for op in &ops {
                k.step(op);
            }
            ops.extend(check::epilogue(&k));
            trace::record(&Boot::testing(), &ops, None).unwrap()
        })
        .collect();
    let mut missed = Vec::new();
    let invisible = |m: &Mutation| {
        m.rule() == "policy"
            || m.rule() == "R12"
            || *m == Mutation::R5NoMaskOnFire
            // Random sequences never reach MAX_OPEN_CALLS; the flood family does (its traces are
            // the ones to replay for R4a).
            || matches!(m, Mutation::OpenCallsUnlimited | Mutation::R4aOpenCallsPerThread | Mutation::R4aFullTakesNothing)
    };
    for m in Mutation::ALL.into_iter().filter(|m| !invisible(m)) {
        let detected = texts.iter().position(|text| {
            matches!(std::panic::catch_unwind(|| trace::check(text, Some(m))), Ok(Err(_)) | Err(_))
        });
        eprintln!("{m:?}: the first trace that does not replay is number {detected:?}");
        if detected.is_none() {
            missed.push(m);
        }
    }
    assert!(
        missed.is_empty(),
        "a kernel with these rule breaks replays the model's traces unnoticed: {missed:?}"
    );
}

/// The example in traces/: a client lends two pages to a server, its budget is destroyed while
/// the server holds them (R3), and the server keeps the pages until it replies. PIDs are drawn at
/// random, so the ops are built on a model as they go.
fn lender_dies_mid_call() -> Vec<Op> {
    let mut k = Kernel::boot(&Boot::default(), None).unwrap();
    let mut ops = Vec::new();
    let mut go = |k: &mut Kernel, op: Op| {
        let s = k.step(&op).unwrap();
        ops.push(op);
        s.notes.iter().find_map(|n| match n {
            Note::Thread { pid, tid } => Some((*pid, *tid)),
            _ => None,
        })
    };
    let init = |call| Op::Sys { pid: 1, tid: 1, call };
    // init's slots: 1 root, 2 system, 3 users, 4-8 the devices; new handles from 9.
    let budget = |parent, account| Syscall::BudgetCreate {
        parent,
        pages: 32,
        processes: 1,
        weight: 50,
        first: 0,
        labels: vec![],
        account,
        deadline: FOREVER,
    };
    let buf = 0x10_0000_0000; // the model's first kernel-chosen address
    go(&mut k, init(Syscall::EndpointCreate)); // h:9
    go(&mut k, init(budget(3, 1001))); // h:10 alice
    go(&mut k, init(budget(2, 0))); // h:11 server
    go(&mut k, init(Syscall::ProcessCreate { budget: 11, exit_endpoint: 9 })); // h:12
    let start = Syscall::ProcessStart { process: 12, entry: 0x1000, sp: 0x2000, arg: 0, handles: vec![9] };
    let (sp, st) = go(&mut k, init(start)).unwrap();
    go(&mut k, init(Syscall::Mint { source: MintSource::Handle(9), badge: 5, budget: Some(10) })); // h:13
    go(&mut k, init(Syscall::ProcessCreate { budget: 10, exit_endpoint: 9 })); // h:14
    let start = Syscall::ProcessStart { process: 14, entry: 0x1000, sp: 0x2000, arg: 0, handles: vec![13] };
    let (cp, ct) = go(&mut k, init(start)).unwrap();
    let server = |call| Op::Sys { pid: sp, tid: st, call };
    let client = |call| Op::Sys { pid: cp, tid: ct, call };
    go(&mut k, server(Syscall::Receive { h: Some(1), timeout: FOREVER, max_transfer: 0 }));
    go(&mut k, client(Syscall::MapAnon { len: 2 * PAGE_SIZE, flags: FLAG_R | FLAG_W }));
    go(&mut k, Op::Write { pid: cp, tid: ct, addr: buf, value: 42 });
    let lend = Some(Buffer { addr: buf, npages: 2 });
    go(&mut k, client(Syscall::Call { h: 1, words: [1, 2, 3, 4], handles: vec![], lend, timeout: FOREVER }));
    go(&mut k, Op::Read { pid: sp, tid: st, addr: buf });
    go(&mut k, Op::Write { pid: sp, tid: st, addr: buf, value: 99 });
    go(&mut k, init(Syscall::BudgetUsage { h: 11 }));
    go(&mut k, init(Syscall::BudgetDestroy { h: 10 }));
    go(&mut k, Op::Read { pid: sp, tid: st, addr: buf });
    go(&mut k, init(Syscall::BudgetUsage { h: 11 }));
    go(&mut k, server(Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 }));
    go(&mut k, server(Syscall::Reply { msg_id: 1, words: [0; 4], handles: vec![] }));
    go(&mut k, init(Syscall::BudgetUsage { h: 11 }));
    go(&mut k, server(Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 }));
    ops
}

const EXAMPLE: &str = include_str!("../traces/lender-dies-mid-call.trace");

const COMMENT: &str = "\
# R3, lends and abandoned calls: a client in alice's budget lends two pages to a system server;
# init destroys alice's budget while the server holds them. While the call is open the lent pages
# are charged to both sides (the server's usage counts them, with the open call and the two
# page-table pages mapping them); once the caller is gone, to the server only. The server still
# reads its buffer, is told the call was abandoned, replies (the reply is discarded) and the pages
# are freed. The client's exit notice (killed) is waiting on the endpoint.
# Written by tests/traces.rs (REDOUBT_MODEL_BLESS=1 rewrites it); format: README.md.
";

#[test]
fn the_example_trace_is_what_the_model_does() {
    let text = trace::record(&Boot::default(), &lender_dies_mid_call(), None).unwrap();
    if std::env::var("REDOUBT_MODEL_BLESS").is_ok() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/traces/lender-dies-mid-call.trace");
        std::fs::write(path, format!("{COMMENT}{text}")).unwrap();
    }
    let body: String = EXAMPLE.lines().filter(|l| !l.starts_with('#')).map(|l| format!("{l}\n")).collect();
    assert_eq!(body, text, "the model's trace changed; rerun with REDOUBT_MODEL_BLESS=1 to rewrite traces/");
    trace::check(EXAMPLE, None).unwrap();
}

/// A replayer reads traces from outside: hostile text must give an error, never a panic or a
/// hang. Recorded traces are damaged at random (a token replaced by something hostile, lines
/// dropped or duplicated) and replayed on the model; plus a few fixed worst cases.
#[test]
fn hostile_traces_are_refused_cleanly() {
    let hostile = [
        "18446744073709551615",
        "h:18446744073709551615+18446744073709551615",
        "a:0xffffffffffffffff+0x10",
        "[1,[2,[3]]]",
        "k=[a=b]",
        "x@18446744073709551615",
        "@",
        "=",
        "[",
        "]",
        "-",
        "forever",
        "tm:1",
        "m:0",
        "h:0",
        "0",
    ];
    let fixed = [
        String::new(),
        "redoubt-model-trace 1\n".to_string(),
        format!("redoubt-model-trace 1\n{}\n", "[".repeat(100_000)),
        format!("redoubt-model-trace 1\nboot root=[{}] system=[1,1,1] users=[1,1,1]\n", "9,".repeat(10_000)),
        "redoubt-model-trace 1\nboot root=[0,0,0] system=[0,0,0] users=[0,0,0]\n".to_string(),
        "redoubt-model-trace 1\ncosts budget=1 process=1 thread=1 endpoint=1 handles_per_page=0 page_table=1 \
         open_call=1 exit_slot=1 badge_slots_per_page=1\n"
            .to_string(),
        // Costs whose sums would overflow (the red team's panics at `tables_needed` and at a
        // delivery's page count).
        "redoubt-model-trace 1\ncosts budget=1 process=1 thread=1 endpoint=1 handles_per_page=1 \
         page_table=18446744073709551615 open_call=18446744073709551615 exit_slot=1 badge_slots_per_page=1\n"
            .to_string(),
        "redoubt-model-trace 1\ntick 18446744073709551615\n".to_string(),
        "redoubt-model-trace 1\ndo p:1 t:1 random 18446744073709551615 -> ok\n".to_string(),
    ];
    for text in &fixed {
        let r = std::panic::catch_unwind(|| trace::check(text, None));
        assert!(matches!(r, Ok(Err(_))), "{:?}: {r:?}", &text[..text.len().min(80)]);
    }
    let mut rng = redoubt_model::gen::Rng::new(99);
    for seed in 0..500 {
        let text = trace::record(&Boot::testing(), &random_ops(seed), None).unwrap();
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        for _ in 0..rng.range(1, 4) {
            let i = rng.below(lines.len() as u64) as usize;
            match rng.below(3) {
                0 => {
                    let mut t: Vec<String> = lines[i].split(' ').map(String::from).collect();
                    let j = rng.below(t.len() as u64) as usize;
                    t[j] = hostile[rng.below(hostile.len() as u64) as usize].to_string();
                    lines[i] = t.join(" ");
                }
                1 => {
                    lines.remove(i);
                }
                _ => {
                    let l = lines[i].clone();
                    lines.insert(i, l);
                }
            }
        }
        let damaged = lines.join("\n");
        let r = std::panic::catch_unwind(|| trace::check(&damaged, None));
        assert!(r.is_ok(), "seed {seed}: the replayer panicked on\n{damaged}");
    }
}
