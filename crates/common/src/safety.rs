use crate::{HardwareIdentityReport, HardwareIdentityState, RuntimeBackendMode, SupportLevel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareSafetyState {
    SimulationOnly,
    HardwareProbeReadOnly,
    BlockedIdentityConflict,
    BlockedUnknownIdentity,
    BlockedUnsupportedTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareSafetyGate {
    pub state: HardwareSafetyState,
    pub configured_target_accepted: bool,
    pub identity_confirmed: bool,
    pub simulated_mining_allowed: bool,
    pub hardware_mining_allowed: bool,
    pub asic_bus_writes_allowed: bool,
    pub tuning_writes_allowed: bool,
    pub flashing_allowed: bool,
    pub reasons: Vec<String>,
}

pub fn evaluate_hardware_safety(
    backend: RuntimeBackendMode,
    support: SupportLevel,
    identity: &HardwareIdentityReport,
) -> HardwareSafetyGate {
    if support == SupportLevel::Unsupported {
        return blocked(
            HardwareSafetyState::BlockedUnsupportedTarget,
            "configured target is unsupported in build 0.1.0",
            false,
            identity_confirmed(identity),
        );
    }

    match identity.state {
        HardwareIdentityState::Conflict => {
            return blocked(
                HardwareSafetyState::BlockedIdentityConflict,
                "detected identity conflicts with configured target",
                false,
                false,
            );
        }
        HardwareIdentityState::Unknown if backend != RuntimeBackendMode::Simulated => {
            return blocked(
                HardwareSafetyState::BlockedUnknownIdentity,
                "hardware identity is unknown on a non-simulated backend",
                true,
                false,
            );
        }
        _ => {}
    }

    match backend {
        RuntimeBackendMode::Simulated => HardwareSafetyGate {
            state: HardwareSafetyState::SimulationOnly,
            configured_target_accepted: true,
            identity_confirmed: false,
            simulated_mining_allowed: true,
            hardware_mining_allowed: false,
            asic_bus_writes_allowed: false,
            tuning_writes_allowed: false,
            flashing_allowed: false,
            reasons: vec![
                "simulated backend may report synthetic mining state".to_string(),
                "hardware mining, flashing, tuning writes, and ASIC bus writes are disabled"
                    .to_string(),
            ],
        },
        RuntimeBackendMode::HardwareProbe => HardwareSafetyGate {
            state: HardwareSafetyState::HardwareProbeReadOnly,
            configured_target_accepted: true,
            identity_confirmed: identity_confirmed(identity),
            simulated_mining_allowed: false,
            hardware_mining_allowed: false,
            asic_bus_writes_allowed: false,
            tuning_writes_allowed: false,
            flashing_allowed: false,
            reasons: vec![
                "hardware-probe backend is read-only".to_string(),
                "no GPIO, UART, I2C, SPI, fan, voltage, clock, flash, or ASIC commands are allowed"
                    .to_string(),
            ],
        },
    }
}

fn blocked(
    state: HardwareSafetyState,
    reason: &str,
    configured_target_accepted: bool,
    identity_confirmed: bool,
) -> HardwareSafetyGate {
    HardwareSafetyGate {
        state,
        configured_target_accepted,
        identity_confirmed,
        simulated_mining_allowed: false,
        hardware_mining_allowed: false,
        asic_bus_writes_allowed: false,
        tuning_writes_allowed: false,
        flashing_allowed: false,
        reasons: vec![reason.to_string()],
    }
}

fn identity_confirmed(identity: &HardwareIdentityReport) -> bool {
    matches!(identity.state, HardwareIdentityState::Inferred)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoardFamily, HardwareIdentityConfidence, Model};

    #[test]
    fn simulated_backend_allows_only_simulated_mining() {
        let identity = HardwareIdentityReport::simulated(BoardFamily::Xilinx, Model::S19jPro);
        let gate = evaluate_hardware_safety(
            RuntimeBackendMode::Simulated,
            SupportLevel::MvpStable,
            &identity,
        );

        assert_eq!(gate.state, HardwareSafetyState::SimulationOnly);
        assert!(gate.simulated_mining_allowed);
        assert!(!gate.hardware_mining_allowed);
        assert!(!gate.asic_bus_writes_allowed);
        assert!(!gate.flashing_allowed);
    }

    #[test]
    fn hardware_probe_is_read_only_even_when_identity_is_confirmed() {
        let mut identity = HardwareIdentityReport::simulated(BoardFamily::Xilinx, Model::S19jPro);
        identity.backend = RuntimeBackendMode::HardwareProbe;
        identity.state = HardwareIdentityState::Inferred;
        identity.confidence = HardwareIdentityConfidence::High;
        identity.detected_board = Some(BoardFamily::Xilinx);
        identity.detected_model = Some(Model::S19jPro);
        let gate = evaluate_hardware_safety(
            RuntimeBackendMode::HardwareProbe,
            SupportLevel::MvpStable,
            &identity,
        );

        assert_eq!(gate.state, HardwareSafetyState::HardwareProbeReadOnly);
        assert!(gate.identity_confirmed);
        assert!(!gate.hardware_mining_allowed);
        assert!(!gate.tuning_writes_allowed);
    }

    #[test]
    fn identity_conflict_blocks_everything() {
        let mut identity = HardwareIdentityReport::simulated(BoardFamily::Xilinx, Model::S19jPro);
        identity.backend = RuntimeBackendMode::HardwareProbe;
        identity.state = HardwareIdentityState::Conflict;
        identity.detected_board = Some(BoardFamily::BeagleBone);
        let gate = evaluate_hardware_safety(
            RuntimeBackendMode::HardwareProbe,
            SupportLevel::MvpStable,
            &identity,
        );

        assert_eq!(gate.state, HardwareSafetyState::BlockedIdentityConflict);
        assert!(!gate.configured_target_accepted);
        assert!(!gate.simulated_mining_allowed);
        assert!(!gate.hardware_mining_allowed);
    }

    #[test]
    fn unsupported_target_blocks_everything() {
        let identity = HardwareIdentityReport::simulated(BoardFamily::Cvitek, Model::S19jPro);
        let gate = evaluate_hardware_safety(
            RuntimeBackendMode::Simulated,
            SupportLevel::Unsupported,
            &identity,
        );

        assert_eq!(gate.state, HardwareSafetyState::BlockedUnsupportedTarget);
        assert!(!gate.configured_target_accepted);
        assert!(!gate.simulated_mining_allowed);
    }
}
