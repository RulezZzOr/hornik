//! Real BM1398 (Antminer S19 / BHB428xx) serial protocol primitives.
//!
//! # Provenance
//!
//! The ground truth for this module is the stock `bmminer` binary plus the
//! `bitmain_axi.ko` / `fpga_mem_driver.ko` kernel modules recovered from the
//! vendor `Antminer-S19-merge-release-*.bmu` update package. Confirmed facts
//! used here:
//!
//! * **Transport.** An S19 XIL control board (Zynq-7007S) does *not* talk to the
//!   hashboards over a raw tty. The CPU reaches the BM1398 chain through an FPGA
//!   in the Zynq PL, memory-mapped via `/dev/axi_fpga_dev` (`bitmain_axi.ko`).
//!   The FPGA owns the chip-side UART, the work TX FIFO and the nonce RX FIFO.
//!   The previous `omo-asic/*/v1` text frames written to `/dev/ttyPS0` were a
//!   development placeholder, not the real bus.
//! * **Framing.** The stock firmware default is `bitmain-use-vil = true`, i.e.
//!   the "VIL" command frame format shared across the BM139x/BM136x family.
//! * **PLL.** `bmminer` logs `final refdiv, fbdiv, postdiv1, postdiv2`, i.e. the
//!   standard BM13xx PLL: `freq = 25 MHz * fbdiv / (refdiv * postdiv1 * postdiv2)`.
//! * **Operating point.** Factory `cgminer.conf` ships `bitmain-freq = 400`,
//!   `bitmain-voltage = 1420`, `bitmain-freq-level = 100`.
//!
//! # Confidence and safety
//!
//! The CRC-5, CRC-16 and PLL math here are deterministic and fully unit-tested.
//! The VIL register-write framing, its opcodes (`0x41`/`0x51`), the CRC-5
//! routine, the preamble handling and the FPGA command path are CONFIRMED
//! against a Ghidra decompile of the stock `bmminer`. Items still tagged
//! `NEEDS-HW-CONFIRM` (nonce/work FIFO offsets, full chip register map, work
//! wire layout) must be validated against `bmminer` or a logic-analyzer capture
//! before any real ASIC write is enabled. The hardware safety gate keeps those
//! writes disabled until then, so this module is a *codec*: it builds and parses
//! bytes, it does not by itself drive hardware.

/// Crystal reference clock feeding the BM1398 PLL, in MHz.
pub const CRYSTAL_MHZ: u32 = 25;

/// Factory default per-chip frequency (MHz), from stock `cgminer.conf`.
pub const DEFAULT_FREQUENCY_MHZ: u16 = 400;

/// FPGA register map for the Bitmain Zynq AXI bridge (`/dev/axi_fpga_dev`).
///
/// Offsets are word indices into the mmap'd register window and follow the
/// public Bitmain Zynq ("C5"/SoC) driver layout. `NEEDS-HW-CONFIRM` against the
/// user's `bitmain_axi.ko` mapping before issuing real writes.
pub mod fpga {
    /// Read-only hardware/FPGA version word. CONFIRMED: stock bmminer reads
    /// register index 0 to log `FPGA Version` (`FUN_00054d64` -> read reg 0).
    pub const HARDWARE_VERSION: usize = 0x00;

    /// Command control / trigger register (word offset). CONFIRMED from bmminer:
    /// `FUN_00054568` writes register index `0x1b`, which the index->offset table
    /// maps to word offset `0x30`. Writing [`COMMAND_TRIGGER`]`| (chain << 16)`
    /// ships the buffered command; [`COMMAND_BUSY`] reads set until it drains.
    pub const COMMAND_CONTROL: usize = 0x30;
    /// Command data buffer: three 32-bit words holding the packed VIL frame.
    /// CONFIRMED from bmminer (`FUN_000544ac` writes indices `0x1c/0x1d/0x1e` ->
    /// word offsets `0x31/0x32/0x33`).
    pub const COMMAND_DATA: [usize; 3] = [0x31, 0x32, 0x33];
    /// Bits OR'd into [`COMMAND_CONTROL`] to trigger a send (`0x8080_0000`),
    /// per bmminer `FUN_0005474c`.
    pub const COMMAND_TRIGGER: u32 = 0x8080_0000;
    /// Busy/done bit in [`COMMAND_CONTROL`]: a command is in flight while set.
    pub const COMMAND_BUSY: u32 = 0x8000_0000;

