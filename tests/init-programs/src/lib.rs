//! What the test programs `init` starts share: their console. Each writes its lines through the
//! `/dev/cons` connection `init` minted for it, so `consoled` starts every one of them with that
//! connection's id (servers/consoled.md, "Started by `init`"), and the bench attributes a verdict
//! by it (docs/testbench.md, "The servers' cases under `init`").

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use redoubt_client::console::Console;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::startup::Startup;

/// Pages lent with each call: a console line or a read of a small entry.
pub const LEND_PAGES: usize = 1;

/// Exit codes: the program could not reach its console, so it had no line to say why.
pub mod code {
    pub const NO_LEND: u32 = 2;
    pub const NO_CONSOLE: u32 = 3;
}

/// Where a program ends once it has said its lines: `init` restarts a server that exits, so a
/// program whose exit is not the point of its case never exits.
pub fn park() -> ! {
    loop {
        let _ = redoubt_rt::handle::sleep(redoubt_rt::abi::FOREVER);
    }
}

/// The console `init` gave the program, and the lend its calls use.
pub struct Out {
    pub lend: Lend,
    console: Console,
}

impl Out {
    /// `/dev/cons` from the block's namespace.
    pub fn open(startup: &Startup) -> Result<Out, u32> {
        let mut lend = Lend::new(LEND_PAGES).map_err(|_| code::NO_LEND)?;
        let ns = Namespace::from_startup(startup, &mut lend).map_err(|_| code::NO_CONSOLE)?;
        let console = Console::open(&ns, &mut lend).map_err(|_| code::NO_CONSOLE)?;
        Ok(Out { lend, console })
    }

    /// Writes all of `text`, however many writes that takes.
    pub fn say(&mut self, text: &str) -> Result<(), Error> {
        let mut rest = text.as_bytes();
        while !rest.is_empty() {
            match self.console.write(&mut self.lend, rest)? {
                0 => return Err(Error::Rerror),
                n => rest = &rest[n..],
            }
        }
        Ok(())
    }
}
