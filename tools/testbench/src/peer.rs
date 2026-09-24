//! The far side of a `[net]` case's network (answer 174; docs/testbench.md, "Peers"): hosts the
//! guest may reach, connections the bench dials into the guest, and a capture of every frame the
//! guest sent, all judged after the boot by the bench, never by anything in the guest.
//!
//! - **Peers.** Each `[[net.peer]]` is a QEMU `guestfwd` to a program: for every connection the guest makes
//!   to that address, libslirp starts `testbench peer-helper` ([`helper`]) with the connection on its
//!   standard input and output. The helper records the connection as a file before it echoes a byte, so the
//!   count of files is the count of connections, and there is no host listener whose port another process
//!   could take.
//! - **The virtual network.** `guestfwd` only takes addresses inside slirp's network, so a case with peers
//!   widens it to [`VNET`], a /16 in which slirp keeps its usual host (10.0.2.2), DNS alias and guest
//!   address. The peers sit at 10.0.9.x, outside the /24 the guest thinks it is on, and the guest reaches
//!   them through its gateway. `restrict=on` still holds (`qemu.rs`), so nothing else in the /16 reaches the
//!   host.
//! - **The capture.** QEMU's `filter-dump` writes every frame on the guest's network card to a pcap file,
//!   before slirp sees it, so a SYN `restrict=on` would drop is in it too. After the boot it is parsed fail
//!   closed: a missing, empty, cut or malformed file fails the case.
//!
//! What a case with peers must show: every peer counted exactly its `connections`, both in the
//! records and in the capture (distinct source ports of SYNs to it), no SYN from the guest to a
//! `self_forbidden` address, no IPv4 from the guest that is not TCP, nothing that is neither IPv4
//! nor ARP, and no ARP request for anything but the gateway. A peer expecting a connection is the
//! positive control: its SYN in the capture shows that the capture was live.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use crate::case::{Dial, Net};
use crate::qemu::{Forward, Verdict};

/// slirp's network in a case with peers: a /16, with slirp's host, DNS alias and first guest
/// address where they are by default, so the guest's own configuration is the milestone's.
pub const VNET: &str = "net=10.0.0.0/16,host=10.0.2.2,dns=10.0.2.3,dhcpstart=10.0.2.15";
/// The network [`VNET`] names, for checking a peer's address against it.
const VNET_PREFIX: (Ipv4Addr, u8) = (Ipv4Addr::new(10, 0, 0, 0), 16);
/// slirp's own /24 inside it: its host, DNS alias and the guest. No peer may sit there.
const SLIRP_PREFIX: (Ipv4Addr, u8) = (Ipv4Addr::new(10, 0, 2, 0), 24);
/// The guest's gateway, slirp's host: the one address the guest may ask ARP for.
pub const GATEWAY: Ipv4Addr = Ipv4Addr::new(10, 0, 2, 2);
/// The Ethernet source of every frame slirp sends: `52:55` and slirp's host address. A frame from
/// any other source is taken as the guest's, so a mistake here checks more frames, never fewer.
pub const SLIRP_MAC: [u8; 6] = [0x52, 0x55, 10, 0, 2, 2];

/// The hidden subcommand libslirp runs for each connection to a peer.
pub const HELPER: &str = "peer-helper";
/// How long the bench waits, after the boot, for helpers still recording connections the capture
/// shows: a helper is started when the handshake completes and records at once.
const SETTLE: Duration = Duration::from_secs(2);

/// One boot's peer files, beside its disk image: the helpers' records and the capture.
pub struct Files {
    pub records: PathBuf,
    pub capture: PathBuf,
}

impl Files {
    /// The files for the boot whose disk image is `disk` (the log's name, `.img`).
    pub fn beside(disk: &Path) -> Files {
        Files { records: disk.with_extension("peers"), capture: disk.with_extension("pcap") }
    }