    /// Work TX FIFO ready register: bit `(1 << chain)` set means the chain can
    /// accept work. CONFIRMED from bmminer (`FUN_00053d40` reads reg 3).
    pub const WORK_FIFO_READY: usize = 0x03;
    /// Work TX FIFO: the first word of a work item is written here, the rest to
    /// [`WORK_FIFO_DATA`]. CONFIRMED from bmminer (`FUN_000547c0`).
    pub const WORK_FIFO_FIRST: usize = 0x10;
    /// Work TX FIFO: continuation words after the first. CONFIRMED (`FUN_000547c0`).
    pub const WORK_FIFO_DATA: usize = 0x11;

    /// Nonce RX FIFO data words, read alternately to drain one entry (four
    /// words). CONFIRMED from bmminer (`FUN_00053b5c` reads regs 4 and 5).
    pub const NONCE_FIFO_DATA: [usize; 2] = [0x04, 0x05];
    /// Nonce RX FIFO status: low 15 bits hold a count; the real entry count is
    /// `(status & 0x7fff) >> 1`. CONFIRMED from bmminer (`FUN_00053ad4` reads reg 6).
    pub const NONCE_FIFO_STATUS: usize = 0x06;
    /// Nonce RX control register. CONFIRMED from bmminer (`FUN_0005444c` writes
    /// reg 7); OR in [`NONCE_RX_ENABLE`] to start nonce reception.
    pub const NONCE_CONTROL: usize = 0x07;
    /// Enable bit for nonce reception in [`NONCE_CONTROL`] (`0x1_0000`).
    pub const NONCE_RX_ENABLE: u32 = 0x0001_0000;
    /// Number of 32-bit words the nonce FIFO returns per entry (regs 4,5,4,5).
    pub const NONCE_ENTRY_WORDS: usize = 4;
}

/// VIL serial-wire preamble (`0x55 0xAA`), prepended by the FPGA on the
/// chip-side bus. The CPU does *not* include it in the command buffer it writes
/// to the FPGA (confirmed against stock bmminer), so it is informational here.
pub const TX_PREAMBLE: [u8; 2] = [0x55, 0xAA];
/// Nonce/response frame preamble (`0xAA 0x55`), chain -> host.
pub const RX_PREAMBLE: [u8; 2] = [0xAA, 0x55];

/// VIL command-type / group / opcode bits.
///
/// Public BM13xx VIL set (matches the open bitaxe/cgminer constants). The header
/// byte is `TYPE | GROUP | OPCODE`.
pub mod vil {
    /// Frame carries work/job data.
    pub const TYPE_JOB: u8 = 0x20;
    /// Frame carries a control command.
    pub const TYPE_CMD: u8 = 0x40;
    /// Broadcast to every chip on the chain.
    pub const GROUP_ALL: u8 = 0x10;
    /// Address a single chip (paired with a chip address byte).
    pub const GROUP_SINGLE: u8 = 0x00;

    /// Assign a chip address (single, walks the chain during enumeration).
    pub const OP_SET_ADDRESS: u8 = 0x00;
    /// Write a chip register.
    pub const OP_WRITE_REGISTER: u8 = 0x01;
    /// Read a chip register.
    pub const OP_READ_REGISTER: u8 = 0x02;
    /// Put the chain inactive (stop receiving the address walk).
    pub const OP_CHAIN_INACTIVE: u8 = 0x03;
}

/// Well-known BM1398 chip register addresses.
///
/// `NEEDS-HW-CONFIRM`: addresses follow the public BM13xx reference; confirm the
/// exact map for the BHB428xx hashboard before enabling writes.
pub mod reg {
    /// Chip address / enumeration register (read returns the chip address).
    pub const CHIP_ADDRESS: u8 = 0x00;
    /// PLL0 parameter register (hash clock frequency).
    pub const PLL0_PARAMETER: u8 = 0x08;
    /// Chip nonce offset register.
    pub const CHIP_NONCE_OFFSET: u8 = 0x0C;
    /// Ticket mask (share difficulty selection).
    pub const TICKET_MASK: u8 = 0x14;

    /// Clock-order / UART-relay baud configuration register. CONFIRMED from
    /// bmminer (`FUN_0006ce54` reads/writes chip register `0x28`, packing the
    /// baud divider into bits [15:8]).
    pub const CLOCK_BAUD_CONFIG: u8 = 0x28;
    /// Fast-UART / high-speed PLL register. CONFIRMED from bmminer
    /// (`FUN_0006ce54` writes chip register `0x60` for the fast path).
    pub const FAST_UART_PLL: u8 = 0x60;

