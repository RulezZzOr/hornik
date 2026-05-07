use openmineros_common::{
    BoardFamily, BoardProfile, Capability, CapabilitySet, ChainStatus, HardwareIdentityObservation,
    HardwareIdentityReport, HardwareProbeReport, HealthStatus, MinerMode, MinerStatus, Model,
    ProbeCheck, ProbeStatus, RuntimeBackendMode, Severity, SupportLevel, TargetError,
    infer_hardware_identity, supported_targets,
};
use std::path::Path;
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

    pub fn probe_report(&self) -> HardwareProbeReport {
        match self {
            Self::Simulated(backend) => backend.probe_report(),
            Self::HardwareProbe(backend) => backend.probe_report(),
        }
    }

    pub fn identity_report(&self) -> HardwareIdentityReport {
        match self {
            Self::Simulated(backend) => backend.identity_report(),
            Self::HardwareProbe(backend) => backend.identity_report(),
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

    pub fn probe_report(&self) -> HardwareProbeReport {
        HardwareProbeReport::new(
            self.mode(),
            self.model,
            self.profile.family,
            probe_plan(self.profile.family, ProbeMode::Skipped),
            vec![
                "simulated backend does not inspect host hardware".to_string(),
                "start with --backend hardware-probe for read-only board bring-up checks"
                    .to_string(),
            ],
        )
    }

    pub fn identity_report(&self) -> HardwareIdentityReport {
        HardwareIdentityReport::simulated(self.profile.family, self.model)
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

    pub fn probe_report(&self) -> HardwareProbeReport {
        HardwareProbeReport::new(
            self.mode(),
            self.model,
            self.profile.family,
            probe_plan(self.profile.family, ProbeMode::ReadOnlyFilesystem),
            vec![
                "checks only test whether expected OS paths exist".to_string(),
                "no GPIO, UART, I2C, SPI, fan, voltage, clock, or ASIC commands are issued"
                    .to_string(),
            ],
        )
    }

    pub fn identity_report(&self) -> HardwareIdentityReport {
        infer_hardware_identity(
            self.mode(),
            self.profile.family,
            self.model,
            identity_observations(self.profile.family, ProbeMode::ReadOnlyFilesystem),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeMode {
    Skipped,
    ReadOnlyFilesystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProbeExpectation {
    name: &'static str,
    interface: &'static str,
    path: &'static str,
    required: bool,
}

fn probe_plan(board: BoardFamily, mode: ProbeMode) -> Vec<ProbeCheck> {
    probe_expectations(board)
        .into_iter()
        .map(|expectation| probe_check(expectation, mode))
        .collect()
}

fn identity_observations(board: BoardFamily, mode: ProbeMode) -> Vec<HardwareIdentityObservation> {
    let mut observations = Vec::new();

    for expectation in probe_expectations(board) {
        if mode == ProbeMode::ReadOnlyFilesystem {
            let path = Path::new(expectation.path);
            if path.is_file() {
                if let Ok(value) = std::fs::read_to_string(path) {
                    observations.push(HardwareIdentityObservation {
                        source: expectation.interface.to_string(),
                        key: expectation.name.to_string(),
                        value: sanitize_observation_value(&value),
                    });
                }
            } else if path.exists() {
                observations.push(HardwareIdentityObservation {
                    source: expectation.interface.to_string(),
                    key: expectation.name.to_string(),
                    value: format!("{} present", expectation.path),
                });
            }
        }
    }

    observations
}

fn sanitize_observation_value(value: &str) -> String {
    value
        .replace('\0', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn probe_check(expectation: ProbeExpectation, mode: ProbeMode) -> ProbeCheck {
    let (status, detail) = match mode {
        ProbeMode::Skipped => (
            ProbeStatus::Skipped,
            "simulated backend skipped host filesystem probing".to_string(),
        ),
        ProbeMode::ReadOnlyFilesystem => {
            if Path::new(expectation.path).exists() {
                (
                    ProbeStatus::Detected,
                    "expected path exists on this host".to_string(),
                )
            } else {
                (
                    ProbeStatus::Missing,
                    "expected path is not present on this host".to_string(),
                )
            }
        }
    };

    ProbeCheck {
        name: expectation.name.to_string(),
        interface: expectation.interface.to_string(),
        path: expectation.path.to_string(),
        required: expectation.required,
        status,
        detail,
    }
}

fn probe_expectations(board: BoardFamily) -> Vec<ProbeExpectation> {
    match board {
        BoardFamily::Xilinx => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: true,
            },
            ProbeExpectation {
                name: "control UART",
                interface: "uart",
                path: "/dev/ttyPS0",
                required: true,
            },
            ProbeExpectation {
                name: "GPIO control",
                interface: "gpio",
                path: "/sys/class/gpio",
                required: true,
            },
            ProbeExpectation {
                name: "hardware monitor sensors",
                interface: "hwmon",
                path: "/sys/class/hwmon",
                required: false,
            },
        ],
        BoardFamily::BeagleBone => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: true,
            },
            ProbeExpectation {
                name: "control UART",
                interface: "uart",
                path: "/dev/ttyO1",
                required: true,
            },
            ProbeExpectation {
                name: "GPIO control",
                interface: "gpio",
                path: "/sys/class/gpio",
                required: true,
            },
            ProbeExpectation {
                name: "IIO sensor bus",
                interface: "iio",
                path: "/sys/bus/iio/devices",
                required: false,
            },
        ],
        BoardFamily::Amlogic => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: true,
            },
            ProbeExpectation {
                name: "control UART",
                interface: "uart",
                path: "/dev/ttyS1",
                required: true,
            },
            ProbeExpectation {
                name: "GPIO control",
                interface: "gpio",
                path: "/sys/class/gpio",
                required: true,
            },
            ProbeExpectation {
                name: "hardware monitor sensors",
                interface: "hwmon",
                path: "/sys/class/hwmon",
                required: false,
            },
        ],
        BoardFamily::Cvitek => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: false,
            },
            ProbeExpectation {
                name: "safe mode only",
                interface: "support-policy",
                path: "unsupported",
                required: false,
            },
        ],
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

    #[test]
    fn simulated_probe_report_is_skipped() {
        let backend = SimulatedBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let report = backend.probe_report();

        assert_eq!(report.backend, RuntimeBackendMode::Simulated);
        assert!(report.safe_read_only);
        assert_eq!(report.summary.skipped, report.summary.total);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == ProbeStatus::Skipped)
        );
    }

    #[test]
    fn simulated_identity_uses_configured_target_only() {
        let backend = SimulatedBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let identity = backend.identity_report();

        assert_eq!(
            identity.state,
            openmineros_common::HardwareIdentityState::ConfiguredOnly
        );
        assert_eq!(identity.configured_board, BoardFamily::Xilinx);
        assert_eq!(identity.configured_model, Model::S19jPro);
        assert!(identity.detected_board.is_none());
    }

    #[test]
    fn hardware_probe_report_has_required_xilinx_checks() {
        let backend = HardwareProbeBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let report = backend.probe_report();

        assert_eq!(report.backend, RuntimeBackendMode::HardwareProbe);
        assert_eq!(report.board_family, BoardFamily::Xilinx);
        assert!(report.safe_read_only);
        assert_eq!(report.summary.total, 4);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.interface == "uart" && check.path == "/dev/ttyPS0")
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.interface == "gpio" && check.required)
        );
    }

    #[test]
    fn hardware_probe_identity_is_read_only() {
        let backend = HardwareProbeBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let identity = backend.identity_report();

        assert!(identity.safe_read_only);
        assert_eq!(identity.backend, RuntimeBackendMode::HardwareProbe);
        assert_eq!(identity.configured_board, BoardFamily::Xilinx);
        assert_eq!(identity.configured_model, Model::S19jPro);
    }
}