    /// Clears what an earlier boot of the same case left, so no old record or frame is counted.
    fn fresh(&self) -> Result<()> {
        if self.records.exists() {
            std::fs::remove_dir_all(&self.records)
                .with_context(|| format!("clearing {}", self.records.display()))?;
        }
        std::fs::create_dir_all(&self.records)?;
        if self.capture.exists() {
            std::fs::remove_file(&self.capture)?;
        }
        Ok(())
    }
}

/// A peer's address: a TCP endpoint in [`VNET`] but outside slirp's own /24.
pub fn parse_peer(addr: &str) -> Result<SocketAddrV4> {
    let parsed: SocketAddrV4 = addr.parse().with_context(|| format!("peer {addr:?}: not ADDR:PORT"))?;
    ensure!(contains(VNET_PREFIX, *parsed.ip()), "peer {addr}: outside {VNET}");
    ensure!(!contains(SLIRP_PREFIX, *parsed.ip()), "peer {addr}: inside slirp's own 10.0.2.0/24");
    ensure!(parsed.port() != 0, "peer {addr}: port 0");
    Ok(parsed)
}

/// An IPv4 prefix `A.B.C.D/len` with its host bits zero.
pub fn parse_prefix(text: &str) -> Result<(Ipv4Addr, u8)> {
    let (addr, len) = text.split_once('/').with_context(|| format!("{text:?}: not A.B.C.D/len"))?;
    let addr: Ipv4Addr = addr.parse().with_context(|| format!("{text:?}: not an IPv4 address"))?;
    let len: u8 =
        len.parse().ok().filter(|l| *l <= 32).with_context(|| format!("{text:?}: a length over 32"))?;
    ensure!(u32::from(addr) & !mask(len) == 0, "{text:?}: host bits set");
    Ok((addr, len))
}

fn mask(len: u8) -> u32 { u32::MAX.checked_shl(32 - u32::from(len)).unwrap_or(0) }

fn contains((net, len): (Ipv4Addr, u8), addr: Ipv4Addr) -> bool {
    u32::from(addr) & mask(len) == u32::from(net) & mask(len)
}

/// QEMU's option syntax separates with commas; a comma inside a value is doubled.
fn qemu_value(text: &str) -> String { text.replace(',', ",,") }

/// The `-netdev user` options a case's peers add: the wider network and one `guestfwd` each. None
/// for a case without peers, which keeps slirp's default /24. Clears the boot's peer files.
pub fn netdev_options(net: &Net, files: &Files) -> Result<String> {
    if net.peer.is_empty() {
        return Ok(String::new());
    }
    files.fresh()?;
    let helper = std::env::current_exe().context("the bench's own path")?;
    let mut options = format!(",{VNET}");
    for peer in &net.peer {
        let addr = parse_peer(&peer.addr)?;
        // libslirp splits the command like a shell (g_shell_parse_argv) and runs it without one.
        let command = [
            helper.to_string_lossy().as_ref(),
            HELPER,
            "--id",
            &addr.to_string(),
            "--dir",
            files.records.to_string_lossy().as_ref(),
        ]
        .map(crate::qemu::shell_quote)
        .join(" ");
        options += &format!(",guestfwd=tcp:{}:{}-cmd:{}", addr.ip(), addr.port(), qemu_value(&command));
    }
    Ok(options)
}

/// The QEMU arguments that capture the guest's frames, for a case with peers.
pub fn capture_args(net: &Net, files: &Files) -> Vec<String> {
    if net.peer.is_empty() {
        return Vec::new();
    }
    let file = qemu_value(&files.capture.to_string_lossy());
    vec!["-object".into(), format!("filter-dump,id=capture0,netdev=net0,file={file}")]
}

// --- The helper ---------------------------------------------------------------------------------

/// `testbench peer-helper --id ADDR:PORT --dir DIR`: one guest connection to a peer, on standard
/// input and output. Records it as `DIR/ADDR:PORT.<pid>.<random>` (created exclusively, a new name
/// on a collision), then echoes until the guest closes. If it cannot record, it closes without
/// echoing a byte, so the guest never sees a connection the bench did not count. It writes nothing
/// else anywhere: libslirp gives it the connection as standard error too.
pub fn helper(args: impl Iterator<Item = String>) -> ! {
    let code = match record(args) {
        Ok(()) => {
            echo();
            0
        }
        Err(_) => 1,
    };
    std::process::exit(code)
}