    /// Core register control. NEEDS-HW-CONFIRM.
    pub const CORE_REGISTER_CONTROL: u8 = 0x3C;
}

/// Bitmain VIL CRC-5 (poly `x^5 + x^2 + 1` = `0x05`, init `0x1F`, MSB-first).
///
/// VERIFIED against the stock `bmminer` CRC routine (`FUN_0006e918`): same
/// all-ones init, same MSB-first bit walk, same taps, and an empty input yields
/// `0x1F`. Returns the 5-bit checksum in the low bits of the byte.
pub fn crc5(data: &[u8]) -> u8 {
    // `crcin` holds the LFSR state, bit 0 = least significant tap.
    let mut crcin = [1u8; 5];
    let mut byte_idx = 0usize;
    let mut bit_mask = 0x80u8;

    for _ in 0..(data.len() * 8) {
        let din = u8::from(data[byte_idx] & bit_mask != 0);
        let crcout = [
            crcin[4] ^ din,
            crcin[0],
            crcin[1] ^ crcin[4] ^ din,
            crcin[2],
            crcin[3],
        ];

        bit_mask >>= 1;
        if bit_mask == 0 {
            bit_mask = 0x80;
            byte_idx += 1;
        }
        crcin = crcout;
    }

    crcin[0] | (crcin[1] << 1) | (crcin[2] << 2) | (crcin[3] << 3) | (crcin[4] << 4)
}

/// CRC-16/CCITT-FALSE (poly `0x1021`, init `0xFFFF`), used to protect work frames.
pub fn crc16_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// A single VIL command (control plane), independent of any transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VilCommand {
    /// Assign `chip_address` to the next un-addressed chip in the walk.
    SetChipAddress { chip_address: u8 },
    /// Stop the address walk on the whole chain.
    ChainInactive,
    /// Write `value` to `register`, on one chip or all chips.
    WriteRegister {
        all: bool,
        chip_address: u8,
        register: u8,
        value: u32,
    },
    /// Request a read of `register`, from one chip or all chips.
    ReadRegister {
        all: bool,
        chip_address: u8,
        register: u8,
    },
}

impl VilCommand {
    /// Serialize the command into the bytes the CPU writes to the FPGA command
    /// buffer: `header | length | body... | crc5`.
    ///
    /// There is deliberately **no** `0x55 0xAA` preamble here: the stock bmminer
    /// builds exactly this frame (`FUN_0006e69c` -> a 9-byte register write) and
    /// the FPGA prepends the `0x55 0xAA` preamble on the chip-side serial wire.
    /// `length` counts the whole command including header, length and crc.
    pub fn to_frame(&self) -> Vec<u8> {
        let (header, body): (u8, Vec<u8>) = match *self {
            VilCommand::SetChipAddress { chip_address } => (
                vil::TYPE_CMD | vil::GROUP_SINGLE | vil::OP_SET_ADDRESS,
                vec![chip_address, 0x00],
            ),
            VilCommand::ChainInactive => (
                vil::TYPE_CMD | vil::GROUP_ALL | vil::OP_CHAIN_INACTIVE,
                vec![0x00, 0x00],
            ),
            VilCommand::WriteRegister {
                all,
                chip_address,
                register,
                value,
            } => {
                let group = if all { vil::GROUP_ALL } else { vil::GROUP_SINGLE };
                let mut body = Vec::with_capacity(6);
                body.push(chip_address);
                body.push(register);
                body.extend_from_slice(&value.to_be_bytes());
                (vil::TYPE_CMD | group | vil::OP_WRITE_REGISTER, body)
            }
            VilCommand::ReadRegister {
                all,
                chip_address,
                register,
            } => {
                let group = if all { vil::GROUP_ALL } else { vil::GROUP_SINGLE };
                (
                    vil::TYPE_CMD | group | vil::OP_READ_REGISTER,
                    vec![chip_address, register],
                )
            }
        };

        // Length counts the whole command: header + length + body + crc5. For a
        // register write body = [chip_addr, reg, value(4)] so length = 9, which
        // matches the stock bmminer builder (`FUN_0006e69c` writes `buf[1]=9`).
        let length = (body.len() + 3) as u8;
        let mut frame = Vec::with_capacity(body.len() + 3);
        frame.push(header);
        frame.push(length);
        frame.extend_from_slice(&body);
        // CRC5 covers header + length + body (every byte but the crc). Verified
        // against stock bmminer `FUN_0006e918(buf, bit_len)`.
        let crc = crc5(&frame);
        frame.push(crc);
        frame
    }
}

