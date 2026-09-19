//! The trace format: what the model writes, it reads back and replays to the same text; a
//! trace from a kernel that breaks a rule does not replay on the specified model; and the example
//! trace in `traces/` is what the model does.

mod common;

use redoubt_model::check;
use redoubt_model::gen::Gen;
use redoubt_model::kernel::{Boot, Kernel};
use redoubt_model::mutation::Mutation;
use redoubt_model::spec::{Class, FLAG_R, FLAG_W, FOREVER, PAGE_SIZE};
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
/// the mask directly).
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
    let invisible =
        |m: &Mutation| m.rule() == "policy" || m.rule() == "R12" || *m == Mutation::R5NoMaskOnFire;
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
/// the server holds them (R3), and the server keeps the pages until it replies.
fn lender_dies_mid_call() -> Vec<Op> {
    let init = |call| Op::Sys { pid: 1, tid: 1, call };
    let server = |call| Op::Sys { pid: 2, tid: 2, call };
    let client = |call| Op::Sys { pid: 3, tid: 3, call };
    // init's slots: 1 root, 2 system, 3 users, 4-8 the devices; new handles from 9.
    let budget = |parent, class: Class, account| Syscall::BudgetCreate {
        parent,
        pages: 32,
        processes: 1,
        weight: 50,
        class: class.raw(),
        labels: vec![],
        account,
        deadline: FOREVER,
    };
    let buf = 0x10_0000_0000; // the model's first kernel-chosen address
    vec![
        init(Syscall::EndpointCreate),                                 // h:9
        init(budget(3, Class::User, 1001)),                            // h:10 alice
        init(budget(2, Class::System, 0)),                             // h:11 server
        init(Syscall::ProcessCreate { budget: 11, exit_endpoint: 9 }), // h:12
        init(Syscall::ProcessStart { process: 12, entry: 0x1000, sp: 0x2000, handles: vec![9] }),
        init(Syscall::Mint { source: MintSource::Handle(9), badge: 5, budget: Some(10) }), // h:13
        init(Syscall::ProcessCreate { budget: 10, exit_endpoint: 9 }),                     // h:14
        init(Syscall::ProcessStart { process: 14, entry: 0x1000, sp: 0x2000, handles: vec![13] }),
        server(Syscall::Receive { h: Some(1), timeout: FOREVER, max_transfer: 0 }),
        client(Syscall::MapAnon { len: 2 * PAGE_SIZE, flags: FLAG_R | FLAG_W }),
        Op::Write { pid: 3, tid: 3, addr: buf, value: 42 },
        client(Syscall::Call {
            h: 1,
            words: [1, 2, 3, 4],
            handles: vec![],
            lend: Some(Buffer { addr: buf, npages: 2 }),
            timeout: FOREVER,
        }),
        Op::Read { pid: 2, tid: 2, addr: buf },
        Op::Write { pid: 2, tid: 2, addr: buf, value: 99 },
        init(Syscall::BudgetDestroy { h: 10 }),
        Op::Read { pid: 2, tid: 2, addr: buf },
        init(Syscall::BudgetUsage { h: 11 }),
        server(Syscall::Reply { msg_id: 1, words: [0; 4], handles: vec![] }),
        init(Syscall::BudgetUsage { h: 11 }),
        server(Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 }),
    ]
}

const EXAMPLE: &str = include_str!("../traces/lender-dies-mid-call.trace");

const COMMENT: &str = "\
# R3, lends outlive their lender: a client in alice's budget lends two pages to a system server;
# init destroys alice's budget while the server holds them. The server still reads its buffer,
# the pages are charged to the server's budget (its usage is 6 pages, not 4) until its reply, which is
# discarded, and then freed. The client's exit notice (killed) is waiting on the endpoint.
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
