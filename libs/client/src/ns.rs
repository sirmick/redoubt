//! The namespace: the program's table of where each connection is bound, built from its startup
//! block (userland/sessions.md, "How a program reads its namespace"). A name resolves by the
//! longest matching prefix, the one rule the runtime's startup block uses too.
//!
//! The namespace owns its connections. A `bind` puts a connection under another prefix: the same
//! connection, one badge, not a copy.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::path;
use redoubt_rt::startup::Startup;

use crate::error::{Error, Refusal};
use crate::file::Connection;

pub struct Namespace {
    entries: Vec<(String, Connection)>,
}

impl Namespace {
    /// Every namespace entry of the block, each connection attached once: a handle bound at two
    /// paths is one connection. A server that cannot be attached fails the whole table.
    pub fn from_startup(startup: &Startup, lend: &mut Lend) -> Result<Namespace, Error> {
        let mut handles = Vec::new();
        let mut entries: Vec<(String, Connection)> = Vec::new();
        for (prefix, handle) in startup.namespace() {
            let conn = match handles.iter().position(|h| *h == handle) {
                Some(i) => entries[i].1.clone(),
                None => Connection::attach(Endpoint::from_handle(handle), lend)?,
            };
            handles.push(handle);
            entries.push((prefix.to_string(), conn));
        }
        Ok(Namespace { entries })
    }

    /// An empty namespace, for a program started with none.
    pub fn new() -> Namespace { Namespace { entries: Vec::new() } }

    /// Binds `conn` at the clean absolute `prefix`, in place of whatever was bound there.
    pub fn bind(&mut self, prefix: &str, conn: Connection) -> Result<(), Error> {
        if !path::is_clean_absolute(prefix) {
            return Err(Refusal::BadPath.into());
        }
        self.entries.retain(|(p, _)| p != prefix);
        self.entries.push((prefix.to_string(), conn));
        Ok(())
    }

    /// The connection the clean absolute `path` resolves to, and the rest of `path` below it.
    pub fn lookup<'p>(&self, path: &'p str) -> Option<(&Connection, &'p str)> {
        path::resolve(self.list(), path)
    }

    /// The table: (prefix, connection), in binding order.
    pub fn list(&self) -> impl Iterator<Item = (&str, &Connection)> {
        self.entries.iter().map(|(prefix, conn)| (prefix.as_str(), conn))
    }
}

impl Default for Namespace {
    fn default() -> Namespace { Namespace::new() }
}
