//! `launch` and `grants` on the fake kernel, which keeps what each child was given: the stub, the
//! image, the stack and the startup block where the launching convention puts them, the block
//! read back by the runtime's own parser; refusals before any kernel call; a refusal midway
//! handing the budget back; an image moved one batch at a time; and a child's grants released at
//! every server by its exit notice.

mod common;

use common::{AUDIT_BADGE, Boot, Keyd};
use redoubt_client::file::Connection;
use redoubt_client::grants::{Grants, RELEASE_TIMEOUT};
use redoubt_client::launch::{Job, Launch, PLACE_PAGES, STACK_PAGES, stack_paint};
use redoubt_client::{Error, Lend, Name, Refusal, typed};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Cause, Error as SysError, FOREVER, Handle, MAX_START_HANDLES, MemFlags, PAGE_SIZE};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::ipc::Event;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::keyd::{self, ErrorCode, Grant, Message, Release, Reply, SignRecord};
use stub::{IMAGE_AT, MAX_STACK_PAGES, STACK_TOP, STARTUP_AT, STUB_ENTRY};

const STUB: &[u8] = b"the stub, flat";
const IMAGE: &[u8] = b"\x7fELF the program";

/// A launcher with a budget and an exit endpoint of its own for one child.
fn launcher() -> (usize, Handle, Handle) {
    let f = fake();
    let launcher = f.process(0, &[]);
    (launcher, f.budget(launcher), f.endpoint(launcher))
}

fn h(i: u32) -> Handle { Handle::new(i).unwrap() }

fn padded(bytes: &[u8], pages: usize) -> Vec<u8> {
    let mut page = bytes.to_vec();
    page.resize(pages * PAGE_SIZE, 0);
    page
}

#[test]
fn a_child_gets_the_stub_its_image_a_stack_and_its_block() {
    let f = fake();
    let (launcher, budget, exit) = launcher();
    let cons = f.endpoint(launcher);
    let keys = f.endpoint(launcher);
    let job = f.as_process(launcher, || {
        let mut launch = Launch::new(STUB, IMAGE, Budget::from_handle(budget), Endpoint::from_handle(exit));
        launch.namespace("/", cons).namespace("/dev/cons", cons).handle("keys", keys).arg("-v").arg("");
        launch.stack_tag(7).heap_pages(48);
        launch.start().ok().unwrap()
    });
    let child = f.launched(launcher, job.process().handle());
    let rw = MemFlags::READ | MemFlags::WRITE;
    let block = &child.maps[3].2;
    assert_eq!(child.maps[0], (STUB_ENTRY, MemFlags::READ | MemFlags::EXECUTE, padded(STUB, 1)));
    assert_eq!(child.maps[1], (IMAGE_AT, rw, padded(IMAGE, 1)));
    let paint: Vec<u8> =
        (0..STACK_PAGES * PAGE_SIZE / 8).flat_map(|i| stack_paint(7, i as u16).to_le_bytes()).collect();
    assert_eq!(child.maps[2], (STACK_TOP - STACK_PAGES * PAGE_SIZE, rw, paint));
    assert_eq!((child.maps[3].0, child.maps[3].1), (STARTUP_AT, MemFlags::READ));
    assert_eq!(child.maps.len(), 4);
    assert_eq!(child.start, Some((STUB_ENTRY, STACK_TOP - 16, STARTUP_AT)));
    // The block, as the child's runtime and the stub will read it: one slot per handle, however
    // many names it has.
    let startup = Startup::parse(block).unwrap();
    assert_eq!(startup.namespace().collect::<Vec<_>>(), [("/", h(1)), ("/dev/cons", h(1))]);
    assert_eq!(startup.handle("keys"), Some(h(2)));
    assert_eq!(startup.args().collect::<Vec<_>>(), ["-v", ""]);
    assert_eq!(startup.image(), Some((IMAGE_AT, IMAGE.len())));
    assert_eq!((startup.heap_pages(), startup.tag()), (Some(48), 7));
    assert!(f.installed(launcher, job.process().handle(), 0, cons));
    assert!(f.installed(launcher, job.process().handle(), 1, keys));
}