/// Resolved BM1398 PLL divider set for a target frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bm1398Pll {
    pub refdiv: u32,
    pub fbdiv: u32,
    pub postdiv1: u32,
    pub postdiv2: u32,
    /// Frequency the dividers actually realize, in MHz.
    pub realized_mhz: u32,
}

impl Bm1398Pll {
    /// VCO frequency (`25 MHz * fbdiv / refdiv`) the dividers imply, in MHz.
    pub fn vco_mhz(&self) -> u32 {
        CRYSTAL_MHZ * self.fbdiv / self.refdiv
    }

    /// Pack the dividers into the PLL0 parameter register word.
    ///
    /// Layout matches the public BM13xx PLL0 encoding
    /// (`fbdiv[15:8] | refdiv[13:8]... | postdiv1[6:4] | postdiv2[2:0]`).
    /// `NEEDS-HW-CONFIRM` for the BHB428xx board.
    pub fn to_register(&self) -> u32 {
        ((self.fbdiv & 0xFFF) << 16)
            | ((self.refdiv & 0x3F) << 8)
            | ((self.postdiv1 & 0x7) << 4)
            | (self.postdiv2 & 0x7)
    }
}

/// Solve for the BM1398 PLL dividers closest to `target_mhz`.
///
/// Mirrors the stock `bmminer` search: keep the VCO inside `[2000, 3200] MHz`,
/// prefer `postdiv1 >= postdiv2`, and minimize the absolute frequency error.
/// Returns `None` if `target_mhz` is zero.
pub fn solve_pll(target_mhz: u16) -> Option<Bm1398Pll> {
    if target_mhz == 0 {
        return None;
    }
    let target = u32::from(target_mhz);

    const VCO_MIN: u32 = 2000;
    const VCO_MAX: u32 = 3200;

    let mut best: Option<Bm1398Pll> = None;
    let mut best_error = u32::MAX;

    for refdiv in 1..=2u32 {
        for postdiv1 in 1..=7u32 {
            for postdiv2 in 1..=postdiv1 {
                let divisor = refdiv * postdiv1 * postdiv2;
                // Ideal fbdiv (rounded) for this divisor chain.
                let fbdiv = ((target * divisor) + (CRYSTAL_MHZ / 2)) / CRYSTAL_MHZ;
                if fbdiv == 0 {
                    continue;
                }
                let vco = CRYSTAL_MHZ * fbdiv / refdiv;
                if !(VCO_MIN..=VCO_MAX).contains(&vco) {
                    continue;
                }
                let realized = CRYSTAL_MHZ * fbdiv / divisor;
                let error = realized.abs_diff(target);
                if error < best_error {
                    best_error = error;
                    best = Some(Bm1398Pll {
                        refdiv,
                        fbdiv,
                        postdiv1,
                        postdiv2,
                        realized_mhz: realized,
                    });
                    if error == 0 {
                        return best;
                    }
                }
            }
        }
    }

    best
}

/// Build the address-walk command stream that enumerates a chain of
/// `chip_count` chips: a broadcast `ChainInactive`, then one `SetChipAddress`
/// per chip at the vendor stride of `interval`.
pub fn enumerate_chain(chip_count: u16, interval: u8) -> Vec<Vec<u8>> {
    let mut frames = Vec::with_capacity(usize::from(chip_count) + 1);
    frames.push(VilCommand::ChainInactive.to_frame());
    for index in 0..chip_count {
        let chip_address = (u16::from(interval) * index) as u8;
        frames.push(VilCommand::SetChipAddress { chip_address }.to_frame());
    }
    frames
}

