//! Parked calls on the fake kernel (answers 81 and 82): a call held open is resumed under
//! `serve`, an abandoned one is replied to at once, and one past its server-side deadline is
//! answered with a timeout.

mod common;

use common::fake;
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::{self, Endpoint};
use redoubt_rt::ipc::Event;
use redoubt_rt::server::parked::Parked;
use redoubt_rt::server::{Admission, Limits};

/// Opcodes of the test protocol: park me; wake one parked call; stop.
const WAIT: u64 = 1;
const WAKE: u64 = 2;
/// The status of a parked call past its deadline.
const TIMED_OUT: u64 = 5;
/// The longest a call stays parked (µs).
const LONGEST: u64 = 200_000;

/// A server that parks `WAIT` calls and answers one with 42 for each `WAKE`, until its endpoint
/// goes. Returns how many abandoned-call notices it handled and how many calls expired.
fn serve(ep: Endpoint) -> (u32, u32) {
    let mut admission = Admission::new(Limits { buckets: 4, in_flight: 8, files: 0, state: 0 }).unwrap();
    let mut parked: Parked<u64> = Parked::new(LONGEST);
    let (mut abandoned, mut expired) = (0, 0);
    loop {
        let now = handle::time_now().unwrap();
        while let Some(call) = parked.expired(&mut admission, now) {
            let (request, _) = call.unwrap();
            request.reply(&[TIMED_OUT, 0, 0, 0], &[]).unwrap();
            expired += 1;
        }
        let timeout = parked.next_deadline().map_or(FOREVER, |d| d.saturating_sub(now).max(1));
        match ep.receive(timeout, 0) {
            Ok(Event::Call(request)) if request.words[0] == WAIT => {
                let badge = request.caller.badge;
                if let Err(refused) = parked.park(&mut admission, request, badge, badge, now) {
                    refused.0.reply(&[9, 0, 0, 0], &[]).unwrap();
                }
            }
            Ok(Event::Call(request)) if request.words[0] == WAKE => {
                let woken = match parked.resume_first(&mut admission, |_| true) {
                    Some(call) => {
                        let (waiting, _) = call.unwrap();
                        waiting.reply(&[0, 42, 0, 0], &[]).unwrap();
                        1
                    }
                    None => 0,
                };
                request.reply(&[0, woken, 0, 0], &[]).unwrap();
            }
            Ok(Event::Call(request)) => request.reply(&[1, 0, 0, 0], &[]).unwrap(),
            Ok(Event::Abandoned(id)) => {
                assert!(
                    parked.abandoned(&mut admission, id, &[0; 4]).is_some(),
                    "a notice for a call not parked"
                );
                abandoned += 1;
            }
            Ok(_) | Err(Error::Timeout) => {}
            Err(_) => return (abandoned, expired),
        }
    }
}

#[test]
fn parked_calls_are_served_abandoned_and_expired() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let [a, b, c, d] = [1001, 1002, 1003, 1004].map(|account| f.process(account, &[]));
    let [ha, hb, hc, hd] =
        [(a, 1), (b, 2), (c, 3), (d, 4)].map(|(pid, badge)| f.grant(server, receive, pid, badge));
    let server_thread = f.run(server, move || {
        let (abandoned, expired) = serve(Endpoint::from_handle(receive));
        abandoned * 100 + expired
    });

    // A waits; B wakes it (retrying until A's call is parked).
    let waiter = f.run(a, move || {
        Endpoint::from_handle(ha).call(&[WAIT, 0, 0, 0], &[], None, FOREVER).unwrap().words[1] as u32
    });
    f.run(b, move || {
        let ep = Endpoint::from_handle(hb);
        while ep.call(&[WAKE, 0, 0, 0], &[], None, FOREVER).unwrap().words[1] == 0 {
            handle::sleep(1000).unwrap();
        }
        0
    })
    .join()
    .unwrap();
    assert_eq!(waiter.join().unwrap(), 42);
    // The server made A's call its current call before it answered it (answer 82).
    // (A's is the only call resumed so far; B's were answered as they came.)
    let log = f.log(server);
    let served = log.iter().position(|(call, _)| *call == "serve").expect("serve before resuming");
    let a_call = log[served].1;
    assert!(served < log.iter().position(|e| *e == ("reply", a_call)).unwrap());

    // C gives up on its parked call: the server is told and replies at once, freeing it.
    let gave_up = f.run(c, move || {
        let r = Endpoint::from_handle(hc).call(&[WAIT, 0, 0, 0], &[], None, 100_000);
        u32::from(r == Err(Error::Timeout))
    });
    assert_eq!(gave_up.join().unwrap(), 1);
    // D waits past the server's deadline: its call is answered with a timeout, under `serve`.
    let expired = f.run(d, move || {
        Endpoint::from_handle(hd).call(&[WAIT, 0, 0, 0], &[], None, FOREVER).unwrap().words[0] as u32
    });
    assert_eq!(expired.join().unwrap(), TIMED_OUT as u32);
    let log = f.log(server);
    let (_, d_call) = *log.last().unwrap();
    assert_eq!(log[log.len() - 2], ("serve", d_call));

    while f.open_calls(server) != 0 {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 100 + 1, "one abandoned call, one expired");
}