fn record(mut args: impl Iterator<Item = String>) -> Result<()> {
    let (mut id, mut dir) = (None, None);
    while let Some(flag) = args.next() {
        match (flag.as_str(), args.next()) {
            ("--id", Some(value)) => id = Some(value),
            ("--dir", Some(value)) => dir = Some(PathBuf::from(value)),
            _ => bail!("usage: {HELPER} --id ADDR:PORT --dir DIR"),
        }
    }
    let (Some(id), Some(dir)) = (id, dir) else { bail!("usage: {HELPER} --id ADDR:PORT --dir DIR") };
    let id: SocketAddrV4 = id.parse()?;
    for _ in 0..16 {
        let mut random = [0u8; 8];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let name = format!("{id}.{}.{:016x}", std::process::id(), u64::from_le_bytes(random));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(name)) {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("no free record name")
}

fn echo() {
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout().lock());
    let mut buf = [0u8; 4096];
    while let Ok(n) = input.read(&mut buf) {
        if n == 0 || output.write_all(&buf[..n]).and_then(|_| output.flush()).is_err() {
            break;
        }
    }
}

/// The connections recorded in `dir`, per peer. A name that is not a record fails.
fn count_records(dir: &Path) -> Result<BTreeMap<SocketAddrV4, u32>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("peer records {}: {e}", dir.display()))?;
    let mut counts = BTreeMap::new();
    for entry in entries {
        let name = entry.map_err(|e| format!("peer records: {e}"))?.file_name();
        let name = name.to_string_lossy();
        let mut parts = name.rsplitn(3, '.');
        let (random, pid, id) = (parts.next(), parts.next(), parts.next());
        let id = match (random, pid, id) {
            (Some(r), Some(p), Some(id)) if r.len() == 16 && p.parse::<u32>().is_ok() => id.parse().ok(),
            _ => None,
        };
        let Some(id) = id else { return Err(format!("peer records: {name:?} is not a record")) };
        *counts.entry(id).or_insert(0) += 1;
    }
    Ok(counts)
}

// --- Dials --------------------------------------------------------------------------------------

/// Connections the bench makes into the guest through its forwarded ports, while it boots.
pub struct Dials {
    stop: Arc<AtomicBool>,
    running: Vec<(String, JoinHandle<Result<(), String>>)>,
}

impl Dials {
    /// Starts every `[[net.dial]]`, each retrying until it is answered or `deadline` passes.
    pub fn start(net: &Net, forwards: &[Forward], deadline: Instant) -> Result<Dials> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut running = Vec::new();
        for dial in &net.dial {
            let host = forwards
                .iter()
                .find(|(guest, _)| *guest == dial.port)
                .map(|(_, host)| *host)
                .with_context(|| format!("dial to guest port {}: not in net.forward", dial.port))?;
            let (dial, stop) = (dial.clone(), stop.clone());
            let name = format!("dial :{}", dial.port);
            running.push((name, std::thread::spawn(move || dial_until(&dial, host, deadline, &stop))));
        }
        Ok(Dials { stop, running })
    }

    /// Stops the dials still trying and says why the first unanswered one failed.
    fn finish(self) -> Option<String> {
        self.stop.store(true, Ordering::Relaxed);
        let mut failure = None;
        for (name, thread) in self.running {
            let result = thread.join().unwrap_or_else(|_| Err("the dial panicked".into()));
            if let (Err(why), None) = (result, &failure) {
                failure = Some(format!("{name}: {why}"));
            }
        }
        failure
    }
}

/// Dials the guest port forwarded to `host` until one connection echoes `dial.expect` for
/// `dial.send`. Before the guest listens, slirp takes the connection and then closes it, which is
/// only a retry.
fn dial_until(dial: &Dial, host: u16, deadline: Instant, stop: &AtomicBool) -> Result<(), String> {
    let mut last = String::from("never connected");
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        match dial_once(dial, host, deadline) {
            Ok(()) => return Ok(()),
            Err(why) => last = why,
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("{:?} was not echoed as {:?}: {last}", dial.send, dial.expect))
}

