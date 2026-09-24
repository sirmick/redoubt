//! The fuzz targets' runs (`servers/ipd/fuzz`), as seeded sweeps: many random inputs through
//! `redoubt_ipd::fake::drive_frames` and `drive_session`. Each checks, as it goes, every frame
//! `ipd` sends (no SYN to the box's own addresses, ARP only for the gateway among them, nothing
//! but TCP) and the socket table's invariants; a panic anywhere is the failure.

use redoubt_ipd::fake::{drive_frames, drive_session};

struct Rng(u64);

impl Rng {
    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len)
            .map(|_| {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                (self.0 >> 24) as u8
            })
            .collect()
    }
}

#[test]
fn randomized_frames() {
    let mut rng = Rng(0x0dd_ba11_c0ff_ee42);
    let (mut steps, mut sent, mut sockets) = (0, 0, 0);
    for i in 0..400 {
        let len = 512 + (i * 37) % 3000;
        let d = drive_frames(&rng.bytes(len));
        (steps, sent, sockets) = (steps + d.steps, sent + d.sent, sockets.max(d.sockets));
    }
    eprintln!("frames: {steps} steps, {sent} frames sent, at most {sockets} sockets");
    assert!(steps > 400 * 20 && sent > 400 * 10 && sockets >= 4, "the sweep stayed shallow");
}

#[test]
fn randomized_sessions() {
    let mut rng = Rng(0x5e55_1015_f00d_4321);
    let mut total = redoubt_ipd::fake::Drove::default();
    for i in 0..400 {
        let len = 512 + (i * 53) % 3000;
        let d = drive_session(&rng.bytes(len));
        total.steps += d.steps;
        total.sent += d.sent;
        total.answered += d.answered;
        total.held += d.held;
        total.minted += d.minted;
        total.sockets = total.sockets.max(d.sockets);
    }
    eprintln!("sessions: {total:?}");
    assert!(
        total.steps > 400 * 30
            && total.answered > 3000
            && total.held > 50
            && total.minted > 500
            && total.sockets >= 8,
        "the sweep stayed shallow: {total:?}"
    );
}