#[test]
fn parking_is_admitted_per_bucket_and_share() {
    // A client parking more than its share gets its call back to answer at once.
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let client = f.process(2001, &[]);
    let conn = f.grant(server, receive, client, 7);
    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let mut admission = Admission::new(Limits { buckets: 2, in_flight: 4, files: 0, state: 0 }).unwrap();
        let mut parked: Parked<()> = Parked::new(10_000_000);
        let mut refused = 0;
        while let Ok(event) = ep.receive(FOREVER, 0) {
            if let Event::Call(request) = event {
                let now = handle::time_now().unwrap();
                if let Err(back) = parked.park(&mut admission, request, 7, (), now) {
                    back.0.reply(&[9, 0, 0, 0], &[]).unwrap();
                    refused += 1;
                    if refused == 1 {
                        // Answer the parked ones so the clients finish.
                        while let Some(call) = parked.resume_first(&mut admission, |_| true) {
                            call.unwrap().0.reply(&[0; 4], &[]).unwrap();
                        }
                    }
                }
            }
        }
        refused
    });
    // Half of the bucket's 4 for a lone share: the third call is refused at once.
    let threads: Vec<_> = (0..3)
        .map(|_| {
            f.run(client, move || {
                Endpoint::from_handle(conn).call(&[WAIT, 0, 0, 0], &[], None, FOREVER).unwrap().words[0]
                    as u32
            })
        })
        .collect();
    let mut statuses: Vec<u32> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    statuses.sort();
    assert_eq!(statuses, [0, 0, 9]);
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 1);
}

/// Answer 90's attack on a server that parks calls (the steward's shape): an agent floods its
/// sponsor's bucket; the sponsor still parks its own calls, and ending the agent's lease is
/// answered at once, ahead of admission, whatever the bucket holds.
#[test]
fn an_agent_flooding_a_bucket_leaves_its_sponsor_a_share_and_its_lease_end() {
    const END_LEASE: u64 = 3;
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let (agent, sponsor) = (f.process(1001, &[]), f.process(1001, &[]));
    let (agent_conn, sponsor_conn) =
        (f.grant(server, receive, agent, 20), f.grant(server, receive, sponsor, 21));
    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let mut admission = Admission::new(Limits { buckets: 4, in_flight: 8, files: 0, state: 0 }).unwrap();
        let mut parked: Parked<()> = Parked::new(10_000_000);
        while let Ok(event) = ep.receive(FOREVER, 0) {
            let Event::Call(request) = event else { continue };
            let now = handle::time_now().unwrap();
            let share = request.caller.badge;
            match request.words[0] {
                // Ahead of admission: no slot taken, answered at once; then every parked call
                // is answered, so the clients finish.
                END_LEASE => {
                    request.reply(&[0, parked.len() as u64, 0, 0], &[]).unwrap();
                    while let Some(call) = parked.resume_first(&mut admission, |_| true) {
                        call.unwrap().0.reply(&[0; 4], &[]).unwrap();
                    }
                }
                _ => {
                    if let Err(back) = parked.park(&mut admission, request, share, (), now) {
                        back.0.reply(&[9, 0, 0, 0], &[]).unwrap();
                    }
                }
            }
        }
        0
    });
    let wait = |pid, conn| {
        f.run(pid, move || {
            Endpoint::from_handle(conn).call(&[WAIT, 0, 0, 0], &[], None, FOREVER).unwrap().words[0] as u32
        })
    };
    // The agent floods: half the bucket (4 of 8) parks, the rest is refused.
    let flood: Vec<_> = (0..6).map(|_| wait(agent, agent_conn)).collect();
    while f.open_calls(server) < 4 {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // The sponsor still parks calls of its own: a third of the bucket.
    let own: Vec<_> = (0..2).map(|_| wait(sponsor, sponsor_conn)).collect();
    while f.open_calls(server) < 6 {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let parked_before_end = f.run(sponsor, move || {
        Endpoint::from_handle(sponsor_conn).call(&[END_LEASE, 0, 0, 0], &[], None, FOREVER).unwrap().words[1]
            as u32
    });
    assert_eq!(parked_before_end.join().unwrap(), 6);
    let mut agent_statuses: Vec<u32> = flood.into_iter().map(|t| t.join().unwrap()).collect();
    agent_statuses.sort();
    assert_eq!(agent_statuses, [0, 0, 0, 0, 9, 9]);
    assert!(own.into_iter().all(|t| t.join().unwrap() == 0));
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 0);
}
