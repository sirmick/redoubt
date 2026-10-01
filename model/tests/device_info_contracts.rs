//! `device_info` (kernel/devices.md): the model's own checks, independent of the kernel boot
//! bench (`tests/programs/src/bin/device-info-attack.rs` covers the kernel).

mod common;
use common::contracts::*;
use redoubt_model::{
    kernel::{Boot, DeviceSpec},
    spec::*,
    syscall::*,
};

/// The syscall's own `Result<Ret, Error>`, unwrapping the step machinery around it.
fn call(w: &mut World, s: Syscall) -> Result<Ret, Error> {
    match w.sys(1, s).unwrap().outcome {
        Outcome::Done(r) => r,
        x => panic!("expected a value or an error, got {x:?}"),
    }
}

/// `init` holds the three budgets, then every device in the boot's order: each handle answers
/// with the `Devs` entry its object was made from.
#[test]
fn every_device_handle_answers_its_devs_entry() {
    let boot = Boot::default();
    let mut w = World::booted(&boot, None);
    for (i, spec) in boot.devices.iter().enumerate() {
        let want = match *spec {
            DeviceSpec::Mmio { base, pages, dma, .. } => {
                Ret::Device { kind: 1, a: base, b: pages * PAGE_SIZE, flags: u64::from(dma) }
            }
            DeviceSpec::Irq { n } => Ret::Device { kind: 2, a: n, b: 0, flags: 0 },
            DeviceSpec::Reset => Ret::Device { kind: 3, a: 0, b: 0, flags: 0 },
        };
        assert_eq!(call(&mut w, Syscall::DeviceInfo { h: 4 + i as u64 }), Ok(want), "device {i}");
    }
}

/// `BadHandle` (0, too wide, not held, closed), then `WrongObject` for anything that is not a
/// device: a budget, an endpoint.
#[test]
fn what_is_not_a_device_is_refused_in_order() {
    let mut w = World::new(None);
    let Ok(Ret::Handle(ep)) = call(&mut w, Syscall::EndpointCreate) else { panic!("endpoint") };
    let Ok(Ret::Handle(closed)) = call(&mut w, Syscall::EndpointCreate) else { panic!("endpoint") };
    assert_eq!(call(&mut w, Syscall::HandleClose { h: closed }), Ok(Ret::Unit));
    for h in [0, 1 << 32, u64::MAX, closed, 999] {
        assert_eq!(call(&mut w, Syscall::DeviceInfo { h }), Err(Error::BadHandle), "handle {h:#x}");
    }
    for h in [1, 2, 3, ep] {
        assert_eq!(call(&mut w, Syscall::DeviceInfo { h }), Err(Error::WrongObject), "handle {h}");
    }
}

/// It maps nothing and charges nothing: every budget's usage and the caller's address space are
/// as they were, and an IRQ stays masked.
#[test]
fn device_info_changes_nothing() {
    let boot = Boot::default();
    let mut w = World::booted(&boot, None);
    let usage = |w: &World| w.k.budgets.values().map(|b| b.pages_used).collect::<Vec<_>>();
    let (before, space, devices) = (usage(&w), w.k.processes[&1].space.len(), format!("{:?}", w.k.devices));
    for h in 0..4 + boot.devices.len() as u64 + 2 {
        let _ = call(&mut w, Syscall::DeviceInfo { h });
    }
    assert_eq!(usage(&w), before);
    assert_eq!(w.k.processes[&1].space.len(), space);
    assert_eq!(format!("{:?}", w.k.devices), devices);
}
