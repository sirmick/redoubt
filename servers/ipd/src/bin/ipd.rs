//! `ipd`, the program: one thread serving `/net` over 9P for one network (servers/ipd.md).
//!
//! **The loop.** Each turn answers expired parked calls, asks `netd` for the MAC while the link
//! is down, then waits for one event: a call is served (answered or parked) and nothing else
//! runs until the next `receive`, so no call is current while smoltcp runs; a frame from `netd`
//! is taken in; an abandoned call is answered. After any event that is not a call the stack is
//! polled and parked calls whose sockets moved are served again. A call is followed by a
//! `receive` with no wait, so the stack is polled as soon as nothing more is queued.
//!
//! **Started by the net rig until `init` exists.** The `tests/net` rig starts this program
//! through the loader stub with the startup block `init` will write; `init` starting it is
//! planned (servers/ipd.md, "Started by `init`"; plan/m1-separation.md).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_ipd::args;
use redoubt_ipd::fs::NetFs;
use redoubt_ipd::link::Link;
use redoubt_ipd::netd::NetdLink;
use redoubt_ipd::server::Ipd;
use redoubt_ipd::stack::{Entropy, Net, Stack, Use};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The startup block named no endpoint `ipd` for it to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// `receive` failed for a reason other than the endpoint going away.
pub const RECEIVE_FAILED: u32 = 3;
/// The startup block has no `netd` handle.
pub const NO_NETD: u32 = 5;
/// The arguments are not ones `ipd` will run with ([`args::parse`], [`args::Config::sizing`]).
pub const BAD_ARGS: u32 = 6;
/// The kernel would give no random word for the minted badges.
pub const NO_RESOURCES: u32 = 7;

/// The startup-block names of `ipd`'s handles (servers/init.md, "The startup block").
pub const ENDPOINT: &str = "ipd";
pub const NETD: &str = "netd";

/// How long `ipd` waits before asking `netd` for the MAC again, doubling to [`INFO_BACKOFF_MAX`].
pub const INFO_BACKOFF: u64 = 100_000;
pub const INFO_BACKOFF_MAX: u64 = 5_000_000;

/// The kernel's CSPRNG.
struct Kernel;

impl Entropy for Kernel {
    fn draw(&mut self, _why: Use) -> Option<u64> { redoubt_rt::handle::random_u64().ok() }
}

/// A clock that fails reads as the far future, so every deadline has passed and nothing waits
/// for ever on a broken clock.
fn now() -> u64 { redoubt_rt::handle::time_now().unwrap_or(u64::MAX) }

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(endpoint) = startup.handle(ENDPOINT).map(Endpoint::from_handle) else { return NO_ENDPOINT };
    let Ok(config) = args::parse(startup.args()) else { return BAD_ARGS };
    let Ok(sizing) = config.sizing() else { return BAD_ARGS };
    let Some(netd) = startup.handle(NETD).map(Endpoint::from_handle) else { return NO_NETD };
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RESOURCES };

    let net = Net { addr: config.addr, len: config.len, gateway: config.gateway, selfset: config.selfset() };
    let stack = Stack::new(net, Link::new(NetdLink::new(netd)), Kernel, sizing.max_sockets);
    let fs = NetFs::new(stack, &config.scopes, sizing.caps);
    let mut ipd = Ipd::new(NineServer::with_admission(fs, sizing.admission, random), config.ingress);

    let (mut next_info, mut backoff) = (0, INFO_BACKOFF);
    let mut poll_now = false;
    loop {
        let t = now();
        ipd.expire(t);
        // No link: ask `netd` again, with backoff. `ipd` never exits on a link fault.
        if !ipd.nine.fs.stack.is_up() && t >= next_info {
            let fs = ipd.fs();
            match fs.stack.link.netif().info() {
                Ok(mac) if fs.stack.link_up(mac, t) => backoff = INFO_BACKOFF,
                _ => {
                    next_info = t.saturating_add(backoff);
                    backoff = (backoff * 2).min(INFO_BACKOFF_MAX);
                }
            }
        }
        let mut timeout = ipd.timeout(t);
        if !ipd.nine.fs.stack.is_up() {
            let retry = next_info.saturating_sub(t);
            timeout = Some(timeout.map_or(retry, |d| d.min(retry)));
        }
        let timeout = if poll_now { 0 } else { timeout.map_or(FOREVER, |d| d.max(1)) };
        poll_now = false;
        match endpoint.receive(timeout, 1) {
            Ok(Event::Call(request)) => {
                ipd.on_call(request, now());
                poll_now = true;
                continue;
            }
            Ok(Event::Send(delivery)) => {
                ipd.on_send(delivery, now());
            }
            Ok(Event::Abandoned(id)) => ipd.on_abandoned(id),
            Ok(Event::Interrupt | Event::Exit(_)) | Err(Error::Timeout) => ipd.received_no_call(),
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return RECEIVE_FAILED,
        }
        ipd.poll(now());
    }
}
