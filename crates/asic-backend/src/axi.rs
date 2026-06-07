//! Memory-mapped AXI FPGA transport for the Antminer S19 XIL control board.
//!
//! # Why this exists
//!
//! On S19 XIL (Zynq-7007S) the CPU reaches the BM1398 hashboard chain through an
//! FPGA in the Zynq PL. The stock `bitmain_axi.ko` exposes that FPGA register
//! window as a character device, `/dev/axi_fpga_dev`, whose only meaningful file
//! op is `mmap`. The driver does **not** implement `read`/`write` fops, so the
//! register window must be reached through a shared mmap — a plain
//! `read_at`/`write_at` against the device would fail. This module wraps that
//! mmap as a small, volatile, word-addressed register file and layers the
//! command / work / nonce helpers on top of it.
//!
//! # Testability
//!
//! [`AxiFpga::map_anonymous`] backs the same register API with an anonymous
//! shared mapping, so every method here is exercised on the host without a
//! board. On a real miner [`AxiFpga::map_device`] maps `/dev/axi_fpga_dev`.
//!
//! # Safety posture
//!
//! Mapping the real device and writing its registers is a hardware write. This
//! module is a transport mechanism only; nothing here is invoked on the live
//! mining path until the hardware safety gate opens it. The window base
//! (`0x4000_0000`) and size (`0x1400`) are CONFIRMED from the stock
//! `bitmain_axi.ko` (`ioremap(0x4000_0000, 0x1400)`); the individual register
//! offsets *within* that window and the command/FIFO handshake still follow the
//! public Bitmain Zynq reference and remain `NEEDS-HW-CONFIRM` until validated
//! against the stock `bmminer` or on the board.

use crate::bm1398::fpga;
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::ptr::NonNull;

/// Physical base address of the FPGA register window on the Zynq AXI GP0 bus.
///
/// CONFIRMED from the stock `bitmain_axi.ko`, whose init does
/// `ioremap(0x4000_0000, 0x1400)`. Userspace reaches the same window by mmap'ing
/// `/dev/axi_fpga_dev` at offset 0, so this constant is informational for the
/// device path but pins the hardware target.
pub const AXI_FPGA_PHYS_BASE: u64 = 0x4000_0000;

/// Size of the mapped FPGA register window, in bytes.
///
/// CONFIRMED from `bitmain_axi.ko` (`ioremap(_, 0x1400)`): the FPGA exposes a
/// 5120-byte (0x1400) register window.
pub const WINDOW_BYTES: usize = 0x1400;

/// Size of the mapped FPGA register window, in 32-bit words (`0x1400 / 4`).
pub const WINDOW_WORDS: usize = WINDOW_BYTES / 4;

/// A volatile, word-addressed view of the AXI FPGA register window.
#[derive(Debug)]
pub struct AxiFpga {
    base: NonNull<u32>,
    words: usize,
    /// Length of the mmap in bytes, retained for `munmap`.
    map_len: usize,
    /// When true the mapping is `PROT_READ` only; writes would fault, so they
    /// are rejected before they reach the page.
    readonly: bool,
    /// Kept open for the lifetime of the mapping when backed by a device/file.
    _file: Option<std::fs::File>,
}

/// Read-only snapshot of the FPGA identity/status registers, for safe on-board
/// validation of the register map without issuing any writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpgaProbe {
    pub fpga_version: u32,
    pub nonce_fifo_status: u32,
    pub pending_nonce_entries: u32,
    pub work_fifo_ready: u32,
    pub command_busy: bool,
}

/// Open the FPGA register device read-only and sample its identity/status
/// registers. Writes are impossible on this mapping, so it is safe to run
/// against a board that is currently mining under another firmware.
pub fn probe_readonly(path: impl AsRef<Path>) -> io::Result<FpgaProbe> {
    let fpga = AxiFpga::map_device_readonly(path)?;
    Ok(FpgaProbe {
        fpga_version: fpga.fpga_version(),
        nonce_fifo_status: fpga.read_word(fpga::NONCE_FIFO_STATUS),
        pending_nonce_entries: fpga.pending_nonce_entries(),
        work_fifo_ready: fpga.read_word(fpga::WORK_FIFO_READY),
        command_busy: fpga.command_busy(),
    })
}