/// A unit of work handed to the BM1398 chain, with precomputed midstates.
///
/// The SHA-256 first-block midstate(s) are computed upstream (one for a plain
/// job, up to four when AsicBoost version rolling is on). This struct only
/// serializes them into the VIL job wire layout; it does not hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// Work id echoed back with any nonce the chain finds.
    pub work_id: u8,
    /// Nonce the chain starts iterating from (little-endian on the wire).
    pub starting_nonce: u32,
    /// Compact difficulty target (`nbits`), big-endian as in the block header.
    pub nbits: u32,
    /// Block time (`ntime`), big-endian as in the block header.
    pub ntime: u32,
    /// Final 4 bytes of the merkle root that follow the midstate boundary.
    pub merkle_root_tail: [u8; 4],
    /// One to four 32-byte SHA-256 midstates (more than one enables AsicBoost).
    pub midstates: Vec<[u8; 32]>,
}

/// Error returned when a [`WorkItem`] cannot be serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkError {
    /// `midstates` was empty or held more than four entries.
    InvalidMidstateCount(usize),
}

impl WorkItem {
    /// Serialize into a VIL job frame: `0x55 0xAA | header | length | work_id |
    /// num_midstates | starting_nonce[4] LE | nbits[4] | ntime[4] |
    /// merkle_tail[4] | midstate[32]... | crc16[2]`.
    ///
    /// `NEEDS-HW-CONFIRM`: field endianness and whether the FPGA expects the
    /// CRC16 in big- or little-endian byte order on BHB428xx.
    pub fn to_frame(&self) -> Result<Vec<u8>, WorkError> {
        let count = self.midstates.len();
        if count == 0 || count > 4 {
            return Err(WorkError::InvalidMidstateCount(count));
        }

        let header = vil::TYPE_JOB | vil::GROUP_ALL;
        // After the preamble: header(1) + length(1) + work_id(1) + num(1)
        // + starting_nonce(4) + nbits(4) + ntime(4) + merkle_tail(4)
        // + midstate(32*count) + crc16(2) = 22 + 32*count.
        let length = (22 + 32 * count) as u8;

        let mut body = Vec::with_capacity(usize::from(length));
        body.push(header);
        body.push(length);
        body.push(self.work_id);
        body.push(count as u8);
        body.extend_from_slice(&self.starting_nonce.to_le_bytes());
        body.extend_from_slice(&self.nbits.to_be_bytes());
        body.extend_from_slice(&self.ntime.to_be_bytes());
        body.extend_from_slice(&self.merkle_root_tail);
        for midstate in &self.midstates {
            body.extend_from_slice(midstate);
        }
        let crc = crc16_false(&body);
        body.extend_from_slice(&crc.to_be_bytes());

        let mut frame = Vec::with_capacity(body.len() + 2);
        frame.extend_from_slice(&TX_PREAMBLE);
        frame.extend_from_slice(&body);
        Ok(frame)
    }
}

/// One entry drained from the BM1398 nonce RX FIFO.
///
/// Layout CONFIRMED from the stock bmminer nonce/register processor
/// (`FUN_00034810`). Each entry arrives as two 32-bit FPGA words: a metadata
/// word and the nonce word. The metadata bytes are (little-endian within the
/// word) `byte0..byte3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonceEntry {
    /// The 32-bit nonce (FIFO data word 1).
    pub nonce: u32,
    /// CRC error flag: metadata `byte0 & 0x40` (bmminer logs `reg crc error`).
    pub crc_error: bool,
    /// True when the entry is a register-read response, not a nonce: metadata
    /// `byte3 & 0x60` (bits [6:5]) is non-zero.
    pub register_response: bool,
    /// Work/job id the nonce answers (metadata `byte1`).
    pub work_id: u8,
    /// Chip/core selector from metadata `byte2` and `byte3 & 0x1f`. The exact
    /// chip-vs-core split is NEEDS-HW-CONFIRM.
    pub chip_core: u16,
}

/// Decode one nonce FIFO entry from its metadata word and nonce word, mirroring
/// the field extraction in stock bmminer `FUN_00034810`.
pub fn decode_nonce_entry(meta: u32, nonce: u32) -> NonceEntry {
    let byte0 = (meta & 0xff) as u8;
    let byte1 = ((meta >> 8) & 0xff) as u8;
    let byte2 = ((meta >> 16) & 0xff) as u8;
    let byte3 = ((meta >> 24) & 0xff) as u8;
    NonceEntry {
        nonce,
        crc_error: byte0 & 0x40 != 0,
        register_response: byte3 & 0x60 != 0,
        work_id: byte1,
        chip_core: (u16::from(byte2) << 5) | u16::from(byte3 & 0x1f),
    }
}

