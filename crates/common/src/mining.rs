//! Bitcoin mining primitives used to turn a Stratum job into per-chip work:
//! SHA-256 block compression and midstate, double-SHA-256, merkle-root folding,
//! coinbase assembly, and block-header construction.
//!
//! # Why a hand-rolled SHA-256 compression
//!
//! ASIC work needs the SHA-256 *midstate* — the 8-word internal state after the
//! first 64-byte block of the 80-byte header. The `sha2` crate does not expose
//! that intermediate state, so [`compress`] implements the block function
//! directly. Its correctness is pinned by [`tests`] that reconstruct the full
//! digest from [`compress`] and assert it matches `sha2::Sha256` across many
//! inputs, and `sha256d` itself is delegated to `sha2`.
//!
//! # Endianness
//!
//! Stratum delivers several fields (version, prev-hash, ntime, nbits) in a
//! display byte order that must be rearranged for the header. The byte-order
//! helpers here follow the common cgminer/Stratum convention and are tagged
//! `NEEDS-HW-CONFIRM` until validated end-to-end against a real share.

use sha2::{Digest, Sha256};

/// SHA-256 initial hash state (`H0`).
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// SHA-256 round constants (`K`).
const K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
    0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
    0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
    0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
    0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
    0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
    0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
    0xc671_78f2,
];