// SAFETY: `AxiFpga` owns its mapping exclusively and exposes only `&mut self`
// register writes, so moving it across threads is sound; concurrent access must
// still be serialized by the caller (e.g. behind a `Mutex`), exactly as the
// existing UART transport is held.
unsafe impl Send for AxiFpga {}

impl AxiFpga {
    /// Map the real FPGA register device (`/dev/axi_fpga_dev` on a board).
    pub fn map_device(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        let map_len = WINDOW_WORDS * std::mem::size_of::<u32>();
        // SAFETY: `file` is a valid RW fd; we map exactly `map_len` bytes shared.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        Self::from_mapping(ptr, map_len, false, Some(file))
    }

    /// Map the FPGA register device read-only (`PROT_READ`), for safe on-board
    /// validation. Any attempt to write through the returned handle is rejected.
    pub fn map_device_readonly(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = std::fs::OpenOptions::new().read(true).open(path)?;
        let map_len = WINDOW_WORDS * std::mem::size_of::<u32>();
        // SAFETY: `file` is a valid readable fd; we map `map_len` bytes read-only.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        Self::from_mapping(ptr, map_len, true, Some(file))
    }

    /// Back the register API with an anonymous shared mapping (host tests).
    pub fn map_anonymous() -> io::Result<Self> {
        let map_len = WINDOW_WORDS * std::mem::size_of::<u32>();
        // SAFETY: anonymous mapping, no fd; kernel zero-fills the region.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANON,
                -1,
                0,
            )
        };
        Self::from_mapping(ptr, map_len, false, None)
    }

    fn from_mapping(
        ptr: *mut libc::c_void,
        map_len: usize,
        readonly: bool,
        file: Option<std::fs::File>,
    ) -> io::Result<Self> {
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        // `mmap` returns page-aligned memory, so the `u32` cast is well aligned.
        let base = NonNull::new(ptr.cast::<u32>())
            .ok_or_else(|| io::Error::other("mmap returned a null pointer"))?;
        Ok(Self {
            base,
            words: map_len / std::mem::size_of::<u32>(),
            map_len,
            readonly,
            _file: file,
        })
    }

    /// Whether this mapping is read-only.
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    /// Read register word at `index` (word offset into the window).
    pub fn read_word(&self, index: usize) -> u32 {
        assert!(index < self.words, "register index {index} out of range");
        // SAFETY: bounds checked above; the mapping covers `self.words` words.
        unsafe { self.base.as_ptr().add(index).read_volatile() }
    }

    /// Write `value` to the register word at `index`.
    pub fn write_word(&mut self, index: usize, value: u32) {
        assert!(!self.readonly, "refusing to write to a read-only FPGA mapping");
        assert!(index < self.words, "register index {index} out of range");
        // SAFETY: bounds checked above; mapping is writable and word aligned.
        unsafe { self.base.as_ptr().add(index).write_volatile(value) }
    }

    /// Read the FPGA hardware/version word (`bmminer` logs `FPGA Version`).
    pub fn fpga_version(&self) -> u32 {
        self.read_word(fpga::HARDWARE_VERSION)
    }

    /// Ship a VIL command frame to `chain` through the FPGA command path.
    ///
    /// CONFIRMED handshake from stock bmminer (`FUN_000544ac` + `FUN_0005474c`):
    /// pack the frame big-endian into the three [`fpga::COMMAND_DATA`] words,
    /// then write [`fpga::COMMAND_CONTROL`] = [`fpga::COMMAND_TRIGGER`]
    /// `| (chain << 16)` to launch it. A VIL command is at most 9 bytes, so it
    /// always fits in the three command-data words.
    pub fn write_command_frame(&mut self, chain: u8, frame: &[u8]) {
        let words = pack_be_words(frame);
        for (slot, &offset) in fpga::COMMAND_DATA.iter().enumerate() {
            self.write_word(offset, words.get(slot).copied().unwrap_or(0));
        }
        self.write_word(
            fpga::COMMAND_CONTROL,
            fpga::COMMAND_TRIGGER | (u32::from(chain) << 16),
        );
    }

    /// Whether the FPGA still has a command in flight (busy bit set).
    pub fn command_busy(&self) -> bool {
        self.read_word(fpga::COMMAND_CONTROL) & fpga::COMMAND_BUSY != 0
    }

    /// Whether `chain` can currently accept work (its ready bit is set in
    /// [`fpga::WORK_FIFO_READY`]).
    pub fn work_fifo_ready(&self, chain: u8) -> bool {
        self.read_word(fpga::WORK_FIFO_READY) & (1u32 << u32::from(chain)) != 0
    }

    /// Push a serialized work item into the work TX FIFO.
    ///
    /// CONFIRMED layout (bmminer `FUN_000547c0`): the first 32-bit word goes to
    /// [`fpga::WORK_FIFO_FIRST`], every following word to [`fpga::WORK_FIFO_DATA`].
    pub fn push_work(&mut self, work: &[u8]) {
        for (index, word) in pack_be_words(work).into_iter().enumerate() {
            let offset = if index == 0 {
                fpga::WORK_FIFO_FIRST
            } else {
                fpga::WORK_FIFO_DATA
            };
            self.write_word(offset, word);
        }
    }

    /// Enable nonce reception (sets [`fpga::NONCE_RX_ENABLE`] in the nonce
    /// control register, as bmminer's reader does at startup).
    pub fn enable_nonce_rx(&mut self) {
        let current = self.read_word(fpga::NONCE_CONTROL);
        self.write_word(fpga::NONCE_CONTROL, current | fpga::NONCE_RX_ENABLE);
    }

    /// Number of nonce entries currently waiting in the RX FIFO.
    ///
    /// CONFIRMED (bmminer `FUN_00053ad4`): the status register's low 15 bits are
    /// a word count, and the entry count is `(status & 0x7fff) >> 1`.
    pub fn pending_nonce_entries(&self) -> u32 {
        (self.read_word(fpga::NONCE_FIFO_STATUS) & 0x7fff) >> 1
    }

    /// Drain one nonce entry (four words) from the RX FIFO, reading the two data
    /// registers alternately as bmminer's `FUN_00053b5c` does. Returns `None`
    /// when the FIFO is empty.
    pub fn read_nonce_entry(&mut self) -> Option<[u32; fpga::NONCE_ENTRY_WORDS]> {
        if self.pending_nonce_entries() == 0 {
            return None;
        }
        let mut entry = [0u32; fpga::NONCE_ENTRY_WORDS];
        for (index, slot) in entry.iter_mut().enumerate() {
            *slot = self.read_word(fpga::NONCE_FIFO_DATA[index % 2]);
        }
        Some(entry)
    }
}