fn dial_once(dial: &Dial, host: u16, deadline: Instant) -> Result<(), String> {
    let to = std::net::SocketAddr::from(([127, 0, 0, 1], host));
    let mut stream = TcpStream::connect_timeout(&to, Duration::from_secs(1)).map_err(|e| e.to_string())?;
    stream.write_all(dial.send.as_bytes()).map_err(|e| e.to_string())?;
    let mut got = Vec::new();
    let mut buf = [0u8; 1024];
    while !got.windows(dial.expect.len().max(1)).any(|w| w == dial.expect.as_bytes()) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!("got {:?} by the deadline", String::from_utf8_lossy(&got)));
        }
        stream.set_read_timeout(Some(left.min(Duration::from_millis(500)))).map_err(|e| e.to_string())?;
        match stream.read(&mut buf) {
            Ok(0) => return Err(format!("closed after {:?}", String::from_utf8_lossy(&got))),
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

// --- The capture --------------------------------------------------------------------------------

/// The frames of a pcap file as QEMU's `filter-dump` writes it: little-endian, microsecond
/// timestamps, version 2.4, Ethernet. Anything else, a cut frame, trailing bytes or no frame at all
/// is refused: a capture the bench cannot read whole proves nothing.
pub fn frames(bytes: &[u8]) -> Result<Vec<&[u8]>, String> {
    let u32_at = |at: usize| -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) };
    if bytes.len() < 24 {
        return Err(format!("the capture is {} bytes, shorter than a pcap header", bytes.len()));
    }
    if u32_at(0) != 0xa1b2_c3d4 {
        return Err(format!("the capture's magic is {:#010x}, not a little-endian pcap", u32_at(0)));
    }
    if bytes[4..8] != [2, 0, 4, 0] {
        return Err("the capture is not pcap version 2.4".into());
    }
    let snaplen = u32_at(16) as usize;
    if u32_at(20) != 1 {
        return Err(format!("the capture's link type is {}, not Ethernet", u32_at(20)));
    }
    let mut frames = Vec::new();
    let mut at = 24;
    while at < bytes.len() {
        if bytes.len() - at < 16 {
            return Err(format!("the capture ends inside a frame header, at byte {at}"));
        }
        let (captured, length) = (u32_at(at + 8) as usize, u32_at(at + 12) as usize);
        if captured != length || captured > snaplen {
            return Err(format!("frame {} was cut: {captured} of {length} bytes", frames.len()));
        }
        at += 16;
        if bytes.len() - at < captured {
            return Err(format!("the capture ends inside frame {}", frames.len()));
        }
        frames.push(&bytes[at..at + captured]);
        at += captured;
    }
    if frames.is_empty() {
        return Err("the capture holds no frame".into());
    }
    Ok(frames)
}

/// What the guest sent, as far as the judgment needs it: for each destination, the source ports
/// of its SYNs, one per connection attempt (a retransmitted SYN keeps its port).
#[derive(Debug, Default)]
pub struct Sent {
    pub syns: BTreeMap<SocketAddrV4, BTreeSet<u16>>,
}

