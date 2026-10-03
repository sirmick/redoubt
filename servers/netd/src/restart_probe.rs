//! The bench's `netd-restart` probe (docs/testbench.md, "Peers, dials and the capture"): what the
//! case and `netd`'s test-only feature `restart-probe` agree on. Only that feature acts on it; a
//! default build compiles these matchers and never calls them.
//!
//! - **The trigger** is one UDP datagram the bench sends into the guest from outside, carrying [`PAYLOAD`] to
//!   port [`PORT`]. Nothing resends a datagram, so the instance `init` starts after the fault never sees it,
//!   which a count inside `netd` could not promise.
//! - **The instance call** is a call carrying [`INSTANCE`] first, from any badge, which `netd` answers with
//!   32 bits it drew at random when it started: its caller tells the instance `init` started after the fault
//!   from the one before. `netd`'s endpoint is `init`'s and outlives an instance, so a call made after the
//!   fault waits for the new one.

/// The guest UDP port the poke is sent to; nothing listens on it.
pub const PORT: u16 = 47_000;

/// The poke's whole UDP payload.
pub const PAYLOAD: &[u8] = b"redoubt netd restart probe";

/// The first word of the instance call. Words are 32 bits on rv32, so it is too.
pub const INSTANCE: u64 = 0x7072_6f62;

/// Whether `frame`, an Ethernet frame from the wire, is the poke: IPv4, UDP, to [`PORT`], with
/// exactly [`PAYLOAD`] (the UDP length says where it ends).
pub fn is_poke(frame: &[u8]) -> bool {
    const ETHER: usize = 14;
    let Some(ip) = frame.get(ETHER..) else { return false };
    if frame[12..14] != [0x08, 0x00] || ip.len() < 20 || ip[0] >> 4 != 4 || ip[9] != 17 {
        return false;
    }
    let Some(udp) = ip.get(usize::from(ip[0] & 0x0f) * 4..) else { return false };
    let Some(header) = udp.get(..8) else { return false };
    let length = usize::from(u16::from_be_bytes([header[4], header[5]]));
    u16::from_be_bytes([header[2], header[3]]) == PORT && udp.get(8..length) == Some(PAYLOAD)
}

/// Whether a call's words are the instance call.
pub fn is_instance(words: &[u64; 4]) -> bool { words[0] == INSTANCE }

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    fn datagram(port: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 12];
        f.extend_from_slice(&[0x08, 0x00]);
        let mut ip = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 2, 2, 10, 0, 2, 15];
        let total = (20 + 8 + payload.len()) as u16;
        ip[2..4].copy_from_slice(&total.to_be_bytes());
        f.extend_from_slice(&ip);
        f.extend_from_slice(&40000u16.to_be_bytes());
        f.extend_from_slice(&port.to_be_bytes());
        f.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
        f.extend_from_slice(&[0, 0]);
        f.extend_from_slice(payload);
        f
    }

    /// Only the exact datagram is the poke: not another port, another payload, a longer one, TCP
    /// carrying the same bytes, or a cut frame.
    #[test]
    fn only_the_poke_is_the_poke() {
        assert!(is_poke(&datagram(PORT, PAYLOAD)));
        // Ethernet pads a short frame: the UDP length decides.
        let mut padded = datagram(PORT, PAYLOAD);
        padded.extend_from_slice(&[0; 8]);
        assert!(is_poke(&padded));
        assert!(!is_poke(&datagram(PORT + 1, PAYLOAD)));
        assert!(!is_poke(&datagram(PORT, b"redoubt netd restart probf")));
        assert!(!is_poke(&datagram(PORT, &[PAYLOAD, b"!"].concat())));
        let mut tcp = datagram(PORT, PAYLOAD);
        tcp[14 + 9] = 6;
        assert!(!is_poke(&tcp));
        let whole = datagram(PORT, PAYLOAD);
        for cut in 0..whole.len() {
            assert!(!is_poke(&whole[..cut]), "cut at {cut}");
        }
        assert!(is_instance(&[INSTANCE, 0, 0, 0]) && !is_instance(&[0, INSTANCE, 0, 0]));
    }
}
