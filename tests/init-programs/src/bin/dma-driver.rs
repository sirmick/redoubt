//! `dma-driver`: a test driver `init` places a DMA-capable device for, as `dev`. At each start it
//! finds the device by that name and takes one page of DMA, which joins the device to its reset
//! set, then serves [`redoubt_init_programs::dma_driver`]'s requests: it faults on `fault`, so that
//! the kernel resets the device at its end and `init` restarts it, or reboots if the reset failed
//! and the device was quarantined (servers/init.md, "Restarts and reboots"). An instance that
//! cannot take its page exits, and never serves.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_init_programs::dma_driver::{FAULT, OK, PING};
use redoubt_rt::abi::{FOREVER, Handles};
use redoubt_rt::handle::{Endpoint, Mmio};
use redoubt_rt::ipc::Event;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// Exit codes: what it lacked to serve.
const NO_DEVICE: u32 = 2;
const NO_DMA: u32 = 3;
const NO_ENDPOINT: u32 = 4;

fn serve(startup: &Startup) -> u32 {
    let Some(device) = startup.handle("dev") else { return NO_DEVICE };
    // Held for the life of the instance: its end is what resets the device.
    let Ok(_run) = Mmio::from_handle(device).dma_alloc(1) else { return NO_DMA };
    let Some(endpoint) = startup.handle("dma-driver") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(endpoint);
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                let words = match request.words[0] {
                    FAULT => panic!("dma-driver faults while it serves its client"),
                    PING => OK,
                    _ => MALFORMED,
                };
                let mut close = Handles::new();
                for handle in request.handles.as_slice().iter().flatten() {
                    let _ = close.push(*handle);
                }
                let _ = finish(request, &Outcome { words, send: Handles::new(), close });
            }
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            Ok(_) => {}
            Err(_) => redoubt_init_programs::park(),
        }
    }
}