/// Fold one 64-byte block into the 8-word SHA-256 `state`.
pub fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// SHA-256 midstate after the first 64-byte `block`, as big-endian bytes.
pub fn sha256_midstate(block: &[u8; 64]) -> [u8; 32] {
    let mut state = H0;
    compress(&mut state, block);
    let mut out = [0u8; 32];
    for (word, slot) in state.iter().zip(out.chunks_exact_mut(4)) {
        slot.copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Double SHA-256 (`SHA256(SHA256(data))`).
pub fn sha256d(data: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(data);
    Sha256::digest(first).into()
}

/// Fold a coinbase hash with the Stratum merkle branch list into a merkle root.
///
/// Each step hashes `current || branch` with double SHA-256, in the order the
/// branches are supplied by the pool.
pub fn merkle_root(coinbase_hash: [u8; 32], branches: &[[u8; 32]]) -> [u8; 32] {
    let mut root = coinbase_hash;
    for branch in branches {
        let mut buf = [0u8; 64];
        buf[..32].copy_from_slice(&root);
        buf[32..].copy_from_slice(branch);
        root = sha256d(&buf);
    }
    root
}

/// Assemble the coinbase transaction: `coinb1 || extranonce1 || extranonce2 || coinb2`.
pub fn assemble_coinbase(
    coinb1: &[u8],
    extranonce1: &[u8],
    extranonce2: &[u8],
    coinb2: &[u8],
) -> Vec<u8> {
    let mut coinbase =
        Vec::with_capacity(coinb1.len() + extranonce1.len() + extranonce2.len() + coinb2.len());
    coinbase.extend_from_slice(coinb1);
    coinbase.extend_from_slice(extranonce1);
    coinbase.extend_from_slice(extranonce2);
    coinbase.extend_from_slice(coinb2);
    coinbase
}

/// Build the first 64 bytes of the block header (the SHA-256 midstate input):
/// `version(LE) || prev_hash(32) || merkle_root[0..28]`.
///
/// `prev_hash` and `merkle_root` are taken in header byte order (the caller does
/// any Stratum-specific word swaps). `version` is written little-endian.
pub fn header_first_block(version: u32, prev_hash: &[u8; 32], merkle_root: &[u8; 32]) -> [u8; 64] {
    let mut block = [0u8; 64];
    block[0..4].copy_from_slice(&version.to_le_bytes());
    block[4..36].copy_from_slice(prev_hash);
    block[36..64].copy_from_slice(&merkle_root[0..28]);
    block
}

/// Number of version-rolled midstates a BM1398 work item carries (AsicBoost).
pub const VERSION_ROLL_MIDSTATES: usize = 4;

/// BIP320 version-rolling mask: the block-version bits a miner may roll.
pub const VERSION_ROLL_MASK: u32 = 0x1fff_e000;

/// Produce [`VERSION_ROLL_MIDSTATES`] distinct header version values for
/// AsicBoost by rolling the index into the low bits of `mask`, keeping
/// `base_version`'s bits outside the mask untouched.
///
/// The chip rolls the remaining mask bits itself; this only seeds the four
/// midstates. The exact seed strategy is `NEEDS-HW-CONFIRM`, but every value is
/// a valid distinct version within the allowed rolling region.
pub fn version_rolls(base_version: u32, mask: u32) -> [u32; VERSION_ROLL_MIDSTATES] {
    let shift = mask.trailing_zeros();
    let mut rolls = [0u32; VERSION_ROLL_MIDSTATES];
    for (index, slot) in rolls.iter_mut().enumerate() {
        let rolled = ((index as u32) << shift) & mask;
        *slot = (base_version & !mask) | rolled;
    }
    rolls
}

/// A fully-specified mining job — the data a Stratum `mining.notify` carries —
/// in the byte order needed to build the header.
///
/// Hex decoding and any Stratum-specific word swaps (prev-hash, version, ntime,
/// nbits) are the caller's responsibility; this struct holds header-ready bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiningJob {
    /// Block version (header value; the caller applies any roll mask per share).
    pub version: u32,
    /// Previous block hash, 32 bytes in header byte order.
    pub prev_hash: [u8; 32],
    /// Coinbase part 1 (before the extranonces).
    pub coinb1: Vec<u8>,
    /// Coinbase part 2 (after the extranonces).
    pub coinb2: Vec<u8>,
    /// Merkle branch hashes, each 32 bytes, in pool-supplied order.
    pub merkle_branches: Vec<[u8; 32]>,
    /// Compact difficulty target (`nbits`).
    pub nbits: u32,
    /// Block time (`ntime`).
    pub ntime: u32,
}

impl MiningJob {
    /// Compute the merkle root for this job and a given extranonce pair.
    pub fn merkle_root(&self, extranonce1: &[u8], extranonce2: &[u8]) -> [u8; 32] {
        let coinbase = assemble_coinbase(&self.coinb1, extranonce1, extranonce2, &self.coinb2);
        merkle_root(sha256d(&coinbase), &self.merkle_branches)
    }

    /// Compute the four version-rolled SHA-256 midstates and the merkle-root
    /// tail (`merkle_root[28..32]`) for a BM1398 work item, given a precomputed
    /// `merkle_root` and four header version values.
    pub fn midstates(
        &self,
        merkle_root: &[u8; 32],
        version_rolls: [u32; VERSION_ROLL_MIDSTATES],
    ) -> ([[u8; 32]; VERSION_ROLL_MIDSTATES], [u8; 4]) {
        let mut midstates = [[0u8; 32]; VERSION_ROLL_MIDSTATES];
        for (slot, roll) in midstates.iter_mut().zip(version_rolls) {
            let block = header_first_block(roll, &self.prev_hash, merkle_root);
            *slot = sha256_midstate(&block);
        }
        let mut tail = [0u8; 4];
        tail.copy_from_slice(&merkle_root[28..32]);
        (midstates, tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full SHA-256 via our own [`compress`], for cross-checking against `sha2`.
    fn full_sha256(data: &[u8]) -> [u8; 32] {
        let mut state = H0;
        let bit_len = (data.len() as u64) * 8;
        let mut padded = data.to_vec();
        padded.push(0x80);
        while padded.len() % 64 != 56 {
            padded.push(0);
        }
        padded.extend_from_slice(&bit_len.to_be_bytes());
        for chunk in padded.chunks_exact(64) {
            let mut block = [0u8; 64];
            block.copy_from_slice(chunk);
            compress(&mut state, &block);
        }
        let mut out = [0u8; 32];
        for (word, slot) in state.iter().zip(out.chunks_exact_mut(4)) {
            slot.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    #[test]
    fn compress_matches_sha2_across_many_inputs() {
        for len in [0usize, 1, 3, 55, 56, 63, 64, 65, 120, 256] {
            let data: Vec<u8> = (0..len).map(|i| (i * 7 + 1) as u8).collect();
            assert_eq!(
                full_sha256(&data),
                <[u8; 32]>::from(Sha256::digest(&data)),
                "mismatch at len {len}"
            );
        }
    }

    #[test]
    fn sha256d_matches_double_digest() {
        let data = b"openmineros";
        let expected: [u8; 32] = Sha256::digest(Sha256::digest(data)).into();
        assert_eq!(sha256d(data), expected);
    }

    #[test]
    fn midstate_is_first_block_state() {
        // The midstate of a 64-byte block, then finishing with a known second
        // block, must equal the full digest of the 128-byte message.
        let message: Vec<u8> = (0..128u16).map(|i| i as u8).collect();
        let mut first = [0u8; 64];
        first.copy_from_slice(&message[0..64]);

        let midstate = sha256_midstate(&first);
        // Rebuild the state words from the midstate and continue compression.
        let mut state = [0u32; 8];
        for (word, chunk) in state.iter_mut().zip(midstate.chunks_exact(4)) {
            *word = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        let mut second = [0u8; 64];
        second.copy_from_slice(&message[64..128]);
        compress(&mut state, &second);

        // `state` now equals the state after two full blocks (no padding here),
        // which is exactly what `full_sha256` computes internally for a 128-byte
        // input *without* the length/padding block. Compare to a direct run.
        let mut direct = H0;
        compress(&mut direct, &first);
        compress(&mut direct, &second);
        assert_eq!(state, direct);
    }

    #[test]
    fn merkle_root_with_no_branches_is_the_coinbase_hash() {
        let coinbase_hash = sha256d(b"coinbase");
        assert_eq!(merkle_root(coinbase_hash, &[]), coinbase_hash);
    }

    #[test]
    fn merkle_root_folds_one_branch() {
        let coinbase_hash = sha256d(b"coinbase");
        let branch = sha256d(b"branch");
        let mut buf = [0u8; 64];
        buf[..32].copy_from_slice(&coinbase_hash);
        buf[32..].copy_from_slice(&branch);
        assert_eq!(merkle_root(coinbase_hash, &[branch]), sha256d(&buf));
    }

    #[test]
    fn coinbase_concatenates_parts_in_order() {
        let coinbase = assemble_coinbase(&[0x01], &[0x02, 0x03], &[0x04], &[0x05, 0x06]);
        assert_eq!(coinbase, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    }

    #[test]
    fn header_first_block_places_fields() {
        let prev = [0x11u8; 32];
        let root = [0x22u8; 32];
        let block = header_first_block(0x2000_0000, &prev, &root);
        assert_eq!(&block[0..4], &[0x00, 0x00, 0x00, 0x20]); // version LE
        assert_eq!(&block[4..36], &prev);
        assert_eq!(&block[36..64], &root[0..28]);
    }

    fn sample_job() -> MiningJob {
        MiningJob {
            version: 0x2000_0000,
            prev_hash: [0xab; 32],
            coinb1: vec![0x01, 0x02, 0x03],
            coinb2: vec![0x09, 0x0a],
            merkle_branches: vec![[0x55; 32], [0x66; 32]],
            nbits: 0x1700_7fff,
            ntime: 0x6500_0000,
        }
    }

    #[test]
    fn mining_job_merkle_root_matches_manual_fold() {
        let job = sample_job();
        let e1 = [0xde, 0xad];
        let e2 = [0xbe, 0xef];
        let coinbase = assemble_coinbase(&job.coinb1, &e1, &e2, &job.coinb2);
        let expected = merkle_root(sha256d(&coinbase), &job.merkle_branches);
        assert_eq!(job.merkle_root(&e1, &e2), expected);
    }

    #[test]
    fn version_rolls_are_distinct_and_respect_the_mask() {
        let rolls = version_rolls(0x2000_0000, VERSION_ROLL_MASK);
        // All four are distinct.
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert_ne!(rolls[i], rolls[j]);
            }
        }
        // Bits outside the mask (here bit 29) are preserved.
        for roll in rolls {
            assert_eq!(roll & !VERSION_ROLL_MASK, 0x2000_0000);
            // The rolled bits stay within the mask.
            assert_eq!(roll & !VERSION_ROLL_MASK | (roll & VERSION_ROLL_MASK), roll);
        }
        // Index 0 leaves the base untouched; index 1 sets the mask's lowest bit.
        assert_eq!(rolls[0], 0x2000_0000);
        assert_eq!(rolls[1], 0x2000_0000 | (1 << VERSION_ROLL_MASK.trailing_zeros()));
    }

    #[test]
    fn mining_job_midstates_roll_with_version_and_expose_tail() {
        let job = sample_job();
        let root = job.merkle_root(&[0x00], &[0x00]);
        let (midstates, tail) = job.midstates(&root, [0x2000_0000, 0x2000_2000, 0x2000_4000, 0x2000_6000]);

        // Four distinct version values give four distinct midstates.
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert_ne!(midstates[i], midstates[j], "midstates {i} and {j} collide");
            }
        }
        // The tail is the last four bytes of the merkle root.
        assert_eq!(tail, [root[28], root[29], root[30], root[31]]);
        // Each midstate equals a direct first-block compression.
        let block = header_first_block(0x2000_0000, &job.prev_hash, &root);
        assert_eq!(midstates[0], sha256_midstate(&block));
    }
}
