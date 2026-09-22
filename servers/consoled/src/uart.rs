//! The ns16550 UART, as QEMU's `virt` machine and the FPGA card present it: eight one-byte
//! registers, one byte per address (no register shift).
//!
//! It reaches the registers through [`Registers`], which the runtime maps from the device handle
//! and bounds-checks, so there is no `unsafe` here. Every access is volatile, which is what a
//! FIFO needs: reading `RBR` pops a byte, so an access the compiler dropped or duplicated would
//! lose or invent input.
//!
//! **Two threads, two halves.** `consoled` keeps the whole [`Uart`] on the thread that serves
//! 9P; its interrupt thread touches no register at all and only says "something arrived"
//! (`src/bin/consoled.rs`). [`Registers`] is not `Sync`, so that split is the type system's and
//! not a convention.

use redoubt_rt::handle::Registers;

/// Receive buffer (read) and transmit holding register (write).
const RBR_THR: usize = 0;
/// Interrupt enable; the divisor's low byte while `DLAB` is set.
const IER_DLM: usize = 1;
/// FIFO control (write).
const FCR: usize = 2;
/// Line control.
const LCR: usize = 3;
/// Modem control.
const MCR: usize = 4;
/// Line status.
const LSR: usize = 5;
/// The registers a 16550 has; a mapping shorter than this is not one.
pub const REGISTERS: usize = 8;
/// The receive FIFO's depth: the most bytes a 16550 can be holding, and so the most one drain
/// of it can honestly produce.
pub const FIFO: usize = 16;

/// `IER`: an interrupt when received data is available.
const IER_RX: u8 = 0x01;
/// `FCR`: enable the FIFOs and clear both of them.
const FCR_ENABLE_AND_CLEAR: u8 = 0x07;
/// `LCR`: eight data bits, no parity, one stop bit.
const LCR_8N1: u8 = 0x03;
/// `LCR`: the divisor latch is mapped over the first two registers.
const LCR_DLAB: u8 = 0x80;
/// `MCR`: DTR, RTS and OUT2. OUT2 gates the interrupt line on a 16550 and in QEMU's model, so
/// without it no byte ever raises an interrupt.
const MCR_DTR_RTS_OUT2: u8 = 0x0b;
/// `LSR`: there is a byte to read.
const LSR_DATA_READY: u8 = 0x01;
/// `LSR`: the transmit holding register is empty.
const LSR_THR_EMPTY: u8 = 0x20;

/// How many times [`Uart::put`] looks at `LSR` before giving up on a byte. The transmitter
/// empties in a character time, so this is generous; the point is that it is finite, because a
/// server must not spin for ever on a device that has stopped (CONTAINMENT.md: a server parks
/// calls rather than blocking, and never blocks on hardware either).
const TX_TRIES: u32 = 10_000;

/// One ns16550.
pub struct Uart {
    regs: Registers,
}

impl Uart {
    /// The UART at `regs`, or `None` if the mapping is too short to be one.
    pub fn new(regs: Registers) -> Option<Uart> { (regs.len() >= REGISTERS).then_some(Uart { regs }) }

    /// A register's value. The bounds were checked in [`Uart::new`], so this cannot be `None`;
    /// 0 is the safe reading if it ever were (no data ready, no room to send).
    fn read(&self, at: usize) -> u8 { self.regs.read_u8(at).unwrap_or(0) }

    fn write(&self, at: usize, value: u8) { self.regs.write_u8(at, value); }

    /// 8N1, FIFOs on and cleared, and an interrupt on received data.
    pub fn init(&self) {
        // Interrupts off while the registers are in flux.
        self.write(IER_DLM, 0);
        // A divisor of 1: QEMU ignores the baud rate, and on the FPGA card the divisor is set
        // by the board's own clock, so this only leaves the latch in a defined state.
        self.write(LCR, LCR_DLAB);
        self.write(RBR_THR, 1);
        self.write(IER_DLM, 0);
        self.write(LCR, LCR_8N1);
        self.write(FCR, FCR_ENABLE_AND_CLEAR);
        self.write(MCR, MCR_DTR_RTS_OUT2);
        self.write(IER_DLM, IER_RX);
    }

    /// The next byte the line delivered, or `None` if the FIFO is empty.
    pub fn take(&self) -> Option<u8> { (self.read(LSR) & LSR_DATA_READY != 0).then(|| self.read(RBR_THR)) }

    /// Sends one byte; false if the transmitter did not make room within [`TX_TRIES`] looks,
    /// which a caller reports as a short write rather than spinning.
    pub fn put(&self, byte: u8) -> bool {
        for _ in 0..TX_TRIES {
            if self.read(LSR) & LSR_THR_EMPTY != 0 {
                self.write(RBR_THR, byte);
                return true;
            }
        }
        false
    }

    /// Sends what it can of `bytes`; how many went out. A short count is a short 9P write.
    pub fn put_all(&self, bytes: &[u8]) -> usize { bytes.iter().take_while(|byte| self.put(**byte)).count() }
}
