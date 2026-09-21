// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use std::net::ToSocketAddrs;
use std::thread::JoinHandle;

use crossbeam_channel::unbounded;
#[cfg(feature = "report-memory")]
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, Stats, StatsAlloc};
use redoubt_abi::{SysCall, rsyscall};

use crate::kmain;
#[cfg(feature = "report-memory")]
#[global_allocator]
static GLOBAL: &StatsAlloc<std::alloc::System> = &INSTRUMENTED_SYSTEM;

const SERVER_SPEC: &str = "127.0.0.1:0";

use core::sync::atomic::{AtomicU64, Ordering};
static RNG_LOCAL_STATE: AtomicU64 = AtomicU64::new(1);

fn start_kernel(server_spec: &str) -> JoinHandle<()> {
    assert!(
        std::env::var("REDOUBT_LISTEN_ADDR").is_err(),
        "REDOUBT_LISTEN_ADDR environment variable must be unset to run tests"
    );
    assert!(
        std::env::var("REDOUBT_SERVER").is_err(),
        "REDOUBT_SERVER environment variable must be unset to run tests"
    );

    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::RngCore;
    use rand_chacha::rand_core::SeedableRng;
    let mut pid1_key = [0u8; 16];
    let mut rng = ChaCha8Rng::seed_from_u64(
        RNG_LOCAL_STATE.load(Ordering::SeqCst)
            + redoubt_abi::TESTING_RNG_SEED.load(core::sync::atomic::Ordering::SeqCst),
    );
    //let mut rng = thread_rng();
    for b in pid1_key.iter_mut() {
        *b = rng.next_u32() as u8;
    }
    RNG_LOCAL_STATE.store(rng.next_u64(), Ordering::SeqCst);
    redoubt_abi::arch::set_process_key(&pid1_key);

    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");
    // Attempt to bind. This will fail if the port is in use.
    // let temp_server = TcpListener::bind(server_addr).unwrap();
    // let server_addr = temp_server.local_addr().unwrap();
    // drop(temp_server);

    let (send_addr, recv_addr) = unbounded();

    // Launch the main thread. We pass a `send_addr` channel so that the
    // server can notify us when it's ready to listen.
    let main_thread = std::thread::Builder::new()
        .name("kernel main".to_owned())
        .spawn(move || {
            let server_spec_server = server_addr;
            crate::arch::set_pid1_key(pid1_key);
            crate::arch::set_send_addr(send_addr);
            crate::arch::set_listen_address(&server_spec_server);
            kmain()
        })
        .expect("couldn't start kernel thread");
    let server_addr = recv_addr.recv().unwrap();
    redoubt_abi::arch::set_redoubt_address(server_addr);

    // Connect to server. This first instance needs to make sure the kernel is listening.
    // let mut server_conn = None;
    // let mut connected = false;
    // for i in 1..11 {
    //     let res = TcpStream::connect_timeout(&server_addr, Duration::from_millis(200));
    //     if res.is_ok() {
    //         connected = true;
    //         break;
    //     }
    //     println!("Retrying connection {}/10", i);
    // }
    // // Convert the Option<conn> into conn
    // assert!(connected, "unable to connect to server");
    main_thread
}

fn shutdown_kernel() {
    // Any process ought to be able to shut down the system currently.
    redoubt_abi::wait_process_as_thread(
        redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new("shutdown", || {
            rsyscall(SysCall::Shutdown).expect("unable to shutdown server");
        }))
        .expect("couldn't shut down the kernel"),
    )
    .expect("couldn't wait for the shutdown process to end");
}

// /// Spawn a new "process" with the given server spec inside the given closure
// /// and return a join handle
// fn as_process<F, R>(f: F) -> JoinHandle<R>
// where
//     F: FnOnce() -> R,
//     F: Send + 'static,
//     R: Send + 'static,
// {
//     let server_spec = redoubt_abi::arch::redoubt_address();
//     std::thread::spawn(move || {
//         redoubt_abi::arch::set_redoubt_address(server_spec);
//         redoubt_abi::arch::redoubt_connect();
//         f()
//     })
// }

#[test]
fn shutdown() {
    // Start the server in another thread.
    let main_thread = start_kernel(SERVER_SPEC);

    // Send a raw `Shutdown` message to terminate the kernel.
    shutdown_kernel();

    // Wait for the kernel to exit.
    main_thread.join().expect("couldn't join main thread");
}