/// Checks every frame the guest sent against the rules in the module comment and gathers its
/// SYNs. The first frame breaking a rule is the reason.
pub fn inspect(frames: &[&[u8]], self_forbidden: &[(Ipv4Addr, u8)]) -> Result<Sent, String> {
    let mut sent = Sent::default();
    for (i, frame) in frames.iter().enumerate() {
        let fail = |why: String| Err(format!("captured frame {i}: {why}"));
        if frame.len() < 14 {
            return fail(format!("{} bytes, shorter than an Ethernet header", frame.len()));
        }
        if frame[6..12] == SLIRP_MAC {
            continue;
        }
        let payload = &frame[14..];
        match u16::from_be_bytes([frame[12], frame[13]]) {
            0x0806 => {
                if payload.len() < 28 || payload[..6] != [0, 1, 8, 0, 6, 4] {
                    return fail("the guest sent a malformed ARP packet".into());
                }
                let target = Ipv4Addr::new(payload[24], payload[25], payload[26], payload[27]);
                match u16::from_be_bytes([payload[6], payload[7]]) {
                    1 if target == GATEWAY => {}
                    1 => return fail(format!("the guest asked ARP for {target}, not the gateway {GATEWAY}")),
                    2 => {}
                    op => return fail(format!("the guest sent ARP operation {op}")),
                }
            }
            0x0800 => {
                if payload.len() < 20 || payload[0] >> 4 != 4 || usize::from(payload[0] & 0xf) < 5 {
                    return fail("the guest sent a malformed IPv4 header".into());
                }
                let header = usize::from(payload[0] & 0xf) * 4;
                let total = usize::from(u16::from_be_bytes([payload[2], payload[3]]));
                let dst = Ipv4Addr::new(payload[16], payload[17], payload[18], payload[19]);
                if total < header || total > payload.len() {
                    return fail(format!("the guest sent an IPv4 packet to {dst} whose length is wrong"));
                }
                if u16::from_be_bytes([payload[6], payload[7]]) & 0x3fff != 0 {
                    return fail(format!("the guest sent an IPv4 fragment to {dst}"));
                }
                if payload[9] != 6 {
                    return fail(format!("the guest sent IPv4 protocol {} to {dst}, not TCP", payload[9]));
                }
                let tcp = &payload[header..total];
                if tcp.len() < 20 {
                    return fail(format!("the guest sent a TCP segment to {dst} shorter than its header"));
                }
                let (from, to) = (u16::from_be_bytes([tcp[0], tcp[1]]), u16::from_be_bytes([tcp[2], tcp[3]]));
                let (syn, ack) = (tcp[13] & 0x02 != 0, tcp[13] & 0x10 != 0);
                if syn && !ack {
                    if self_forbidden.iter().any(|p| contains(*p, dst)) {
                        return fail(format!(
                            "the guest sent a SYN to {dst}:{to}, one of the box's own addresses"
                        ));
                    }
                    sent.syns.entry(SocketAddrV4::new(dst, to)).or_default().insert(from);
                }
            }
            other => return fail(format!("the guest sent ethertype {other:#06x}, neither IPv4 nor ARP")),
        }
    }
    Ok(sent)
}

// --- The judgment -------------------------------------------------------------------------------

/// Stops a boot's dials once it has run, and (only if the boot passed so far) fails it for the
/// first dial that was never answered.
pub fn finish_dials(verdict: Verdict, dials: Option<Dials>) -> Verdict {
    let dialled = dials.and_then(Dials::finish);
    match (verdict, dialled) {
        (Verdict::Pass(_), Some(why)) => Verdict::Fail(why),
        (verdict, _) => verdict,
    }
}