impl Drop for AxiFpga {
    fn drop(&mut self) {
        // SAFETY: `base`/`map_len` describe the mapping we created and still own.
        unsafe {
            libc::munmap(self.base.as_ptr().cast::<libc::c_void>(), self.map_len);
        }
    }
}

/// Pack a byte slice into big-endian 32-bit words, zero-padding the final word.
fn pack_be_words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks(4)
        .map(|chunk| {
            let mut buf = [0u8; 4];
            buf[..chunk.len()].copy_from_slice(chunk);
            u32::from_be_bytes(buf)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bm1398::VilCommand;

    #[test]
    fn anonymous_mapping_round_trips_register_words() {
        let mut fpga = AxiFpga::map_anonymous().expect("anon mmap");
        fpga.write_word(fpga::HARDWARE_VERSION, 0xDEAD_BEEF);
        fpga.write_word(fpga::NONCE_CONTROL, 0x0000_001A);
        assert_eq!(fpga.fpga_version(), 0xDEAD_BEEF);
        assert_eq!(fpga.read_word(fpga::NONCE_CONTROL), 0x0000_001A);
    }

    #[test]
    fn fresh_mapping_reads_back_zero() {
        let fpga = AxiFpga::map_anonymous().expect("anon mmap");
        assert_eq!(fpga.fpga_version(), 0);
        assert_eq!(fpga.pending_nonce_entries(), 0);
        assert!(!fpga.command_busy());
    }

    #[test]
    fn write_command_frame_packs_data_words_and_triggers_control() {
        let mut fpga = AxiFpga::map_anonymous().expect("anon mmap");
        let frame = VilCommand::SetChipAddress { chip_address: 0x04 }.to_frame();
        fpga.write_command_frame(2, &frame);

        // First command-data word holds the first big-endian packed word.
        let words = pack_be_words(&frame);
        assert_eq!(fpga.read_word(fpga::COMMAND_DATA[0]), words[0]);
        // Control register carries the trigger bits plus the chain index.
        assert_eq!(
            fpga.read_word(fpga::COMMAND_CONTROL),
            fpga::COMMAND_TRIGGER | (2u32 << 16)
        );
    }

    #[test]
    fn read_nonce_entry_respects_status_and_reads_both_data_regs() {
        let mut fpga = AxiFpga::map_anonymous().expect("anon mmap");
        // Empty FIFO -> nothing to read.
        assert!(fpga.read_nonce_entry().is_none());
        // Status low bits = word count; entry count = (status & 0x7fff) >> 1.
        // Present one entry by setting the count to 2 words.
        fpga.write_word(fpga::NONCE_FIFO_STATUS, 2);
        assert_eq!(fpga.pending_nonce_entries(), 1);
        fpga.write_word(fpga::NONCE_FIFO_DATA[0], 0x1111_1111);
        fpga.write_word(fpga::NONCE_FIFO_DATA[1], 0x2222_2222);
        // Entry drains regs 4,5,4,5 alternately.
        assert_eq!(
            fpga.read_nonce_entry(),
            Some([0x1111_1111, 0x2222_2222, 0x1111_1111, 0x2222_2222])
        );
    }

    #[test]
    fn enable_nonce_rx_sets_enable_bit() {
        let mut fpga = AxiFpga::map_anonymous().expect("anon mmap");
        fpga.enable_nonce_rx();
        assert_eq!(fpga.read_word(fpga::NONCE_CONTROL), fpga::NONCE_RX_ENABLE);
    }

    #[test]
    fn push_work_routes_first_and_continuation_words() {
        let mut fpga = AxiFpga::map_anonymous().expect("anon mmap");
        // Eight bytes -> two words: first to WORK_FIFO_FIRST, second to WORK_FIFO_DATA.
        fpga.push_work(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        assert_eq!(fpga.read_word(fpga::WORK_FIFO_FIRST), 0x1122_3344);
        assert_eq!(fpga.read_word(fpga::WORK_FIFO_DATA), 0x5566_7788);
    }

    #[test]
    fn work_fifo_ready_tests_per_chain_bit() {
        let mut fpga = AxiFpga::map_anonymous().expect("anon mmap");
        assert!(!fpga.work_fifo_ready(2));
        fpga.write_word(fpga::WORK_FIFO_READY, 1 << 2);
        assert!(fpga.work_fifo_ready(2));
        assert!(!fpga.work_fifo_ready(3));
    }

    fn temp_window_file() -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("omo-axi-ro-{unique}.bin"));
        std::fs::write(&path, vec![0u8; WINDOW_BYTES]).unwrap();
        path
    }

    #[test]
    fn probe_readonly_reads_the_version_register() {
        let path = temp_window_file();
        let mut buf = vec![0u8; WINDOW_BYTES];
        buf[0..4].copy_from_slice(&0x1234_5678u32.to_ne_bytes());
        std::fs::write(&path, &buf).unwrap();

        let probe = probe_readonly(&path).expect("read-only probe");
        assert_eq!(probe.fpga_version, 0x1234_5678);
        assert!(!probe.command_busy);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    #[should_panic(expected = "read-only")]
    fn read_only_mapping_rejects_writes() {
        let path = temp_window_file();
        let mut fpga = AxiFpga::map_device_readonly(&path).expect("ro map");
        assert!(fpga.is_readonly());
        fpga.write_word(fpga::HARDWARE_VERSION, 1); // must panic before faulting
    }

    #[test]
    fn pack_be_words_zero_pads_trailing_bytes() {
        assert_eq!(pack_be_words(&[0x55, 0xAA, 0x40]), vec![0x55AA_4000]);
        assert_eq!(
            pack_be_words(&[0x55, 0xAA, 0x40, 0x05, 0x04]),
            vec![0x55AA_4005, 0x0400_0000]
        );
    }
}