#[test]
fn connect_for_process() {
    use redoubt_abi::SID;
    // Start the server in another thread
    let main_thread = start_kernel(SERVER_SPEC);
    let nameserver_addr_bytes = b"nameserver-12345";
    let nameserver_addr = SID::from_bytes(nameserver_addr_bytes).unwrap();

    let (server_addr_send, server_addr_recv) = unbounded();
    let (nameserver_send, nameserver_recv) = unbounded();

    // Spawn the client "process" and wait for the server address.
    let nameserver_process = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "nameserver_process",
        move || {
            let sid = redoubt_abi::create_server_with_address(&nameserver_addr_bytes)
                .expect("couldn't create test server");
            // Indicate that the nameserver is running
            nameserver_send.send(()).unwrap();

            // Receive the first message, which is the SID to register
            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            let (msg, other_sid) = if let redoubt_abi::Message::Scalar(msg) = envelope.body {
                (msg.id, SID::from_u32(msg.arg1 as _, msg.arg2 as _, msg.arg3 as _, msg.arg4 as _))
            } else {
                panic!("unexpected message")
            };

            assert!(msg == 1, "unexpected message id");

            // Receive the second message, which is the "name" to "resolve"
            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            let msg = if let redoubt_abi::Message::BlockingScalar(msg) = envelope.body {
                msg.id
            } else {
                panic!("unexpected message")
            };
            assert!(msg == 10, "unexpected message id");

            let new_cid_result = redoubt_abi::connect_for_process(envelope.sender.pid().unwrap(), other_sid)
                .expect("couldn't connect for other process");
            let new_cid = if let redoubt_abi::Result::ConnectionID(c) = new_cid_result {
                c
            } else {
                panic!("Unexpected return value");
            };
            redoubt_abi::return_scalar(envelope.sender, new_cid as usize).expect("couldn't return scalar");
        },
    ))
    .expect("couldn't spawn client process");

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message server",
        move || {
            let sid = redoubt_abi::create_server().expect("couldn't create test server");
            // Wait for nameserver to start
            nameserver_recv.recv().unwrap();

            let conn = redoubt_abi::try_connect(nameserver_addr).expect("couldn't connect to server");
            // Register our SID with the nameserver
            let sid_u32 = sid.to_u32();
            redoubt_abi::send_message(
                conn,
                redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                    id: 1,
                    arg1: sid_u32.0 as _,
                    arg2: sid_u32.1 as _,
                    arg3: sid_u32.2 as _,
                    arg4: sid_u32.3 as _,
                }),
            )
            .expect("couldn't send message");

            server_addr_send.send(()).unwrap();

            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            assert_eq!(
                envelope.body,
                redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                    id: 15,
                    arg1: 21,
                    arg2: 31,
                    arg3: 41,
                    arg4: 51
                })
            );
        },
    ))
    .expect("couldn't spawn server process");

    // Spawn the client "process" and wait for the server address.
    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message client",
        move || {
            server_addr_recv.recv().unwrap();
            let conn = redoubt_abi::try_connect(nameserver_addr).expect("couldn't connect to server");

            // Attempt to resolve this address.
            let other_conn_result = redoubt_abi::try_send_message(
                conn,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: 10,
                    arg1: 52,
                    arg2: 53,
                    arg3: 54,
                    arg4: 55,
                }),
            )
            .expect("couldn't send message");
            let other_conn = if let redoubt_abi::Result::Scalar1(r) = other_conn_result {
                r
            } else {
                panic!("unexpected return value");
            };

            // Send a message to the server we were just connected to.
            redoubt_abi::try_send_message(
                other_conn as u32,
                redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                    id: 15,
                    arg1: 21,
                    arg2: 31,
                    arg3: 41,
                    arg4: 51,
                }),
            )
            .expect("couldn't send message");
        },
    ))
    .expect("couldn't spawn client process");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");
    crate::wait_process_as_thread(nameserver_process).expect("couldn't join nameserver process");
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_scalar_message() {
    // Start the server in another thread
    let main_thread = start_kernel(SERVER_SPEC);

    let (server_addr_send, server_addr_recv) = unbounded();

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message server",
        move || {
            let sid = redoubt_abi::create_server().expect("couldn't create test server");
            server_addr_send.send(sid).unwrap();
            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            assert_eq!(
                envelope.body,
                redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                    id: 1,
                    arg1: 2,
                    arg2: 3,
                    arg3: 4,
                    arg4: 5
                })
            );
        },
    ))
    .expect("couldn't spawn server process");

    // Spawn the client "process" and wait for the server address.
    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message client",
        move || {
            let sid = server_addr_recv.recv().unwrap();
            let conn = redoubt_abi::try_connect(sid).expect("couldn't connect to server");
            redoubt_abi::try_send_message(
                conn,
                redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                    id: 1,
                    arg1: 2,
                    arg2: 3,
                    arg3: 4,
                    arg4: 5,
                }),
            )
            .expect("couldn't send message");
        },
    ))
    .expect("couldn't spawn client process");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn try_receive_message() {
    // Start the server in another thread
    let main_thread = start_kernel(SERVER_SPEC);

    let (server_addr_send, server_addr_recv) = unbounded();
    let (client_sent_send, client_sent_recv) = unbounded();

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message server",
        move || {
            let sid = redoubt_abi::create_server().expect("couldn't create test server");
            let maybe_envelope = redoubt_abi::try_receive_message(sid).expect("couldn't receive messages");
            assert!(maybe_envelope.is_none(), "some message came back");
            server_addr_send.send(sid).unwrap();
            client_sent_recv.recv().unwrap();
            let maybe_envelope = redoubt_abi::try_receive_message(sid).expect("couldn't receive messages");
            let envelope = maybe_envelope.expect("got None as an envelope");
            assert_eq!(
                envelope.body,
                redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                    id: 11,
                    arg1: 12,
                    arg2: 13,
                    arg3: 14,
                    arg4: 15
                })
            );
        },
    ))
    .expect("couldn't spawn server process");

    // Spawn the client "process" and wait for the server address.
    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message client",
        move || {
            let sid = server_addr_recv.recv().unwrap();
            let conn = redoubt_abi::try_connect(sid).expect("couldn't connect to server");
            redoubt_abi::try_send_message(
                conn,
                redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                    id: 11,
                    arg1: 12,
                    arg2: 13,
                    arg3: 14,
                    arg4: 15,
                }),
            )
            .expect("couldn't send message");
            client_sent_send.send(()).expect("couldn't notify them we sent a message");
        },
    ))
    .expect("couldn't spawn client process");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_blocking_scalar_message() {
    // Start the server in another thread
    let main_thread = start_kernel(SERVER_SPEC);

    let (server_addr_send, server_addr_recv) = unbounded();

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message server",
        move || {
            let sid = redoubt_abi::create_server_with_address(b"send_scalar_mesg")
                .expect("couldn't create test server");
            server_addr_send.send(sid).unwrap();
            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            assert_eq!(
                envelope.body,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: 1,
                    arg1: 2,
                    arg2: 3,
                    arg3: 4,
                    arg4: 5
                })
            );
            redoubt_abi::return_scalar(envelope.sender, 42).expect("couldn't return scalar");

            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            assert_eq!(
                envelope.body,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: 6,
                    arg1: 7,
                    arg2: 8,
                    arg3: 9,
                    arg4: 10
                })
            );
            redoubt_abi::return_scalar2(envelope.sender, 56, 78).expect("couldn't return scalar");
        },
    ))
    .expect("couldn't spawn server process");

    // Spawn the client "process" and wait for the server address.
    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message client",
        move || {
            let sid = server_addr_recv.recv().unwrap();
            let conn = redoubt_abi::try_connect(sid).expect("couldn't connect to server");
            let result = redoubt_abi::try_send_message(
                conn,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: 1,
                    arg1: 2,
                    arg2: 3,
                    arg3: 4,
                    arg4: 5,
                }),
            )
            .expect("couldn't send message");
            assert_eq!(result, redoubt_abi::Result::Scalar1(42));

            let result = redoubt_abi::try_send_message(
                conn,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: 6,
                    arg1: 7,
                    arg2: 8,
                    arg3: 9,
                    arg4: 10,
                }),
            )
            .expect("couldn't send message");
            assert_eq!(result, redoubt_abi::Result::Scalar2(56, 78));
        },
    ))
    .expect("couldn't spawn client process");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn message_ordering() {
    // Start the server in another thread
    let main_thread = start_kernel(SERVER_SPEC);

    let (server_addr_send, server_addr_recv) = unbounded();
    let (server_can_start_send, server_can_start_recv) = unbounded();
    let (client_total_send, client_total_recv) = unbounded();
    let (server_total_send, server_total_recv) = unbounded();

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message server",
        move || {
            let sid = redoubt_abi::create_server_with_address(b"send_scalar_mesg")
                .expect("couldn't create test server");
            server_addr_send.send(sid).unwrap();
            // Sync point waiting to start receiving.
            server_can_start_recv.recv().unwrap();

            let mut queue_length = 1;
            // Keep receiving messages until we get a BlockingScalar message
            loop {
                let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
                match envelope.body {
                    redoubt_abi::Message::Scalar(sm) => {
                        assert_eq!(sm.id, queue_length, "messages were not ordered");
                        queue_length += 1;
                    }
                    redoubt_abi::Message::BlockingScalar(sm) => {
                        assert_eq!(sm.id, queue_length, "blocking message were not ordered");
                        // The BlockingScalar has exceeded the queue length, so subtract
                        // 1 from the running total.
                        queue_length -= 1;
                        redoubt_abi::return_scalar(envelope.sender, queue_length)
                            .expect("couldn't return scalar");
                        break;
                    }
                    _ => panic!("unexpected message received"),
                }
            }

            // Return the total number of messages we've seen to the parent
            server_total_send.send(queue_length).unwrap();
        },
    ))
    .expect("couldn't spawn server process");

    // Spawn the client "process" and wait for the server address.
    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message client",
        move || {
            let sid = server_addr_recv.recv().unwrap();
            let conn = redoubt_abi::try_connect(sid).expect("couldn't connect to server");

            // Determine the length of the kernel queue.
            let mut queue_length = 0;
            for i in 1.. {
                if redoubt_abi::try_send_message(
                    conn,
                    redoubt_abi::Message::Scalar(redoubt_abi::ScalarMessage {
                        id: i,
                        arg1: 0,
                        arg2: 0,
                        arg3: 0,
                        arg4: 0,
                    }),
                )
                .is_err()
                {
                    break;
                }
                queue_length += 1;
            }

            // Let the server process messages
            server_can_start_send.send(()).ok();

            // Send one more message, but make it blocking. This acts as a sentinal
            // value to let the kernel know things are done.
            redoubt_abi::send_message(
                conn,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: queue_length + 1,
                    arg1: 0,
                    arg2: 0,
                    arg3: 0,
                    arg4: 0,
                }),
            )
            .expect("couldn't send message");

            // Report the number of messages to the main thread.
            client_total_send.send(queue_length).unwrap();
        },
    ))
    .expect("couldn't spawn client process");

    let server_total = server_total_recv.recv().unwrap();
    let client_total = client_total_recv.recv().unwrap();

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");
    shutdown_kernel();
    assert_eq!(client_total, server_total, "client and server processed a different number of messages");

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_interleved_blocking_scalar_message() {
    // Start the server in another thread
    let main_thread = start_kernel(SERVER_SPEC);

    let (server_addr_send, server_addr_recv) = unbounded();

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message server",
        move || {
            let sid = redoubt_abi::create_server_with_address(b"send_scalar_mesg")
                .expect("couldn't create test server");
            server_addr_send.send(sid).unwrap();

            let envelope1 = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            let envelope2 = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            let retval1 = if let redoubt_abi::Message::BlockingScalar(bs) = envelope1.body {
                bs.id + 1
            } else {
                panic!("unexpected value")
            };
            let retval2 = if let redoubt_abi::Message::BlockingScalar(bs) = envelope2.body {
                bs.id + 10
            } else {
                panic!("unexpected value")
            };
            redoubt_abi::return_scalar(envelope2.sender, retval2).expect("couldn't return scalar");
            redoubt_abi::return_scalar(envelope1.sender, retval1).expect("couldn't return scalar");
        },
    ))
    .expect("couldn't spawn server process");

    let sid_client_1 = server_addr_recv.recv().unwrap();
    let sid_client_2 = sid_client_1;

    // Spawn the client "process" and wait for the server address. This one will have
    // 1 added to the `id` field.
    let redoubt_client_1 = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message client 1",
        move || {
            let conn = redoubt_abi::try_connect(sid_client_1).expect("couldn't connect to server");
            let result = redoubt_abi::try_send_message(
                conn,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: 1,
                    arg1: 2,
                    arg2: 3,
                    arg3: 4,
                    arg4: 5,
                }),
            )
            .expect("couldn't send message");
            assert_eq!(result, redoubt_abi::Result::Scalar1(2));
        },
    ))
    .expect("couldn't spawn client 1 process");

    // Spawn the client "process" and wait for the server address. This one
    // will have `10` added to the value when it is returned.
    let redoubt_client_2 = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_scalar_message client 2",
        move || {
            let conn = redoubt_abi::try_connect(sid_client_2).expect("couldn't connect to server");
            let result = redoubt_abi::try_send_message(
                conn,
                redoubt_abi::Message::BlockingScalar(redoubt_abi::ScalarMessage {
                    id: 10,
                    arg1: 2,
                    arg2: 3,
                    arg3: 4,
                    arg4: 5,
                }),
            )
            .expect("couldn't send message");
            assert_eq!(result, redoubt_abi::Result::Scalar1(20));
        },
    ))
    .expect("couldn't spawn client 2 process");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client_1).expect("couldn't join client process");
    crate::wait_process_as_thread(redoubt_client_2).expect("couldn't join client process");
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_move_message() {
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let main_thread = start_kernel(SERVER_SPEC);

    let (server_addr_send, server_addr_recv) = unbounded();

    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_move_message server",
        move || {
            // println!("SERVER: Creating server...");
            let sid = redoubt_abi::create_server_with_address(b"send_move_messag")
                .expect("couldn't create test server");
            // println!("SERVER: Sending server address of {:?} to client", sid);
            server_addr_send.send(sid).unwrap();
            // println!("SERVER: Starting to receive messages...");
            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            // println!("SERVER: Received message from {}", envelope.sender);
            let message = envelope.body;
            if let redoubt_abi::Message::Move(m) = message {
                let buf = m.buf;
                let bt = unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) };
                assert_eq!(*test_bytes, *bt, "message was changed by the kernel");
            // let s = String::from_utf8_lossy(&bt);
            // println!("SERVER: Got message: {:?} -> \"{}\"", bt, s);
            } else {
                panic!("unexpected message type");
            }
        },
    ))
    .expect("couldn't start server");

    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_move_message client",
        move || {
            // println!("CLIENT: Waiting for server address...");
            let sid = server_addr_recv.recv().unwrap();
            // println!("CLIENT: Connecting to server {:?}", sid);
            let conn = redoubt_abi::try_connect(sid).expect("couldn't connect to server");
            let msg = redoubt_abi::carton::Carton::from_bytes(test_bytes);
            redoubt_abi::try_send_message(conn, redoubt_abi::Message::Move(msg.into_message(0)))
                .expect("couldn't send a message");
            // println!("CLIENT: Message sent");
        },
    ))
    .expect("couldn't start client");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_borrow_message() {
    let main_thread = start_kernel(SERVER_SPEC);
    let (server_addr_send, server_addr_recv) = unbounded();
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_borrow_message server",
        move || {
            {
                // println!("SERVER: Creating server...");
                let sid = redoubt_abi::create_server_with_address(b"send_borrow_mesg")
                    .expect("couldn't create test server");
                server_addr_send.send(sid).unwrap();
                // println!("SERVER: Receiving message...");
                let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
                // println!("SERVER: Received message from {}", envelope.sender);
                let message = envelope.body;
                if let redoubt_abi::Message::Borrow(m) = message {
                    let buf = m.buf;
                    let bt = unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) };
                    assert_eq!(*test_bytes, *bt);
                    // let s = String::from_utf8_lossy(&bt);
                    // println!("SERVER: Got message: {:?} -> \"{}\"", bt, s);
                    redoubt_abi::return_memory(envelope.sender, m.buf).unwrap();
                // println!("SERVER: Returned memory");
                // println!("SERVER: Returned memory");
                } else {
                    panic!("unexpected message type");
                }
                // println!("SERVER: Dropping things");
            }
            // println!("SERVER: Exiting");
        },
    ))
    .expect("couldn't start server");

    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_borrow_message client",
        move || {
            {
                // Get the server address (out of band) so we know what to connect to
                // println!("CLIENT: Waiting for server to start...");
                let sid = server_addr_recv.recv().unwrap();

                // Perform a connection to the server
                // println!("CLIENT: Connecting to server...");
                let conn = redoubt_abi::connect(sid).expect("couldn't connect to server");

                // Convert the message into a "Carton" that can be shipped as a message
                // println!("CLIENT: Creating carton...");
                let carton = redoubt_abi::carton::Carton::from_bytes(test_bytes);

                // Send the message to the server
                // println!("CLIENT: Lending message...");
                carton.lend(conn, 0).expect("couldn't lend message to server");

                // println!("CLIENT: Done, dropping things");
            }
            // println!("CLIENT: Exit");
        },
    ))
    .expect("couldn't start client");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_mutableborrow_message() {
    let main_thread = start_kernel(SERVER_SPEC);
    let (server_addr_send, server_addr_recv) = unbounded();
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_mutableborrow_message server",
        move || {
            let sid = redoubt_abi::create_server_with_address(b"send_mutborrow_m")
                .expect("couldn't create test server");
            server_addr_send.send(sid).unwrap();
            let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
            // println!("Received message from {}", envelope.sender);
            let message = envelope.body;
            if let redoubt_abi::Message::MutableBorrow(m) = message {
                let bt = unsafe { core::slice::from_raw_parts_mut(m.buf.as_mut_ptr(), m.buf.len()) };
                // eprintln!("SERVER: UPDATING VALUES");
                for letter in bt.iter_mut() {
                    *letter += 1;
                }
                redoubt_abi::return_memory(envelope.sender, m.buf).unwrap();
            } else {
                panic!("unexpected message type");
            }
        },
    ))
    .expect("couldn't start server");

    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_mutableborrow_message client",
        move || {
            // Get the server address (out of band) so we know what to connect to
            let sid = server_addr_recv.recv().unwrap();

            // Perform a connection to the server
            let conn = redoubt_abi::connect(sid).expect("couldn't connect to server");

            // Convert the message into a "Carton" that can be shipped as a message
            let mut carton = redoubt_abi::carton::Carton::from_bytes(&test_bytes);
            let mut check_bytes = test_bytes.to_vec();
            for letter in check_bytes.iter_mut() {
                *letter += 1;
            }

            // Send the message to the server
            // eprintln!("CLIENT: SENDING MESSAGE: {:?}", test_bytes.to_vec());
            carton.lend_mut(conn, 3).expect("couldn't mutably lend data");

            let modified_bytes: &[u8] = carton.as_ref();
            assert_eq!(&check_bytes, &modified_bytes);
        },
    ))
    .expect("couldn't start client");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_repeat_mutableborrow_message() {
    let main_thread = start_kernel(SERVER_SPEC);
    let (server_addr_send, server_addr_recv) = unbounded();
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let loops = 50;

    let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_mutableborrow_message_repeat server",
        move || {
            let sid = redoubt_abi::create_server_with_address(b"send_mutborrow_r")
                .expect("couldn't create test server");
            server_addr_send.send(sid).unwrap();

            for iteration in 0..loops {
                let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
                let message = envelope.body;
                if let redoubt_abi::Message::MutableBorrow(m) = message {
                    let buf = m.buf;
                    let bt = unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) };
                    for letter in bt.iter_mut() {
                        *letter = (*letter).wrapping_add((iteration & 0xff) as u8);
                    }
                    redoubt_abi::return_memory(envelope.sender, m.buf).unwrap();
                } else {
                    panic!("unexpected message type");
                }
            }
        },
    ))
    .expect("couldn't start server");

    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "send_mutableborrow_message_repeat client",
        move || {
            // Get the server address (out of band) so we know what to connect to
            let sid = server_addr_recv.recv().unwrap();

            // Perform a connection to the server
            let conn = redoubt_abi::connect(sid).expect("couldn't connect to server");

            // Convert the message into a "Carton" that can be shipped as a message
            for iteration in 0..loops {
                let mut carton = redoubt_abi::carton::Carton::from_bytes(&test_bytes);
                let mut check_bytes = test_bytes.to_vec();
                for letter in check_bytes.iter_mut() {
                    *letter = (*letter).wrapping_add((iteration & 0xff) as u8);
                }

                // Send the message to the server
                carton.lend_mut(conn, 3).expect("couldn't mutably lend data");

                let modified_bytes: &[u8] = carton.as_ref();
                assert_eq!(&check_bytes, &modified_bytes);
            }
        },
    ))
    .expect("couldn't start client");

    // Wait for both processes to finish
    crate::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
    crate::wait_process_as_thread(redoubt_client).expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