/// Judges a case's peers and capture on one boot's `files`, after it passed (the bench's
/// post-check): the capture's rules, then every peer's count in the records and in the capture.
pub fn judge_peers(net: &Net, files: &Files) -> Result<(), String> {
    let mut bytes =
        std::fs::read(&files.capture).map_err(|e| format!("the capture {}: {e}", files.capture.display()))?;
    if let Some(len) = net.truncate_capture {
        bytes.truncate(len as usize);
    }
    let forbidden = net.self_forbidden.iter().map(|p| parse_prefix(p)).collect::<Result<Vec<_>>>();
    let forbidden = forbidden.map_err(|e| format!("{e:#}"))?;
    let sent = inspect(&frames(&bytes)?, &forbidden)?;
    let peers =
        net.peer.iter().map(|p| Ok((parse_peer(&p.addr)?, p.connections))).collect::<Result<Vec<_>>>();
    let peers = peers.map_err(|e| format!("{e:#}"))?;
    let attempts = |addr: &SocketAddrV4| sent.syns.get(addr).map_or(0, |ports| ports.len() as u32);
    // A helper records right after the handshake; wait a moment for any the capture shows.
    let settle = Instant::now() + SETTLE;
    let mut records = count_records(&files.records)?;
    while Instant::now() < settle
        && peers.iter().any(|(a, _)| records.get(a).copied().unwrap_or(0) < attempts(a))
    {
        std::thread::sleep(Duration::from_millis(50));
        records = count_records(&files.records)?;
    }
    if let Some(stray) = records.keys().find(|a| !peers.iter().any(|(p, _)| p == *a)) {
        return Err(format!("records of connections to {stray}, which is no peer"));
    }
    for (addr, expected) in &peers {
        let got = records.get(addr).copied().unwrap_or(0);
        if got != *expected {
            return Err(format!("peer {addr}: {got} connections, expected {expected}"));
        }
    }
    for (addr, expected) in &peers {
        let got = attempts(addr);
        if got != *expected {
            return Err(format!(
                "peer {addr}: {got} connection attempts in the capture, expected {expected}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUEST_MAC: [u8; 6] = [0x52, 0x54, 0, 0x12, 0x34, 0x56];

    fn pcap(frames: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [0xa1b2_c3d4u32, 0x0004_0002, 0, 0, 65536, 1] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for frame in frames {
            for word in [0u32, 0, frame.len() as u32, frame.len() as u32] {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out.extend_from_slice(frame);
        }
        out
    }

    fn ethernet(src: [u8; 6], kind: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0xff; 6];
        f.extend_from_slice(&src);
        f.extend_from_slice(&kind.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    fn tcp(src: [u8; 6], dst: [u8; 4], from: u16, to: u16, flags: u8) -> Vec<u8> {
        let mut ip = vec![0x45, 0, 0, 40, 0, 0, 0x40, 0, 64, 6, 0, 0, 10, 0, 2, 15];
        ip.extend_from_slice(&dst);
        let mut seg = vec![0u8; 20];
        seg[0..2].copy_from_slice(&from.to_be_bytes());
        seg[2..4].copy_from_slice(&to.to_be_bytes());
        seg[12] = 5 << 4;
        seg[13] = flags;
        ip.extend_from_slice(&seg);
        ethernet(src, 0x0800, &ip)
    }

    fn arp(src: [u8; 6], op: u16, target: [u8; 4]) -> Vec<u8> {
        let mut p = vec![0, 1, 8, 0, 6, 4];
        p.extend_from_slice(&op.to_be_bytes());
        p.extend_from_slice(&src);
        p.extend_from_slice(&[10, 0, 2, 15]);
        p.extend_from_slice(&[0; 6]);
        p.extend_from_slice(&target);
        ethernet(src, 0x0806, &p)
    }

    const SYN: u8 = 0x02;
    const SYN_ACK: u8 = 0x12;

    fn judge(frames: &[Vec<u8>]) -> Result<Sent, String> {
        let bytes = pcap(frames);
        inspect(
            &super::frames(&bytes)?,
            &[parse_prefix("10.0.2.0/24").unwrap(), parse_prefix("10.0.9.102/32").unwrap()],
        )
    }

    /// A capture is read whole or not at all: every way of being short, cut or foreign fails.
    #[test]
    fn a_capture_is_read_fail_closed() {
        let good = pcap(&[tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN)]);
        assert_eq!(frames(&good).unwrap().len(), 1);
        assert!(frames(&[]).unwrap_err().contains("shorter than a pcap header"));
        assert!(frames(&good[..24]).unwrap_err().contains("no frame"));
        assert!(frames(&good[..30]).unwrap_err().contains("inside a frame header"));
        assert!(frames(&good[..good.len() - 1]).unwrap_err().contains("inside frame 0"));
        let mut foreign = good.clone();
        foreign[20] = 101;
        assert!(frames(&foreign).unwrap_err().contains("link type"));
        let mut swapped = good.clone();
        swapped[..4].copy_from_slice(&0xa1b2_c3d4u32.to_be_bytes());
        assert!(frames(&swapped).unwrap_err().contains("magic"));
        let mut cut = good.clone();
        cut[24 + 12] += 1;
        assert!(frames(&cut).unwrap_err().contains("was cut"));
    }

    /// The rules on what the guest may send, each broken once, and the frames that are allowed.
    #[test]
    fn what_the_guest_sends_is_checked() {
        let sent = judge(&[
            arp(GUEST_MAC, 1, [10, 0, 2, 2]),
            arp(GUEST_MAC, 2, [10, 0, 2, 2]),
            tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN),
            tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN),
            tcp(GUEST_MAC, [10, 0, 9, 100], 50001, 7, SYN),
            // Answers to hostfwd connections come from the guest's own address to 10.0.2.2.
            tcp(GUEST_MAC, [10, 0, 2, 2], 8000, 40000, SYN_ACK),
            // slirp's own frames are not the guest's, whatever they carry.
            tcp(SLIRP_MAC, [10, 0, 2, 15], 7, 50000, SYN),
            ethernet(SLIRP_MAC, 0x86dd, &[0; 40]),
        ])
        .unwrap();
        let peer = SocketAddrV4::new(Ipv4Addr::new(10, 0, 9, 100), 7);
        assert_eq!(sent.syns[&peer].len(), 2, "two attempts, one retransmitted");
        assert_eq!(sent.syns.len(), 1);

        let refused = |frame: Vec<u8>, why: &str| {
            let err = judge(&[frame]).unwrap_err();
            assert!(err.contains(why), "{err:?} lacks {why:?}");
        };
        refused(tcp(GUEST_MAC, [10, 0, 9, 102], 50000, 7, SYN), "SYN to 10.0.9.102:7, one of the box's own");
        refused(tcp(GUEST_MAC, [10, 0, 2, 2], 50000, 22, SYN), "SYN to 10.0.2.2:22");
        refused(arp(GUEST_MAC, 1, [10, 0, 2, 3]), "asked ARP for 10.0.2.3");
        refused(ethernet(GUEST_MAC, 0x86dd, &[0; 40]), "ethertype 0x86dd");
        refused(ethernet(GUEST_MAC, 0x0806, &[0; 10]), "malformed ARP");
        let mut udp = tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN);
        udp[14 + 9] = 17;
        refused(udp, "IPv4 protocol 17");
        let mut fragment = tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN);
        fragment[14 + 6] = 0x20;
        refused(fragment, "fragment");
        refused(ethernet(GUEST_MAC, 0x0800, &[0x45; 10]), "malformed IPv4");
        refused(vec![0; 13], "shorter than an Ethernet header");
    }

    #[test]
    fn prefixes_and_peers_are_checked() {
        assert_eq!(parse_prefix("10.0.2.0/24").unwrap(), (Ipv4Addr::new(10, 0, 2, 0), 24));
        assert_eq!(parse_prefix("0.0.0.0/0").unwrap(), (Ipv4Addr::new(0, 0, 0, 0), 0));
        assert!(parse_prefix("10.0.2.1/24").is_err(), "host bits");
        assert!(parse_prefix("10.0.2.0/33").is_err());
        assert!(parse_prefix("10.0.2.0").is_err());
        assert!(parse_peer("10.0.9.100:7").is_ok());
        assert!(parse_peer("10.0.2.9:7").is_err(), "slirp's own /24");
        assert!(parse_peer("10.1.9.100:7").is_err(), "outside the /16");
        assert!(parse_peer("10.0.9.100:0").is_err());
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("testbench-peer-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The helper records before anything else, under a name no other connection takes, and the
    /// counting refuses anything in the directory that is not a record.
    #[test]
    fn records_are_counted_per_peer() {
        let dir = scratch("records");
        let args = |id: &str| {
            ["--id", id, "--dir", dir.to_str().unwrap()].map(String::from).into_iter().collect::<Vec<_>>()
        };
        record(args("10.0.9.100:7").into_iter()).unwrap();
        record(args("10.0.9.100:7").into_iter()).unwrap();
        record(args("10.0.9.110:7").into_iter()).unwrap();
        assert!(record(args("not-an-address").into_iter()).is_err());
        assert!(record(["--id".to_string()].into_iter()).is_err());
        let counts = count_records(&dir).unwrap();
        assert_eq!(counts[&"10.0.9.100:7".parse().unwrap()], 2);
        assert_eq!(counts[&"10.0.9.110:7".parse().unwrap()], 1);
        std::fs::write(dir.join("stray"), b"").unwrap();
        assert!(count_records(&dir).unwrap_err().contains("not a record"));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn net(toml: &str) -> Net { toml::from_str(toml).expect("a [net] table") }

    /// The whole judgment on files: exact counts both ways, and each reason worded as the
    /// self-check cases quote it.
    #[test]
    fn peers_are_judged_on_records_and_capture() {
        let dir = scratch("judge");
        let files = Files::beside(&dir.join("boot.img"));
        let case =
            net("self_forbidden = ['10.0.2.0/24']\n[[peer]]\naddr = '10.0.9.100:7'\nconnections = 1\n\
             [[peer]]\naddr = '10.0.9.101:7'\nconnections = 0\n");
        netdev_options(&case, &files).unwrap();
        let add_record = |id: &str| {
            let args = ["--id", id, "--dir", files.records.to_str().unwrap()].map(String::from);
            record(args.into_iter()).unwrap();
        };
        std::fs::write(&files.capture, pcap(&[tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN)])).unwrap();
        assert_eq!(judge_peers(&case, &files), Err("peer 10.0.9.100:7: 0 connections, expected 1".into()));
        add_record("10.0.9.100:7");
        assert_eq!(judge_peers(&case, &files), Ok(()));
        add_record("10.0.9.100:7");
        assert_eq!(judge_peers(&case, &files), Err("peer 10.0.9.100:7: 2 connections, expected 1".into()));

        netdev_options(&case, &files).unwrap();
        add_record("10.0.9.100:7");
        // A SYN the records do not show (its helper never ran): the capture still counts it.
        std::fs::write(
            &files.capture,
            pcap(&[
                tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN),
                tcp(GUEST_MAC, [10, 0, 9, 101], 50001, 7, SYN),
            ]),
        )
        .unwrap();
        let err = judge_peers(&case, &files).unwrap_err();
        assert_eq!(err, "peer 10.0.9.101:7: 1 connection attempts in the capture, expected 0");
        // A capture that is missing or empty fails whatever the records say.
        std::fs::remove_file(&files.capture).unwrap();
        assert!(judge_peers(&case, &files).unwrap_err().starts_with("the capture "));
        std::fs::write(&files.capture, pcap(&[])).unwrap();
        assert_eq!(judge_peers(&case, &files), Err("the capture holds no frame".into()));
        let cut = net("truncate_capture = 0\n[[peer]]\naddr = '10.0.9.100:7'\nconnections = 1\n");
        std::fs::write(&files.capture, pcap(&[tcp(GUEST_MAC, [10, 0, 9, 100], 50000, 7, SYN)])).unwrap();
        assert!(judge_peers(&cut, &files).unwrap_err().contains("shorter than a pcap header"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A dial is answered only by the echo it expects; a guest port nobody answers on fails it by
    /// the deadline, saying what it got.
    #[test]
    fn a_dial_needs_its_echo() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 16];
                let n = stream.read(&mut buf).unwrap();
                stream.write_all(&buf[..n]).unwrap();
            }
        });
        let stop = AtomicBool::new(false);
        let soon = Instant::now() + Duration::from_secs(5);
        let echo = Dial { port: 8000, send: "hello".into(), expect: "hello".into() };
        assert_eq!(dial_until(&echo, port, soon, &stop), Ok(()));
        let other = Dial { port: 8000, send: "hello".into(), expect: "goodbye".into() };
        let soon = Instant::now() + Duration::from_millis(700);
        let err = dial_until(&other, port, soon, &stop).unwrap_err();
        assert!(err.contains("\"hello\""), "{err}");
    }
}
