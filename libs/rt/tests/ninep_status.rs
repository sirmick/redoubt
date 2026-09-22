//! The server's ownership refusal must be recognized by the generated protocol codec.
mod common;

#[path = "../src/bin/echo-server.rs"]
mod echo_server;

use common::fake;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::Client;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::proto::ninep_common;

#[test]
fn wrong_owner_disconnect_is_a_recognized_not_yours_reply() {
    assert!(echo_server::LIMITS.fits(&echo_server::COST, echo_server::BUDGET));
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let owner = f.process(1001, &[]);
    let owner_connection = f.grant(server, receive, owner, 1);
    let stranger = f.process(1001, &[]);
    let stranger_connection = f.grant(server, receive, stranger, 2);
    let block = StartupBuilder::new(receive.index()).handle("echo", receive).finish().unwrap();
    let serving = f.run(server, move || echo_server::serve(&Startup::parse(&block).unwrap()));
    let id = f.as_process(owner, || {
        let mut client = Client::new(Endpoint::from_handle(owner_connection), 1).unwrap();
        client.new_connection("", 0).unwrap().1
    });
    f.as_process(stranger, || {
        let request = ninep_common::Message::Disconnect(ninep_common::Disconnect { id });
        let words = request.encode(&mut []).unwrap();
        let reply = Endpoint::from_handle(stranger_connection)
            .call(&words, &[], None, FOREVER)
            .into_result()
            .unwrap()
            .0;
        assert_eq!(
            ninep_common::Reply::decode(
                redoubt_rt::wire::typed::opcode(&words).unwrap(),
                &reply.words,
                &[],
                0,
            ),
            Ok(Err(ninep_common::ErrorCode::NotYours))
        );
    });
    // The refusal neither consumes the owner's id nor disconnects its child.
    f.as_process(owner, || {
        let mut client = Client::new(Endpoint::from_handle(owner_connection), 1).unwrap();
        client.disconnect(id).unwrap();
    });
    f.destroy(server, receive);
    assert_eq!(serving.join().unwrap(), 0);
}