// #[cfg(feature = "report-memory")]
// #[test]
// fn measure_memory_usage() {
//     let mut reg = Region::new(&GLOBAL);
//     reg.reset();

//     {
//         // Run the "shutdown" test in its own thread. This ensures that
//         // any TLS is freed when the thread exits.
//         let jh = as_process(shutdown);
//         jh.join()
//             .expect("couldn't run shutdown test for measuring memory");
//     }

//     fn memory_in_use(start: &Stats) -> usize {
//         if start.bytes_deallocated > start.bytes_allocated {
//             eprintln!("Allocated a negative number of bytes!");
//             0
//         } else {
//             start.bytes_allocated - start.bytes_deallocated
//         }
//     }

//     let after_join = reg.change();
//     let miu = memory_in_use(&after_join);
//     println!("After test: {:#?} ({} bytes in use)", after_join, miu);
// }

/// Test that a server can be its own client
#[test]
fn server_client_same_process() {
    // Start the kernel in its own thread
    let main_thread = start_kernel(SERVER_SPEC);

    let internal_server = redoubt_abi::create_process_as_thread(redoubt_abi::arch::ProcessArgsAsThread::new(
        "server_client_same_process process",
        || {
            let server = redoubt_abi::create_server().expect("couldn't create server");
            let connection = redoubt_abi::try_connect(server).expect("couldn't connect to our own server");
            let msg_contents = redoubt_abi::ScalarMessage { id: 1, arg1: 2, arg2: 3, arg3: 4, arg4: 5 };

            redoubt_abi::try_send_message(connection, redoubt_abi::Message::Scalar(msg_contents))
                .expect("couldn't send message");

            let msg = redoubt_abi::receive_message(server).expect("couldn't receive message");

            assert_eq!(msg.body, redoubt_abi::Message::Scalar(msg_contents));
        },
    ))
    .expect("couldn't start server");

    redoubt_abi::wait_process_as_thread(internal_server).expect("couldn't join internal_server process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}

/// Test that one process can have multiple contexts
#[test]
fn multiple_contexts() {
    // ::debug_here::debug_here!();
    // Start the kernel in its own thread
    let main_thread = start_kernel(SERVER_SPEC);

    let internal_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "multiple_contexts process",
        move || {
            let server = redoubt_abi::create_server().expect("couldn't create server");
            let connection = redoubt_abi::try_connect(server).expect("couldn't connect to our own server");
            let msg_contents = redoubt_abi::ScalarMessage { id: 1, arg1: 2, arg2: 3, arg3: 4, arg4: 5 };

            let mut server_threads = vec![];
            for _ in 1..crate::arch::process::MAX_THREAD {
                server_threads.push(
                    redoubt_abi::create_thread(move || {
                        let msg = redoubt_abi::receive_message(server).expect("couldn't receive message");
                        assert_eq!(msg.body, redoubt_abi::Message::Scalar(msg_contents));
                    })
                    .expect("couldn't spawn client thread"),
                );
            }

            for _ in &server_threads {
                redoubt_abi::try_send_message(connection, redoubt_abi::Message::Scalar(msg_contents))
                    .expect("couldn't send message");
            }
            for server_thread in server_threads.into_iter() {
                redoubt_abi::wait_thread(server_thread).expect("couldn't wait for thread");
            }
        },
    ))
    .expect("couldn't create internal server");

    redoubt_abi::wait_process_as_thread(internal_server).expect("couldn't join internal_server process");

    // Any process ought to be able to shut down the system currently.
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn multiple_multiple_contexts() {
    for _ in 0..5 {
        multiple_contexts();
    }
}

/// Test that a server can be restarted and the kernel doesn't crash
#[test]
fn process_restart_server() {
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let main_thread = start_kernel(SERVER_SPEC);

    fn create_destroy_server(test_bytes: &'static [u8]) {
        let (server_addr_send, server_addr_recv) = unbounded();

        let redoubt_server = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
            "process_restart_server server",
            move || {
                let sid = redoubt_abi::create_server_with_address(b"test_recreate_se")
                    .expect("couldn't create test server");
                server_addr_send.send(sid).unwrap();
                let thr = redoubt_abi::create_thread(move || {
                    let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive messages");
                    // println!("Received message from {}", envelope.sender);
                    let message = envelope.body;
                    if let redoubt_abi::Message::Move(m) = message {
                        let buf = m.buf;
                        let bt = unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) };
                        assert_eq!(*test_bytes, *bt);
                    // let s = String::from_utf8_lossy(&bt);
                    // println!("Got message: {:?} -> \"{}\"", bt, s);
                    } else {
                        panic!("unexpected message type");
                    }
                })
                .unwrap();
                redoubt_abi::wait_thread(thr).unwrap();
            },
        ))
        .expect("couldn't spawn server process");

        // Wait for the server to start up
        let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
            "process_restart_server client",
            move || {
                let sid = server_addr_recv.recv().unwrap();
                let conn = redoubt_abi::try_connect(sid).expect("couldn't connect to server");
                let msg = redoubt_abi::carton::Carton::from_bytes(test_bytes);
                redoubt_abi::try_send_message(conn, redoubt_abi::Message::Move(msg.into_message(0)))
                    .expect("couldn't send a message");
            },
        ))
        .expect("couldn't start client process");

        redoubt_abi::wait_process_as_thread(redoubt_server).expect("couldn't join server process");
        redoubt_abi::wait_process_as_thread(redoubt_client).expect("couldn't join client process");
    }

    // create_destroy_server(test_bytes);
    create_destroy_server(test_bytes);

    // Any process ought to be able to shut down the system currently.
    shutdown_kernel();

    main_thread.join().expect("couldn't join kernel process");
}