#[test]
fn the_largest_stack_paints_through_its_last_unit_without_wrapping() {
    let f = fake();
    let (launcher, budget, exit) = launcher();
    let job = f.as_process(launcher, || {
        let mut launch = Launch::new(STUB, IMAGE, Budget::from_handle(budget), Endpoint::from_handle(exit));
        launch.stack_pages(MAX_STACK_PAGES).stack_tag(7);
        launch.start().ok().unwrap()
    });
    let child = f.launched(launcher, job.process().handle());
    let mut index = 0;
    for (at, _, bytes) in &child.maps[2..4] {
        assert_eq!(*at, STACK_TOP - MAX_STACK_PAGES * PAGE_SIZE + index * 8);
        for unit in bytes.chunks_exact(8) {
            assert_eq!(u64::from_le_bytes(unit.try_into().unwrap()), stack_paint(7, index as u16));
            index += 1;
        }
    }
    assert_eq!(index, 1 << 16);
    assert_eq!(child.maps[4].0, STARTUP_AT);
}

/// The attack: what `launch` refuses it refuses before any kernel call, so no half-made process
/// is left, and the budget comes back untouched. Empty, over-cap, and wrapping stacks are among
/// those refused.
#[test]
fn a_bad_launch_is_refused_before_any_kernel_call() {
    let f = fake();
    let (launcher, budget, exit) = launcher();
    let many: Vec<Handle> = (0..=MAX_START_HANDLES).map(|_| f.endpoint(launcher)).collect();
    // Leaked, so a step's names outlive every launch it builds.
    let names: &'static [String] = Vec::leak((0..many.len()).map(|i| format!("h{i}")).collect());
    let refusals: [(&dyn Fn(&mut Launch), &[u8], Refusal); 8] = [
        (&|l| many.iter().zip(names).for_each(|(h, n)| _ = l.handle(n, *h)), IMAGE, Refusal::TooManyHandles),
        (&|_| {}, b"", Refusal::EmptyImage),
        (&|l| _ = l.stack_pages(STACK_TOP / PAGE_SIZE + 1), IMAGE, Refusal::StackTooLarge),
        (&|l| _ = l.stack_pages(usize::MAX), IMAGE, Refusal::StackTooLarge),
        (&|l| _ = l.stack_pages(0), IMAGE, Refusal::StackTooLarge),
        (&|l| _ = l.stack_pages(MAX_STACK_PAGES + 1), IMAGE, Refusal::StackTooLarge),
        (
            &|l| _ = l.handle("Not A Name", many[0]),
            IMAGE,
            Refusal::Startup(redoubt_rt::startup::StartupError::BadString),
        ),
        (
            &|l| _ = l.namespace("/", many[0]).namespace("/", many[1]),
            IMAGE,
            Refusal::Startup(redoubt_rt::startup::StartupError::Duplicate),
        ),
    ];
    let mut budget = Budget::from_handle(budget);
    let mut exit = Endpoint::from_handle(exit);
    for (build, image, refusal) in refusals {
        let calls = f.calls(launcher).len();
        let failed = f.as_process(launcher, || {
            let mut launch = Launch::new(STUB, image, budget, exit);
            build(&mut launch);
            launch.start().err().unwrap()
        });
        assert_eq!(failed.error, Error::Refused(refusal));
        assert_eq!(f.calls(launcher).len(), calls, "{refusal:?} made a kernel call");
        (budget, exit) = (failed.budget, failed.exit);
    }
    // Exactly MAX_START_HANDLES is not too many.
    let job = f.as_process(launcher, || {
        let mut launch = Launch::new(STUB, IMAGE, budget, exit);
        many[..MAX_START_HANDLES].iter().zip(names).for_each(|(h, n)| _ = launch.handle(n, *h));
        launch.start().ok().unwrap()
    });
    assert!(f.installed(
        launcher,
        job.process().handle(),
        MAX_START_HANDLES - 1,
        many[MAX_START_HANDLES - 1]
    ));
}

