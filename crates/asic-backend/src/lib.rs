use openmineros_common::{
    BoardFamily, BoardProfile, Capability, CapabilitySet, ChainStatus, HealthStatus, MinerMode,
    MinerStatus, Model, RuntimeBackendMode, Severity, SupportLevel, TargetError, supported_targets,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error(transparent)]
    Target(#[from] TargetError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendRuntimeStatus {
    pub state: HealthStatus,
    pub severity: Severity,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum BackendHandle {
    Simulated(SimulatedBackend),
    HardwareProbe(HardwareProbeBackend),
}

impl BackendHandle {
    pub fn new(
        mode: RuntimeBackendMode,
        model: Model,
        board: BoardFamily,
    ) -> Result<Self, BackendError> {
        match mode {
            RuntimeBackendMode::Simulated => {
                Ok(Self::Simulated(SimulatedBackend::new(model, board)?))
            }
            RuntimeBackendMode::HardwareProbe => Ok(Self::HardwareProbe(
                HardwareProbeBackend::new(model, board)?,
            )),
        }
    }

    pub fn mode(&self) -> RuntimeBackendMode {
        match self {
            Self::Simulated(backend) => backend.mode(),
            Self::HardwareProbe(backend) => backend.mode(),
        }
    }

    pub fn model(&self) -> Model {
        match self {
            Self::Simulated(backend) => backend.model(),
            Self::HardwareProbe(backend) => backend.model(),
        }
    }

    pub fn profile(&self) -> &BoardProfile {
        match self {
            Self::Simulated(backend) => backend.profile(),
            Self::HardwareProbe(backend) => backend.profile(),
        }
    }

    pub fn support(&self) -> SupportLevel {
        match self {
            Self::Simulated(backend) => backend.support(),
            Self::HardwareProbe(backend) => backend.support(),
        }
    }

    pub fn runtime_status(&self) -> BackendRuntimeStatus {
        match self {
            Self::Simulated(backend) => backend.runtime_status(),
            Self::HardwareProbe(backend) => backend.runtime_status(),
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        match self {
            Self::Simulated(backend) => backend.miner_status(),
            Self::HardwareProbe(backend) => backend.miner_status(),
        }
    }

    pub fn chain_statuses(&self) -> Vec<ChainStatus> {
        match self {
            Self::Simulated(backend) => backend.chain_statuses(),
            Self::HardwareProbe(backend) => backend.chain_statuses(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimulatedBackend {
    model: Model,
    profile: BoardProfile,
    support: SupportLevel,
}

impl SimulatedBackend {
    pub fn new(model: Model, board: BoardFamily) -> Result<Self, BackendError> {
        let support = target_support(model, board)?;
        let profile = board_profile(board);

        Ok(Self {
            model,
            profile,
            support,
        })
    }

    pub fn mode(&self) -> RuntimeBackendMode {
        RuntimeBackendMode::Simulated
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

    pub fn runtime_status(&self) -> BackendRuntimeStatus {
        BackendRuntimeStatus {
            state: HealthStatus::Mining,
            severity: Severity::Ok,
            issues: Vec::new(),
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        simulated_miner_status(self.model)
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

#[derive(Debug, Clone)]
pub struct HardwareProbeBackend {
    model: Model,
    profile: BoardProfile,
    support: SupportLevel,
}

impl HardwareProbeBackend {
    pub fn new(model: Model, board: BoardFamily) -> Result<Self, BackendError> {
        let support = target_support(model, board)?;
        let profile = board_profile(board);

        Ok(Self {
            model,
            profile,
            support,
        })
    }

    pub fn mode(&self) -> RuntimeBackendMode {
        RuntimeBackendMode::HardwareProbe
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

    pub fn runtime_status(&self) -> BackendRuntimeStatus {
        BackendRuntimeStatus {
            state: HealthStatus::Recovering,
            severity: Severity::Warn,
            issues: vec![
                "hardware-probe backend is a read-only bring-up scaffold in build 0.1.0"
                    .to_string(),
                "ASIC bus probing is not implemented yet; no mining is started".to_string(),
            ],
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        MinerStatus {
            hashrate_ths: 0.0,
            power_w: 0,
            efficiency_j_th: 0.0,
            accepted_shares: 0,
            rejected_shares: 0,
            mode: MinerMode::SafeMode,
        }
    }

    pub fn chain_statuses(&self) -> Vec<ChainStatus> {
        (0..3)
            .map(|id| ChainStatus {
                id,
                present: false,
                enabled: false,
                asic_detected: 0,
                temp_board_c: 0.0,
                temp_chip_max_c: 0.0,
                fault: Some("hardware probing is not implemented in build 0.1.0".to_string()),
            })
            .collect()
    }
}

fn target_support(model: Model, board: BoardFamily) -> Result<SupportLevel, BackendError> {
    supported_targets()
        .into_iter()
        .find(|target| target.model == model && target.board == board)
        .map(|target| target.support)
        .ok_or(TargetError::UnsupportedTarget { model, board })
        .map_err(BackendError::Target)
}

fn board_profile(board: BoardFamily) -> BoardProfile {
    match board {
        BoardFamily::Xilinx => openmineros_board_xil::profile(),
        BoardFamily::BeagleBone => openmineros_board_bb::profile(),
        BoardFamily::Amlogic => openmineros_board_aml::profile(),
        BoardFamily::Cvitek => BoardProfile {
            family: BoardFamily::Cvitek,
            soc: "CV1835",
            recovery: "unsupported in 0.1.0",
            capabilities: CapabilitySet::from_flags([Capability::SafeMode]),
        },
    }
}

fn simulated_miner_status(model: Model) -> MinerStatus {
    let hashrate_ths = match model {
        Model::S19 => 95.0,
        Model::S19Pro => 110.0,
        Model::S19j => 90.0,
        Model::S19jPro => 104.0,
        Model::S19Xp => 141.0,
        Model::T19 => 84.0,
    };
    let power_w = match model {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_all_mvp_simulated_backends() {
        for board in [
            BoardFamily::Xilinx,
            BoardFamily::BeagleBone,
            BoardFamily::Amlogic,
        ] {
            let backend = SimulatedBackend::new(Model::S19jPro, board).unwrap();
            assert_eq!(backend.mode(), RuntimeBackendMode::Simulated);
            assert_eq!(backend.profile().family, board);
            assert_eq!(backend.chain_statuses().len(), 3);
        }
    }

    #[test]
    fn hardware_probe_backend_does_not_pretend_to_mine() {
        let backend = HardwareProbeBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let runtime = backend.runtime_status();
        let miner = backend.miner_status();
        let chains = backend.chain_statuses();

        assert_eq!(backend.mode(), RuntimeBackendMode::HardwareProbe);
        assert_eq!(runtime.state, HealthStatus::Recovering);
        assert_eq!(runtime.severity, Severity::Warn);
        assert_eq!(miner.hashrate_ths, 0.0);
        assert_eq!(miner.mode, MinerMode::SafeMode);
        assert!(chains.iter().all(|chain| !chain.present));
    }
}