/// `IncreaseHeap` must reject a `delta` that would overflow
/// `mem_heap_size + delta`. On an RV32 target, a large `delta`
/// silently wraps past `mem_heap_size` in unchecked arithmetic,
/// which can land below `mem_heap_max` and bypass the cap check.
/// Hosted `usize` is 64-bit so the 32-bit wrap isn't directly
/// triggerable here, but the syscall must still reject an absurd
/// delta cleanly.
#[test]
fn increase_heap_rejects_absurd_delta() {
    let main_thread = start_kernel(SERVER_SPEC);

    let redoubt_client = redoubt_abi::create_process_as_thread(redoubt_abi::ProcessArgsAsThread::new(
        "increase_heap_overflow",
        move || {
            let flags = redoubt_abi::MemoryFlags::R | redoubt_abi::MemoryFlags::W;

            // A normal call should succeed.
            let ok = redoubt_abi::increase_heap(4096, flags);
            assert!(ok.is_ok(), "small IncreaseHeap should succeed, got {:?}", ok);

            // A pathological delta should be rejected, not silently accepted.
            let err = redoubt_abi::increase_heap(usize::MAX, flags);
            assert!(err.is_err(), "usize::MAX delta must be rejected, got {:?}", err);
        },
    ))
    .expect("spawn client");

    crate::wait_process_as_thread(redoubt_client).expect("join client");
    shutdown_kernel();
    main_thread.join().expect("join kernel");
}