/// Decode the primary nonce entry from a four-word FIFO block (`regs 4,5,4,5`).
///
/// Words `[0]`/`[1]` are the metadata/nonce pair; words `[2]`/`[3]` may carry a
/// second response and are not decoded here (NEEDS-HW-CONFIRM).
pub fn decode_nonce_block(words: &[u32; 4]) -> NonceEntry {
    decode_nonce_entry(words[0], words[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc5_is_deterministic_and_five_bits() {
        let frame = [0x41u8, 0x05, 0x00, 0x00];
        let crc = crc5(&frame);
        assert_eq!(crc, crc5(&frame));
        assert_eq!(crc & 0xE0, 0, "crc5 must fit in five bits");
    }

    #[test]
    fn crc5_detects_every_single_bit_flip() {
        let base = [vil::TYPE_CMD | vil::OP_WRITE_REGISTER, 0x09, 0x00, reg::PLL0_PARAMETER];
        let good = crc5(&base);
        for byte in 0..base.len() {
            for bit in 0..8 {
                let mut corrupt = base;
                corrupt[byte] ^= 1 << bit;
                assert_ne!(crc5(&corrupt), good, "flip at byte {byte} bit {bit} not caught");
            }
        }
    }

    #[test]
    fn crc16_false_matches_known_check_vector() {
        // CRC-16/CCITT-FALSE of b"123456789" is 0x29B1 (canonical check value).
        assert_eq!(crc16_false(b"123456789"), 0x29B1);
    }

    #[test]
    fn set_chip_address_frame_is_well_formed() {
        let frame = VilCommand::SetChipAddress { chip_address: 0x04 }.to_frame();
        // No 0x55 0xAA preamble: the FPGA adds it on the wire.
        assert_eq!(frame[0], vil::TYPE_CMD | vil::OP_SET_ADDRESS); // 0x40
        assert_eq!(frame[1], 0x05); // length
        assert_eq!(frame[2], 0x04); // chip address
        assert_eq!(frame.len(), 5);
        // CRC covers every byte but the trailing crc.
        assert_eq!(*frame.last().unwrap(), crc5(&frame[..frame.len() - 1]));
    }

    #[test]
    fn write_register_all_sets_broadcast_bit_and_big_endian_value() {
        let frame = VilCommand::WriteRegister {
            all: true,
            chip_address: 0x00,
            register: reg::PLL0_PARAMETER,
            value: 0x0102_0304,
        }
        .to_frame();
        // Matches stock bmminer FUN_0006e69c: header 0x51, len 9, BE value, crc5.
        assert_eq!(frame[0], vil::TYPE_CMD | vil::GROUP_ALL | vil::OP_WRITE_REGISTER); // 0x51
        assert_eq!(frame[1], 0x09); // length: header+len+addr+reg+4 data+crc
        assert_eq!(frame[3], reg::PLL0_PARAMETER);
        assert_eq!(&frame[4..8], &[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(frame.len(), 9);
        assert_eq!(*frame.last().unwrap(), crc5(&frame[..frame.len() - 1]));
    }

    #[test]
    fn chain_inactive_is_broadcast() {
        let frame = VilCommand::ChainInactive.to_frame();
        assert_eq!(frame[0], vil::TYPE_CMD | vil::GROUP_ALL | vil::OP_CHAIN_INACTIVE); // 0x53
        assert_eq!(frame[1], 0x05);
    }

    #[test]
    fn solve_pll_hits_factory_default_exactly() {
        let pll = solve_pll(DEFAULT_FREQUENCY_MHZ).expect("400 MHz solvable");
        assert_eq!(pll.realized_mhz, 400);
        assert_eq!(CRYSTAL_MHZ * pll.fbdiv / (pll.refdiv * pll.postdiv1 * pll.postdiv2), 400);
        assert!((VCO_OK).contains(&pll.vco_mhz()));
    }

    const VCO_OK: std::ops::RangeInclusive<u32> = 2000..=3200;

    #[test]
    fn solve_pll_keeps_vco_in_range_across_the_operating_band() {
        for target in (250..=900).step_by(25) {
            let pll = solve_pll(target).expect("solvable in band");
            assert!(
                VCO_OK.contains(&pll.vco_mhz()),
                "target {target} produced out-of-range VCO {}",
                pll.vco_mhz()
            );
            // Closest achievable point should be within one crystal step.
            assert!(pll.realized_mhz.abs_diff(u32::from(target)) <= CRYSTAL_MHZ);
        }
    }

    #[test]
    fn solve_pll_rejects_zero() {
        assert!(solve_pll(0).is_none());
    }

    #[test]
    fn enumerate_chain_walks_every_chip_after_inactive() {
        let frames = enumerate_chain(4, 2);
        assert_eq!(frames.len(), 5);
        // First frame is the broadcast chain-inactive (header at index 0).
        assert_eq!(frames[0][0], vil::TYPE_CMD | vil::GROUP_ALL | vil::OP_CHAIN_INACTIVE);
        // Subsequent frames assign addresses 0, 2, 4, 6.
        for (index, expected) in [0u8, 2, 4, 6].into_iter().enumerate() {
            assert_eq!(frames[index + 1][0], vil::TYPE_CMD | vil::OP_SET_ADDRESS);
            assert_eq!(frames[index + 1][2], expected);
        }
    }

    #[test]
    fn decode_nonce_entry_extracts_confirmed_fields() {
        // meta bytes (LE within word): byte0=00, byte1=2A, byte2=12, byte3=05.
        let decoded = decode_nonce_entry(0x0512_2A00, 0x1234_5607);
        assert_eq!(decoded.nonce, 0x1234_5607);
        assert!(!decoded.crc_error);
        assert!(!decoded.register_response);
        assert_eq!(decoded.work_id, 0x2A);
        assert_eq!(decoded.chip_core, (0x12 << 5) | 0x05);
    }

    #[test]
    fn decode_nonce_entry_flags_crc_error_and_register_response() {
        // byte0 bit 0x40 -> crc error; byte3 bits[6:5] -> register response.
        let decoded = decode_nonce_entry(0x2000_0040, 0);
        assert!(decoded.crc_error);
        assert!(decoded.register_response);
    }

    #[test]
    fn decode_nonce_block_uses_first_word_pair() {
        let block = [0x0512_2A00, 0x1234_5607, 0xDEAD_BEEF, 0xCAFE_BABE];
        assert_eq!(
            decode_nonce_block(&block),
            decode_nonce_entry(0x0512_2A00, 0x1234_5607)
        );
    }

    fn sample_work(midstates: usize) -> WorkItem {
        WorkItem {
            work_id: 0x2A,
            starting_nonce: 0x0000_0000,
            nbits: 0x1700_7fff,
            ntime: 0x6500_0000,
            merkle_root_tail: [0xde, 0xad, 0xbe, 0xef],
            midstates: vec![[0x11; 32]; midstates],
        }
    }

    #[test]
    fn work_frame_is_job_typed_and_crc_protected() {
        let frame = sample_work(1).to_frame().expect("one midstate");
        assert_eq!(&frame[0..2], &TX_PREAMBLE);
        assert_eq!(frame[2], vil::TYPE_JOB | vil::GROUP_ALL);
        assert_eq!(frame[4], 0x2A); // work_id
        assert_eq!(frame[5], 1); // num_midstates

        // The trailing CRC16 must validate over the body (everything after the
        // preamble except the two CRC bytes).
        let body = &frame[2..frame.len() - 2];
        let crc = u16::from_be_bytes([frame[frame.len() - 2], frame[frame.len() - 1]]);
        assert_eq!(crc16_false(body), crc);
    }

    #[test]
    fn work_frame_length_grows_with_midstate_count() {
        let one = sample_work(1).to_frame().unwrap();
        let four = sample_work(4).to_frame().unwrap();
        // Each extra midstate adds 32 bytes.
        assert_eq!(four.len() - one.len(), 3 * 32);
        // Declared length byte covers everything after the preamble.
        assert_eq!(usize::from(one[3]), one.len() - 2);
    }

    #[test]
    fn work_frame_rejects_empty_and_oversized_midstates() {
        assert_eq!(
            sample_work(0).to_frame(),
            Err(WorkError::InvalidMidstateCount(0))
        );
        assert_eq!(
            sample_work(5).to_frame(),
            Err(WorkError::InvalidMidstateCount(5))
        );
    }

    #[test]
    fn work_frame_starting_nonce_is_little_endian() {
        let mut work = sample_work(1);
        work.starting_nonce = 0x0102_0304;
        let frame = work.to_frame().unwrap();
        // work_id at [4], num_midstates at [5], starting_nonce at [6..10] LE.
        assert_eq!(&frame[6..10], &[0x04, 0x03, 0x02, 0x01]);
    }
}