/// A kernel refusal after `process_create` hands the budget back, with the process that never
/// started in it, for the caller to destroy.
#[test]
fn a_refusal_midway_hands_the_budget_back() {
    let f = fake();
    let (launcher, budget, exit) = launcher();
    for call in ["process_map", "process_start"] {
        f.refuse(launcher, call, SysError::OutOfMemory);
        let failed = f.as_process(launcher, || {
            Launch::new(STUB, IMAGE, Budget::from_handle(budget), Endpoint::from_handle(exit))
                .start()
                .err()
                .unwrap()
        });
        assert_eq!(failed.error, Error::Sys(SysError::OutOfMemory), "{call}");
        assert_eq!(failed.budget.handle(), budget);
    }
    f.as_process(launcher, || Budget::from_handle(budget).destroy().unwrap());
    assert!(f.destroyed(launcher, budget));
}

/// An image of three batches and a page moves in four batches, and a stack of a batch and a page
/// in two, each copied into fresh pages and moved before the next is made, so the launcher holds
/// one batch at most; the child's image reads back whole, and its untagged stack zeroed.
#[test]
fn an_image_moves_one_batch_at_a_time() {
    let f = fake();
    let (launcher, budget, exit) = launcher();
    let image: Vec<u8> = (0..(3 * PLACE_PAGES + 1) * PAGE_SIZE).map(|i| (i % 251) as u8).collect();
    let held = f.held(launcher).1;
    f.held_peak(launcher);
    let job = f.as_process(launcher, || {
        let mut launch = Launch::new(STUB, &image, Budget::from_handle(budget), Endpoint::from_handle(exit));
        launch.stack_pages(PLACE_PAGES + 1);
        launch.start().ok().unwrap()
    });
    let child = f.launched(launcher, job.process().handle());
    let moves = &child.maps[1..5];
    let batch = PLACE_PAGES * PAGE_SIZE;
    let at: Vec<_> = moves.iter().map(|(dst, flags, bytes)| (*dst, *flags, bytes.len())).collect();
    let rw = MemFlags::READ | MemFlags::WRITE;
    assert_eq!(
        at,
        [
            (IMAGE_AT, rw, batch),
            (IMAGE_AT + batch, rw, batch),
            (IMAGE_AT + 2 * batch, rw, batch),
            (IMAGE_AT + 3 * batch, rw, PAGE_SIZE)
        ]
    );
    assert_eq!(moves.iter().flat_map(|(_, _, bytes)| bytes.iter().copied()).collect::<Vec<_>>(), image);
    let stack_at = STACK_TOP - (PLACE_PAGES + 1) * PAGE_SIZE;
    let stack: Vec<_> =
        child.maps[5..7].iter().map(|(dst, flags, bytes)| (*dst, *flags, bytes.len())).collect();
    assert_eq!(stack, [(stack_at, rw, batch), (stack_at + batch, rw, PAGE_SIZE)]);
    assert!(child.maps[5..7].iter().all(|(_, _, bytes)| bytes.iter().all(|b| *b == 0)));
    assert_eq!((child.maps[7].0, child.maps.len()), (STARTUP_AT, 8));
    assert_eq!(f.held_peak(launcher), held + PLACE_PAGES);
    assert_eq!(f.held(launcher).1, held);
}