#[cfg(test)]
mod queue_capacity_tests {
    use redoubt_abi::{Message, PID, SID};

    use crate::server::Server;

    fn scalar(id: usize) -> Message {
        Message::Scalar(redoubt_abi::ScalarMessage { id, arg1: 0, arg2: 0, arg3: 0, arg4: 0 })
    }

    fn make_server() -> Server {
        let mut slot: Option<Server> = None;
        let backing = crate::mem::memory_range(0x1_0000, 0x1_000).unwrap();
        Server::init(&mut slot, PID::new(3).unwrap(), SID::from_bytes(&[0x12, 0x34]).unwrap(), backing)
            .unwrap();
        slot.unwrap()
    }

    /// Response tokens (`WaitingReturn*`, enqueued by `queue_response()` on
    /// every blocking handoff) occupy queue slots but never advance the
    /// generation counters. A queue saturated with tokens must therefore be
    /// reported as full by `has_queue_capacity()` -- this is the exact state
    /// in which a blocking direct handoff (available server thread, full
    /// queue) used to pass the pre-check, move its payload, and then fail
    /// inside `queue_response()`. `has_queue_capacity_scalar()` deliberately
    /// misses it; that is only sound because scalar sends have no payload to
    /// strand on a retry.
    #[test]
    fn token_saturated_queue_reports_full() {
        let mut server = make_server();
        let client = PID::new(4).unwrap();
        let mut tokens = 0;
        while server.queue_response(client, 0, &scalar(1), None).is_ok() {
            tokens += 1;
        }
        assert!(tokens > 0, "queue accepted no tokens");
        assert!(!server.has_queue_capacity(), "token-saturated queue must read as full");
        assert!(server.has_queue_capacity_scalar(), "gen-only probe is expected to miss token saturation");
        assert_eq!(
            server.queue_response(client, 0, &scalar(2), None),
            Err(redoubt_abi::Error::ServerQueueFull)
        );
        assert_eq!(
            server.queue_message(client, 0, scalar(3), None),
            Err(redoubt_abi::Error::ServerQueueFull)
        );
    }

