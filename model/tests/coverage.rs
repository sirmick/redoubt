//! The generator reaches every error path: across a few thousand random sequences, every system
//! call succeeds at least once, every error of the spec's enum is returned by some call, and the
//! interesting deliveries (lends, transfers, each exit cause, interrupts, replies) all happen.
//! Without this, a property that "holds" might only hold because nothing reached it.

use std::collections::BTreeSet;

use redoubt_model::gen::Gen;
use redoubt_model::kernel::{Boot, Kernel};
use redoubt_model::spec::{Cause, Error};
use redoubt_model::syscall::{BufferKind, CALL_NAMES, Op, Outcome, Ret};

/// The error a `call` or a turn carries in its result, if it failed.
fn status_error(r: &Ret) -> Option<Error> {
    match r {
        Ret::Call(c) => c.status.err(),
        Ret::Turn { half, .. } => half.as_ref().as_ref().err().copied(),
        _ => None,
    }
}

fn failed(r: &Ret) -> bool { status_error(r).is_some() }

fn kind(r: &Result<Ret, Error>) -> String {
    match r {
        Ok(Ret::Message(m)) => format!("message {:?}", m.buffer.map(|b| b.kind)),
        Ok(Ret::ExitNotice { cause, .. }) => format!("exit {cause:?}"),
        Ok(Ret::Interrupt { .. }) => "interrupt".into(),
        Ok(Ret::Call(c)) => match &c.reply {
            Some(_) => "reply".into(),
            None => c.status.map_or_else(|e| e.name().into(), |_| "unit".into()),
        },
        Ok(Ret::Replied { .. }) => "unit".into(),
        Ok(Ret::Turn { half, .. }) => kind(half),
        Ok(_) => "ok".into(),
        Err(e) => format!("{e:?}"),
    }
}

#[test]
fn every_call_and_error_is_reached() {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut errors: BTreeSet<Error> = BTreeSet::new();
    for seed in 0..5000u64 {
        // The call each blocked thread is in, so its later result counts for that call.
        let mut blocked: std::collections::BTreeMap<u64, &'static str> = Default::default();
        let mut k = Kernel::boot(&Boot::testing(), None).unwrap();
        let mut g = Gen::new(seed);
        // Long enough for a thread bomb to reach `MAX_THREADS`.
        for _ in 0..450 {
            if k.halted.is_some() {
                break;
            }
            let op = g.next_op(&k);
            let s = k.step(&op).unwrap();
            if let Op::Sys { call, tid, .. } = &op {
                if s.outcome == Outcome::Blocked {
                    blocked.insert(*tid, call.name());
                }
                if matches!(&s.outcome, Outcome::Done(Ok(r)) if !failed(r)) || s.outcome == Outcome::Gone {
                    seen.insert(format!("{} ok", call.name()));
                }
                let what = match &s.outcome {
                    Outcome::Done(r) => kind(r),
                    Outcome::Blocked => "blocked".into(),
                    Outcome::Gone => "ok".into(),
                };
                if let Outcome::Done(Err(e)) = s.outcome {
                    errors.insert(e);
                }
                if let Outcome::Done(Ok(r)) = &s.outcome {
                    if let Some(e) = status_error(r) {
                        errors.insert(e);
                    }
                }
                seen.insert(format!("{} {what}", call.name()));
            }
            for w in &s.wakes {
                if let Ok(r) = &w.result {
                    if let Some(e) = status_error(r) {
                        errors.insert(e);
                    }
                }
                if let Err(e) = w.result {
                    errors.insert(e);
                }
                if let (Some(name), Ok(r)) = (blocked.remove(&w.tid), &w.result) {
                    if failed(r) {
                        continue;
                    }
                    seen.insert(format!("{name} ok"));
                }
                seen.insert(format!("wake {}", kind(&w.result)));
            }
        }
    }
    for c in CALL_NAMES {
        assert!(seen.contains(&format!("{c} ok")), "`{c}` never succeeded");
    }
    for e in Error::ALL {
        assert!(errors.contains(&e), "no call ever returned {e:?}");
    }
    for w in [
        format!("message {:?}", Some(BufferKind::Lend)),
        format!("message {:?}", Some(BufferKind::Transfer)),
        format!("exit {:?}", Cause::Exited),
        format!("exit {:?}", Cause::Faulted),
        format!("exit {:?}", Cause::Killed),
        "interrupt".into(),
        "reply".into(),
    ] {
        assert!(
            seen.contains(&format!("wake {w}")) || seen.contains(&format!("receive {w}")),
            "never delivered: {w}"
        );
    }
}