/// A launch refused on its image's third batch leaves the two moved batches in the child, which
/// never started, hands the budget back for the caller to destroy with them, and leaves the
/// launcher's pages as they were.
#[test]
fn a_refusal_on_the_third_batch_leaves_the_launcher_as_it_was() {
    let f = fake();
    let (launcher, budget, exit) = launcher();
    let image = vec![7u8; (3 * PLACE_PAGES + 1) * PAGE_SIZE];
    let held = f.held(launcher).1;
    let calls = f.calls(launcher).len();
    f.held_peak(launcher);
    // The stub's move and the image's first two go through.
    f.refuse_after(launcher, "process_map", 3, SysError::OutOfMemory);
    let failed = f.as_process(launcher, || {
        Launch::new(STUB, &image, Budget::from_handle(budget), Endpoint::from_handle(exit))
            .start()
            .err()
            .unwrap()
    });
    assert_eq!(failed.error, Error::Sys(SysError::OutOfMemory));
    assert_eq!(failed.budget.handle(), budget);
    let made = &f.calls(launcher)[calls..];
    assert_eq!(made.iter().filter(|call| **call == "process_map").count(), 4);
    // The child, never started, holds the stub and the image's first two batches.
    let [child] = &f.launched_in(launcher, budget)[..] else { panic!("one child in the budget") };
    let moved: Vec<_> = child.maps.iter().map(|(dst, _, bytes)| (*dst, bytes.len())).collect();
    let batch = PLACE_PAGES * PAGE_SIZE;
    assert_eq!(moved, [(STUB_ENTRY, PAGE_SIZE), (IMAGE_AT, batch), (IMAGE_AT + batch, batch)]);
    assert_eq!(child.start, None);
    assert_eq!(f.held_peak(launcher), held + PLACE_PAGES);
    assert_eq!(f.held(launcher).1, held);
    f.as_process(launcher, || failed.budget.destroy().unwrap());
    assert!(f.destroyed(launcher, budget));
}

/// The exit notice releases every grant the launcher made for the child, at every server: its
/// `/boot` connection, a connection the child minted from it for a child of its own, and its
/// `keyd` capability.
#[test]
fn the_exit_notice_releases_every_grant() {
    let f = fake();
    let boot = Boot::start();
    let keyd = Keyd::start();
    let launcher = boot.init;
    let audit = f.grant(keyd.server, keyd.receive, launcher, AUDIT_BADGE);
    let (budget, exit) = (f.budget(launcher), f.endpoint(launcher));
    let (mut job, boot_conn, capability) = f.as_process(launcher, || {
        let mut lend = Lend::new(1).unwrap();
        let root = Connection::attach(Endpoint::from_handle(boot.founding), &mut lend).unwrap();
        let mut grants = Grants::new();
        let boot_conn = grants.connection(&mut lend, &root, "", 0).unwrap();
        let audit = Endpoint::from_handle(audit);
        let (id, capability) =
            typed::call::<keyd::Protocol, _>(&audit, &mut lend, &Message::Grant(Grant {}), &[], |r, got| {
                let Reply::Grant(g) = r else { panic!("another reply") };
                (g.id, got.take(0).unwrap())
            })
            .unwrap();
        grants.record::<keyd::Protocol>(&audit, &Message::Release(Release { id })).unwrap();
        let mut launch = Launch::new(STUB, IMAGE, Budget::from_handle(budget), Endpoint::from_handle(exit));
        launch.namespace("/boot", boot_conn.handle()).handle("audit", capability).grants(grants);
        (launch.start().ok().unwrap(), boot_conn.handle(), capability)
    });
    // The child, and a child of its own on a connection it minted from its `/boot`.
    let child = f.process(1001, &[]);
    let boot_conn = f.copy(launcher, boot_conn, child);
    let capability = f.copy(launcher, capability, child);
    let grandchild = f.process(1001, &[]);
    let minted = f.as_process(child, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(boot_conn), &mut lend).unwrap();
        conn.open(&mut lend, "keyd", mode::OREAD).unwrap().close(&mut lend).unwrap();
        conn.new_connection(&mut lend, "", 0).unwrap().0
    });
    let minted = f.copy(child, minted.handle(), grandchild);
    let sign = Message::SignRecord(SignRecord { record: b"x" });
    let signs = |who: usize| {
        f.as_process(who, || {
            let mut lend = Lend::new(1).unwrap();
            typed::call::<keyd::Protocol, _>(
                &Endpoint::from_handle(capability),
                &mut lend,
                &sign,
                &[],
                |_, _| (),
            )
        })
    };
    assert_eq!(signs(child), Ok(()));

    f.exit(launcher, job.process().handle(), 3);
    let ended = f.as_process(launcher, || job.wait(FOREVER).unwrap());
    assert_eq!((ended.notice.cause, ended.notice.code), (Cause::Exited, 3));
    assert_eq!(ended.released, Ok(()));
    // The verdicts come from the servers: each connection is gone there, and keyd refuses.
    for (who, conn) in [(child, boot_conn), (grandchild, minted)] {
        f.as_process(who, || {
            let mut lend = Lend::new(1).unwrap();
            assert_eq!(
                Connection::attach(Endpoint::from_handle(conn), &mut lend).err(),
                Some(Error::Rerror(Name::Protocol))
            );
        });
    }
    assert_eq!(signs(child), Err(Error::Server(ErrorCode::NotPermitted.code())));
    keyd.stop();
    boot.stop();
}