    /// `has_queue_capacity()` is the pre-flight check that lets
    /// `send_message()` decide it may transfer a payload before enqueueing.
    /// It must never disagree with `queue_response()`'s acceptance in any
    /// reachable state (here: progressive token fill through a wrapped
    /// queue head).
    #[test]
    fn has_queue_capacity_agrees_with_response_enqueue() {
        let mut server = make_server();
        let client = PID::new(4).unwrap();
        for round in 0..10_000usize {
            let predicted = server.has_queue_capacity();
            let accepted = server.queue_response(client, 0, &scalar(1), None).is_ok();
            assert_eq!(
                predicted, accepted,
                "has_queue_capacity() and queue_response() diverged at round {}",
                round
            );
            if !accepted {
                break;
            }
        }
    }

    /// Same agreement property against the message-enqueue path.
    #[test]
    fn has_queue_capacity_agrees_with_message_enqueue() {
        let mut server = make_server();
        let client = PID::new(4).unwrap();
        for round in 0..10_000usize {
            let predicted = server.has_queue_capacity();
            let accepted = server.queue_message(client, 0, scalar(1), None).is_ok();
            assert_eq!(
                predicted, accepted,
                "has_queue_capacity() and queue_message() diverged at round {}",
                round
            );
            if !accepted {
                break;
            }
        }
    }
}
