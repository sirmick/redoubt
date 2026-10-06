//! The steward, the program: read the manifest lines from the arguments, boot the policy core,
//! carve each principal's budgets under `users`, say the tree on the console, then receive on the
//! endpoint named `steward` until it is destroyed.
//!
//! Everything it decides is in `redoubt-steward-server`'s library, so host tests drive the same
//! code (`tests/steward.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::abi::{BudgetSpec, Error, Handle};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::server::own_args;
use redoubt_rt::startup::Startup;
use redoubt_steward_server::{Kernel, start};

redoubt_rt::entry!(serve);

/// The startup block named no `users` budget or no `steward` endpoint.
pub const NO_HANDLE: u32 = 2;
/// The manifest lines were refused, or a carve failed: the box has no users
/// ([`redoubt_steward_server::StartError`], said on the console first).
pub const NOT_STARTED: u32 = 3;

/// Says `line` on the console `init` gave the steward, if it has one.
fn say(startup: &Startup, line: &str) {
    let Some((_, console)) = startup.namespace().find(|(path, _)| *path == "/dev/cons") else { return };
    let Ok(mut lend) = Lend::new(1) else { return };
    let console = Connection::new(Endpoint::from_handle(console));
    let _ = console
        .attach(&mut lend, 0, "")
        .and_then(|_| console.open(&mut lend, 0, mode::OWRITE))
        .and_then(|_| console.write(&mut lend, 0, 0, line.as_bytes()));
    let _ = console.clunk(&mut lend, 0);
}

struct Machine;

impl Kernel for Machine {
    type Budget = Handle;

    fn create(&mut self, parent: Handle, spec: &BudgetSpec) -> Result<Handle, Error> {
        Budget::from_handle(parent).create_child(spec).map(|b| b.handle())
    }
}

fn labels(l: &[u64]) -> String {
    let items: Vec<String> = l.iter().map(|x| format!("{x}")).collect();
    format!("{{{}}}", items.join(","))
}

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let (Some(users), Some(endpoint)) = (startup.handle("users"), startup.handle("steward")) else {
        return NO_HANDLE;
    };
    let args: Vec<&str> = startup.args().collect();
    let lines: Vec<&str> = own_args(&args).collect();
    let steward = match start(&lines, users, &mut Machine) {
        Ok(s) => s,
        Err(e) => {
            say(startup, &format!("{e}\n"));
            return NOT_STARTED;
        }
    };
    for c in &steward.carved {
        let subs: Vec<String> = c.subs.iter().map(|(d, _)| labels(d.labels().as_slice())).collect();
        say(
            startup,
            &format!("steward: carved users/{} account {}: {}\n", c.name, c.account, subs.join(" ")),
        );
    }
    // The protocol (`libs/wire/tables/steward.md`) is the next step: until then a call is
    // received and dropped.
    redoubt_rt::server::serve(&Endpoint::from_handle(endpoint), |_request| ())
}
