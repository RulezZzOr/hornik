//! BM1398 chain driver: protocol ([`crate::bm1398`]) over transport ([`crate::axi`]).
//!
//! This ties the VIL codec to the AXI FPGA register window into one object that
//! can enumerate a chain, set frequency, push work and drain nonces. It is the
//! shape a real S19 XIL bring-up driver takes, but it stays a *mechanism*: the
//! only hardware-touching entry point is [`Bm1398Driver::open_device`], and the
//! caller must not construct that until the hardware safety gate opens. Every
//! method is exercised on the host through [`Bm1398Driver::open_for_test`].

use crate::axi::AxiFpga;
use crate::bm1398::{self, Bm1398Pll, VilCommand};
use std::io;
use std::path::Path;

/// Default address stride used while walking the chain during enumeration.
///
/// `NEEDS-HW-CONFIRM`: BM1398 chains are typically addressed with a fixed stride
/// so that `address = stride * index`; confirm the BHB428xx value on the board.
pub const DEFAULT_ADDRESS_INTERVAL: u8 = 4;

/// A BM1398 hashboard chain reached over the AXI FPGA bridge.
#[derive(Debug)]
pub struct Bm1398Driver {
    fpga: AxiFpga,
    /// Number of chips the last enumeration assigned an address to.
    enumerated_chips: u16,
}

