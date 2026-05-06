use openmineros_common::{
    BoardFamily, BoardProfile, Capability, CapabilitySet, ChainStatus, MinerMode, MinerStatus,
    Model, SupportLevel, TargetError, supported_targets,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error(transparent)]
    Target(#[from] TargetError),
}

#[derive(Debug, Clone)]
pub struct SimulatedBackend {
    model: Model,
    profile: BoardProfile,
    support: SupportLevel,
}

impl SimulatedBackend {
    pub fn new(model: Model, board: BoardFamily) -> Result<Self, BackendError> {
        let support = supported_targets()
            .into_iter()
            .find(|target| target.model == model && target.board == board)
            .map(|target| target.support)
            .ok_or(TargetError::UnsupportedTarget { model, board })?;

        let profile = match board {
            BoardFamily::Xilinx => openmineros_board_xil::profile(),
            BoardFamily::BeagleBone => openmineros_board_bb::profile(),
            BoardFamily::Amlogic => openmineros_board_aml::profile(),
            BoardFamily::Cvitek => BoardProfile {
                family: BoardFamily::Cvitek,
                soc: "CV1835",
                recovery: "unsupported in 0.1.0",
                capabilities: CapabilitySet::from_flags([Capability::SafeMode]),
            },
        };

        Ok(Self {
            model,
            profile,
            support,
        })
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn profile(&self) -> &BoardProfile {
        &self.profile
    }

    pub fn support(&self) -> SupportLevel {
        self.support
    }

    pub fn miner_status(&self) -> MinerStatus {
        let hashrate_ths = match self.model {
            Model::S19 => 95.0,
            Model::S19Pro => 110.0,
            Model::S19j => 90.0,
            Model::S19jPro => 104.0,
            Model::S19Xp => 141.0,
            Model::T19 => 84.0,
        };
        let power_w = match self.model {
            Model::S19Xp => 3010,
            Model::S19Pro => 3250,
            _ => 3068,
        };

        MinerStatus {
            hashrate_ths,
            power_w,
            efficiency_j_th: f64::from(power_w) / hashrate_ths,
            accepted_shares: 0,
            rejected_shares: 0,
            mode: MinerMode::Balanced,
        }
    }

    pub fn chain_statuses(&self) -> Vec<ChainStatus> {
        (0..3)
            .map(|id| ChainStatus {
                id,
                present: true,
                enabled: true,
                asic_detected: 42,
                temp_board_c: 62.0 + f64::from(id),
                temp_chip_max_c: 78.0 + f64::from(id),
                fault: None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_all_mvp_backends() {
        for board in [
            BoardFamily::Xilinx,
            BoardFamily::BeagleBone,
            BoardFamily::Amlogic,
        ] {
            let backend = SimulatedBackend::new(Model::S19jPro, board).unwrap();
            assert_eq!(backend.profile().family, board);
            assert_eq!(backend.chain_statuses().len(), 3);
        }
    }
}