/// The attack: a server that never answers a release cannot stop a launcher reaping. Its release
/// times out after `RELEASE_TIMEOUT` and is reported, not retried (the server sees one call, then
/// its abandonment), and the grant after it is still released: `bootfsd` refuses the connection.
#[test]
fn a_hung_server_does_not_stop_the_reaping() {
    let f = fake();
    let boot = Boot::start();
    let launcher = boot.init;
    let hung = f.process(0, &[]);
    let receive = f.endpoint(hung);
    let at = f.grant(hung, receive, launcher, 1);
    // Takes one call and never answers it; its caller gives up.
    let holding = f.run(hung, move || {
        let endpoint = Endpoint::from_handle(receive);
        let Ok(Event::Call(request)) = endpoint.receive(FOREVER, 0) else { return 1 };
        let Ok(Event::Abandoned(id)) = endpoint.receive(FOREVER, 0) else { return 2 };
        let again = endpoint.receive(2 * RELEASE_TIMEOUT, 0);
        u32::from(id != request.id()) * 3 + u32::from(!matches!(again, Err(SysError::Timeout))) * 4
    });
    let (budget, exit) = (f.budget(launcher), f.endpoint(launcher));
    let (mut job, conn) = f.as_process(launcher, || {
        let mut lend = Lend::new(1).unwrap();
        let root = Connection::attach(Endpoint::from_handle(boot.founding), &mut lend).unwrap();
        let mut grants = Grants::new();
        grants
            .record::<keyd::Protocol>(&Endpoint::from_handle(at), &Message::Release(Release { id: 7 }))
            .unwrap();
        let conn = grants.connection(&mut lend, &root, "", 0).unwrap();
        let mut launch = Launch::new(STUB, IMAGE, Budget::from_handle(budget), Endpoint::from_handle(exit));
        launch.namespace("/boot", conn.handle()).grants(grants);
        (launch.start().ok().unwrap(), conn.handle())
    });
    f.exit(launcher, job.process().handle(), 0);
    let ended = f.as_process(launcher, || job.wait(FOREVER).unwrap());
    assert_eq!(ended.released, Err(Error::Sys(SysError::Timeout)));
    assert_eq!(holding.join().unwrap(), 0);
    f.as_process(launcher, || {
        let mut lend = Lend::new(1).unwrap();
        assert_eq!(
            Connection::attach(Endpoint::from_handle(conn), &mut lend).err(),
            Some(Error::Rerror(Name::Protocol))
        );
    });
    boot.stop();
}

/// Killing a job destroys its budget; its notice, `killed`, still arrives for `wait`.
#[test]
fn a_killed_job_ends_with_its_notice() {
    let f = fake();
    let (launcher, budget, exit) = launcher();
    let mut job: Job = f.as_process(launcher, || {
        Launch::new(STUB, IMAGE, Budget::from_handle(budget), Endpoint::from_handle(exit))
            .start()
            .ok()
            .unwrap()
    });
    f.as_process(launcher, || job.kill().unwrap());
    assert!(f.destroyed(launcher, budget));
    let ended = f.as_process(launcher, || job.wait(FOREVER).unwrap());
    assert_eq!(ended.notice.cause, Cause::Killed);
    assert_eq!(ended.released, Ok(()));
}