impl Bm1398Driver {
    /// Open the real FPGA register device (`/dev/axi_fpga_dev` on a board).
    ///
    /// Hardware-touching: gate this behind the safety gate.
    pub fn open_device(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self::with_fpga(AxiFpga::map_device(path)?))
    }

    /// Open a host-backed driver (anonymous mapping) for tests and dry runs.
    pub fn open_for_test() -> io::Result<Self> {
        Ok(Self::with_fpga(AxiFpga::map_anonymous()?))
    }

    fn with_fpga(fpga: AxiFpga) -> Self {
        Self {
            fpga,
            enumerated_chips: 0,
        }
    }

    /// Read the FPGA hardware/version word.
    pub fn fpga_version(&self) -> u32 {
        self.fpga.fpga_version()
    }

    /// Number of chips assigned an address by the most recent [`Self::enumerate`].
    pub fn enumerated_chips(&self) -> u16 {
        self.enumerated_chips
    }

    /// Walk `chain`: broadcast `ChainInactive`, then assign `chip_count`
    /// sequential addresses at [`DEFAULT_ADDRESS_INTERVAL`]. Returns the number
    /// of address-assignment frames issued.
    pub fn enumerate(&mut self, chain: u8, chip_count: u16) -> u16 {
        let frames = bm1398::enumerate_chain(chip_count, DEFAULT_ADDRESS_INTERVAL);
        for frame in &frames {
            self.fpga.write_command_frame(chain, frame);
        }
        self.enumerated_chips = chip_count;
        chip_count
    }

    /// Resolve the PLL dividers for `frequency_mhz` without touching hardware.
    pub fn plan_frequency(&self, frequency_mhz: u16) -> Option<Bm1398Pll> {
        bm1398::solve_pll(frequency_mhz)
    }

    /// Broadcast a PLL frequency change to every chip on `chain`.
    ///
    /// Returns the realized PLL, or `None` if `frequency_mhz` is unsolvable.
    pub fn set_frequency_all(&mut self, chain: u8, frequency_mhz: u16) -> Option<Bm1398Pll> {
        let pll = bm1398::solve_pll(frequency_mhz)?;
        let frame = VilCommand::WriteRegister {
            all: true,
            chip_address: 0,
            register: bm1398::reg::PLL0_PARAMETER,
            value: pll.to_register(),
        }
        .to_frame();
        self.fpga.write_command_frame(chain, &frame);
        Some(pll)
    }

    /// Push raw, pre-serialized work bytes into the FPGA work FIFO.
    pub fn submit_work(&mut self, work: &[u8]) {
        self.fpga.push_work(work);
    }

    /// Serialize a [`WorkItem`] and push it into the FPGA work FIFO.
    pub fn submit_work_item(&mut self, work: &bm1398::WorkItem) -> Result<(), bm1398::WorkError> {
        let frame = work.to_frame()?;
        self.fpga.push_work(&frame);
        Ok(())
    }

    /// Enable nonce reception on the FPGA, as bmminer's reader does at startup.
    pub fn enable_nonce_rx(&mut self) {
        self.fpga.enable_nonce_rx();
    }

    /// Drain up to `max_entries` nonce entries from the RX FIFO. Each entry is
    /// the raw four-word block the FPGA returns (regs 4,5,4,5); decoding that
    /// block into a [`bm1398::NonceReturn`] is deferred until the nonce wire
    /// layout is confirmed on hardware.
    pub fn poll_nonce_entries(&mut self, max_entries: usize) -> Vec<[u32; 4]> {
        let pending = self.fpga.pending_nonce_entries() as usize;
        let to_read = pending.min(max_entries);
        let mut entries = Vec::with_capacity(to_read);
        for _ in 0..to_read {
            let Some(entry) = self.fpga.read_nonce_entry() else {
                break;
            };
            entries.push(entry);
        }
        entries
    }

    /// Drain and decode up to `max_entries` nonce entries into
    /// [`bm1398::NonceEntry`]s, using the confirmed bmminer field layout.
    pub fn poll_nonces(&mut self, max_entries: usize) -> Vec<bm1398::NonceEntry> {
        self.poll_nonce_entries(max_entries)
            .iter()
            .map(bm1398::decode_nonce_block)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bm1398::fpga;

    #[test]
    fn enumerate_records_chip_count() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        assert_eq!(driver.enumerated_chips(), 0);
        let count = driver.enumerate(0, 76);
        assert_eq!(count, 76);
        assert_eq!(driver.enumerated_chips(), 76);
    }

    #[test]
    fn set_frequency_all_returns_realized_pll() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        let pll = driver.set_frequency_all(0, 525).expect("525 MHz solvable");
        assert_eq!(pll.realized_mhz, 525);
    }

    #[test]
    fn set_frequency_all_rejects_zero() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        assert!(driver.set_frequency_all(0, 0).is_none());
    }

    #[test]
    fn plan_frequency_does_not_require_mut() {
        let driver = Bm1398Driver::open_for_test().expect("test driver");
        assert_eq!(driver.plan_frequency(400).unwrap().realized_mhz, 400);
    }

    #[test]
    fn poll_nonce_entries_drains_pending_fifo() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        assert!(driver.poll_nonce_entries(8).is_empty());

        // Status word count = 2 -> one entry; data regs 4 and 5 feed the block.
        driver.fpga.write_word(fpga::NONCE_FIFO_STATUS, 2);
        driver.fpga.write_word(fpga::NONCE_FIFO_DATA[0], 0x0102_0304);
        driver.fpga.write_word(fpga::NONCE_FIFO_DATA[1], 0x0506_0708);
        let entries = driver.poll_nonce_entries(8);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], [0x0102_0304, 0x0506_0708, 0x0102_0304, 0x0506_0708]);
    }

    #[test]
    fn submit_work_item_serializes_and_pushes() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        let work = bm1398::WorkItem {
            work_id: 1,
            starting_nonce: 0,
            nbits: 0x1700_7fff,
            ntime: 0x6500_0000,
            merkle_root_tail: [0; 4],
            midstates: vec![[0u8; 32]; bm1398::WORK_MIDSTATES],
        };
        assert!(driver.submit_work_item(&work).is_ok());
    }

    #[test]
    fn submit_work_item_rejects_invalid_midstates() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        let work = bm1398::WorkItem {
            work_id: 1,
            starting_nonce: 0,
            nbits: 0,
            ntime: 0,
            merkle_root_tail: [0; 4],
            midstates: vec![],
        };
        assert_eq!(
            driver.submit_work_item(&work),
            Err(bm1398::WorkError::InvalidMidstateCount(0))
        );
    }

    #[test]
    fn poll_nonces_decodes_entries() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        driver.fpga.write_word(fpga::NONCE_FIFO_STATUS, 2);
        driver.fpga.write_word(fpga::NONCE_FIFO_DATA[0], 0x0512_2A00); // meta
        driver.fpga.write_word(fpga::NONCE_FIFO_DATA[1], 0x1234_5607); // nonce
        let nonces = driver.poll_nonces(8);
        assert_eq!(nonces.len(), 1);
        assert_eq!(nonces[0].nonce, 0x1234_5607);
        assert_eq!(nonces[0].work_id, 0x2A);
        assert!(!nonces[0].crc_error);
    }

    #[test]
    fn poll_nonce_entries_caps_at_max() {
        let mut driver = Bm1398Driver::open_for_test().expect("test driver");
        // Word count 20 -> 10 entries available; cap the read at 3.
        driver.fpga.write_word(fpga::NONCE_FIFO_STATUS, 20);
        driver.fpga.write_word(fpga::NONCE_FIFO_DATA[0], 0xAABB_CCDD);
        assert_eq!(driver.poll_nonce_entries(3).len(), 3);
    }
}
