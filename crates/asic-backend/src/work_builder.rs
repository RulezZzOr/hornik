//! Build a BM1398 [`WorkItem`] from a full Stratum mining job.
//!
//! This bridges the pure mining crypto in `openmineros_common::mining` (coinbase
//! assembly, merkle root, version-rolled SHA-256 midstates) to the BM1398 work
//! wire format ([`crate::bm1398::WorkItem`]). It is the piece that was missing
//! between a received `mining.notify` and an ASIC dispatch.

use crate::bm1398::WorkItem;
use openmineros_common::mining::{MiningJob, VERSION_ROLL_MIDSTATES};

/// Build a [`WorkItem`] from a job, its extranonce pair, and four header version
/// values for version rolling (AsicBoost). `starting_nonce` is normally `0`.
///
/// The four version values produce four midstates; the chip rolls the version
/// field across them while sweeping the nonce space.
pub fn build_work_item(
    job: &MiningJob,
    extranonce1: &[u8],
    extranonce2: &[u8],
    version_rolls: [u32; VERSION_ROLL_MIDSTATES],
    work_id: u8,
    starting_nonce: u32,
) -> WorkItem {
    let merkle_root = job.merkle_root(extranonce1, extranonce2);
    let (midstates, merkle_root_tail) = job.midstates(&merkle_root, version_rolls);
    WorkItem {
        work_id,
        starting_nonce,
        nbits: job.nbits,
        ntime: job.ntime,
        merkle_root_tail,
        midstates: midstates.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_job() -> MiningJob {
        MiningJob {
            version: 0x2000_0000,
            prev_hash: [0xab; 32],
            coinb1: vec![0x01, 0x02, 0x03],
            coinb2: vec![0x09, 0x0a],
            merkle_branches: vec![[0x55; 32]],
            nbits: 0x1700_7fff,
            ntime: 0x6500_0000,
        }
    }

    #[test]
    fn build_work_item_produces_a_valid_148_byte_frame() {
        let job = sample_job();
        let work = build_work_item(
            &job,
            &[0xde, 0xad],
            &[0xbe, 0xef],
            [0x2000_0000, 0x2000_2000, 0x2000_4000, 0x2000_6000],
            7,
            0,
        );
        assert_eq!(work.work_id, 7);
        assert_eq!(work.nbits, job.nbits);
        assert_eq!(work.ntime, job.ntime);
        assert_eq!(work.midstates.len(), VERSION_ROLL_MIDSTATES);

        let frame = work.to_frame().expect("valid work frame");
        assert_eq!(frame.len(), crate::bm1398::WORK_FRAME_LEN);
    }

    #[test]
    fn build_work_item_tail_matches_merkle_root() {
        let job = sample_job();
        let root = job.merkle_root(&[0x00], &[0x00]);
        let work = build_work_item(&job, &[0x00], &[0x00], [job.version; 4], 1, 0);
        assert_eq!(work.merkle_root_tail, [root[28], root[29], root[30], root[31]]);
    }

    #[test]
    fn different_extranonce2_changes_the_work() {
        let job = sample_job();
        let rolls = [job.version; 4];
        let a = build_work_item(&job, &[0x01], &[0x00], rolls, 1, 0);
        let b = build_work_item(&job, &[0x01], &[0x01], rolls, 1, 0);
        // A different extranonce2 changes the coinbase, hence the midstates.
        assert_ne!(a.midstates, b.midstates);
    }
}
