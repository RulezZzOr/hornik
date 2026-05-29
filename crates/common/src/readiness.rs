use crate::{
    BoardFamily, BoardProfile, CapabilitySet, HardwareSafetyGate, HardwareSafetyState, Model,
    RuntimeBackendMode, SupportLevel,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareReadinessState {
    SimulationReady,
    ReadOnlyIdentified,
    MiningReady,
    ReadOnlyNeedsIdentity,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareActionReadiness {
    pub action: String,
    pub allowed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareReadinessReport {
    pub schema_version: u8,
    pub backend: RuntimeBackendMode,
    pub model: Model,
    pub board_family: BoardFamily,
    pub support: SupportLevel,
    pub recovery: String,
    pub capabilities: CapabilitySet,
    pub state: HardwareReadinessState,
    pub actions: Vec<HardwareActionReadiness>,
    pub notes: Vec<String>,
}

impl HardwareReadinessReport {
    pub const SCHEMA_VERSION: u8 = 1;
}

pub fn evaluate_hardware_readiness(
    backend: RuntimeBackendMode,
    model: Model,
    profile: &BoardProfile,
    support: SupportLevel,
    safety: &HardwareSafetyGate,
) -> HardwareReadinessReport {
    let state = readiness_state(backend, support, safety);
    let notes = readiness_notes(state, support, safety);

    HardwareReadinessReport {
        schema_version: HardwareReadinessReport::SCHEMA_VERSION,
        backend,
        model,
        board_family: profile.family,
        support,
        recovery: profile.recovery.to_string(),
        capabilities: profile.capabilities.clone(),
        state,
        actions: vec![
            action(
                "simulated_mining",
                safety.simulated_mining_allowed,
                "synthetic mining state is only allowed in simulated backend",
            ),
            action(
                "hardware_mining",
                safety.hardware_mining_allowed,
                "real mining remains disabled until a hardware backend is implemented and gated",
            ),
            action(
                "asic_bus_writes",
                safety.asic_bus_writes_allowed,
                "ASIC bus writes remain disabled in build 0.1.0",
            ),
            action(
                "tuning_writes",
                safety.tuning_writes_allowed,
                "clock and voltage writes remain disabled in build 0.1.0",
            ),
            action(
                "flashing",
                safety.flashing_allowed,
                "flashing remains disabled until signed install/update flows are implemented",
            ),
        ],
        notes,
    }
}

fn readiness_state(
    backend: RuntimeBackendMode,
    support: SupportLevel,
    safety: &HardwareSafetyGate,
) -> HardwareReadinessState {
    if support == SupportLevel::Unsupported || !safety.configured_target_accepted {
        return HardwareReadinessState::Blocked;
    }

    match (backend, safety.state, safety.identity_confirmed) {
        (RuntimeBackendMode::Simulated, _, _) => HardwareReadinessState::SimulationReady,
        (RuntimeBackendMode::HardwareProbe, HardwareSafetyState::HardwareProbeReadOnly, true) => {
            HardwareReadinessState::ReadOnlyIdentified
        }
        (RuntimeBackendMode::HardwareProbe, _, _) => HardwareReadinessState::ReadOnlyNeedsIdentity,
        (RuntimeBackendMode::HardwareMining, HardwareSafetyState::HardwareMiningEnabled, true) => {
            HardwareReadinessState::MiningReady
        }
        (RuntimeBackendMode::HardwareMining, _, _) => HardwareReadinessState::ReadOnlyNeedsIdentity,
    }
}

fn readiness_notes(
    state: HardwareReadinessState,
    support: SupportLevel,
    safety: &HardwareSafetyGate,
) -> Vec<String> {
    let mut notes = Vec::new();

    notes.push(match state {
        HardwareReadinessState::SimulationReady => {
            "configured target is ready for API/UI simulation only".to_string()
        }
        HardwareReadinessState::ReadOnlyIdentified => {
            "configured target was identified through read-only evidence".to_string()
        }
        HardwareReadinessState::MiningReady => {
            "configured target is identified and ready for live hardware mining".to_string()
        }
        HardwareReadinessState::ReadOnlyNeedsIdentity => {
            "read-only hardware identity must be confirmed before future hardware actions"
                .to_string()
        }
        HardwareReadinessState::Blocked => {
            "configured target is blocked by safety policy".to_string()
        }
    });

    if support == SupportLevel::Experimental {
        notes.push("target is experimental until verified on real hardware".to_string());
    }

    notes.extend(safety.reasons.iter().cloned());
    notes
}

fn action(action: &str, allowed: bool, reason: &str) -> HardwareActionReadiness {
    HardwareActionReadiness {
        action: action.to_string(),
        allowed,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BoardFamily, Capability, HardwareIdentityConfidence, HardwareIdentityReport,
        HardwareIdentityState, evaluate_hardware_safety,
    };

    fn profile(family: BoardFamily) -> BoardProfile {
        BoardProfile {
            family,
            soc: "test",
            recovery: "test recovery",
            capabilities: CapabilitySet::from_flags([Capability::SafeMode]),
        }
    }

    fn identity(
        backend: RuntimeBackendMode,
        state: HardwareIdentityState,
        confidence: HardwareIdentityConfidence,
        detected_board: Option<BoardFamily>,
        detected_model: Option<Model>,
    ) -> HardwareIdentityReport {
        let mut identity = HardwareIdentityReport::simulated(BoardFamily::Xilinx, Model::S19jPro);
        identity.backend = backend;
        identity.state = state;
        identity.confidence = confidence;
        identity.detected_board = detected_board;
        identity.detected_model = detected_model;
        identity
    }

    #[test]
    fn simulated_target_is_ready_only_for_simulation() {
        let identity = HardwareIdentityReport::simulated(BoardFamily::Xilinx, Model::S19jPro);
        let safety = evaluate_hardware_safety(
            RuntimeBackendMode::Simulated,
            SupportLevel::MvpStable,
            &identity,
        );
        let report = evaluate_hardware_readiness(
            RuntimeBackendMode::Simulated,
            Model::S19jPro,
            &profile(BoardFamily::Xilinx),
            SupportLevel::MvpStable,
            &safety,
        );

        assert_eq!(report.state, HardwareReadinessState::SimulationReady);
        assert!(
            report
                .actions
                .iter()
                .any(|action| action.action == "simulated_mining" && action.allowed)
        );
        assert!(
            report
                .actions
                .iter()
                .any(|action| action.action == "hardware_mining" && !action.allowed)
        );
    }

    #[test]
    fn hardware_probe_identified_is_read_only_identified() {
        let identity = identity(
            RuntimeBackendMode::HardwareProbe,
            HardwareIdentityState::Inferred,
            HardwareIdentityConfidence::High,
            Some(BoardFamily::Xilinx),
            Some(Model::S19jPro),
        );
        let safety = evaluate_hardware_safety(
            RuntimeBackendMode::HardwareProbe,
            SupportLevel::MvpStable,
            &identity,
        );
        let report = evaluate_hardware_readiness(
            RuntimeBackendMode::HardwareProbe,
            Model::S19jPro,
            &profile(BoardFamily::Xilinx),
            SupportLevel::MvpStable,
            &safety,
        );

        assert_eq!(report.state, HardwareReadinessState::ReadOnlyIdentified);
    }

    #[test]
    fn hardware_probe_needs_identity_stays_read_only_needs_identity() {
        let identity = identity(
            RuntimeBackendMode::HardwareProbe,
            HardwareIdentityState::ConfiguredOnly,
            HardwareIdentityConfidence::Low,
            None,
            None,
        );
        let safety = evaluate_hardware_safety(
            RuntimeBackendMode::HardwareProbe,
            SupportLevel::MvpStable,
            &identity,
        );
        let report = evaluate_hardware_readiness(
            RuntimeBackendMode::HardwareProbe,
            Model::S19jPro,
            &profile(BoardFamily::Xilinx),
            SupportLevel::MvpStable,
            &safety,
        );

        assert_eq!(report.state, HardwareReadinessState::ReadOnlyNeedsIdentity);
    }

    #[test]
    fn hardware_mining_ready_is_distinct_from_read_only_identified() {
        let identity = identity(
            RuntimeBackendMode::HardwareMining,
            HardwareIdentityState::Inferred,
            HardwareIdentityConfidence::High,
            Some(BoardFamily::Xilinx),
            Some(Model::S19jPro),
        );
        let safety = evaluate_hardware_safety(
            RuntimeBackendMode::HardwareMining,
            SupportLevel::MvpStable,
            &identity,
        );
        let report = evaluate_hardware_readiness(
            RuntimeBackendMode::HardwareMining,
            Model::S19jPro,
            &profile(BoardFamily::Xilinx),
            SupportLevel::MvpStable,
            &safety,
        );

        assert_eq!(report.state, HardwareReadinessState::MiningReady);
    }

    #[test]
    fn hardware_mining_needs_identity_stays_read_only_needs_identity() {
        let identity = identity(
            RuntimeBackendMode::HardwareMining,
            HardwareIdentityState::ConfiguredOnly,
            HardwareIdentityConfidence::Low,
            None,
            None,
        );
        let safety = evaluate_hardware_safety(
            RuntimeBackendMode::HardwareMining,
            SupportLevel::MvpStable,
            &identity,
        );
        let report = evaluate_hardware_readiness(
            RuntimeBackendMode::HardwareMining,
            Model::S19jPro,
            &profile(BoardFamily::Xilinx),
            SupportLevel::MvpStable,
            &safety,
        );

        assert_eq!(report.state, HardwareReadinessState::ReadOnlyNeedsIdentity);
    }

    #[test]
    fn unsupported_target_is_blocked() {
        let identity = HardwareIdentityReport::simulated(BoardFamily::Cvitek, Model::S19jPro);
        let safety = evaluate_hardware_safety(
            RuntimeBackendMode::Simulated,
            SupportLevel::Unsupported,
            &identity,
        );
        let report = evaluate_hardware_readiness(
            RuntimeBackendMode::Simulated,
            Model::S19jPro,
            &profile(BoardFamily::Cvitek),
            SupportLevel::Unsupported,
            &safety,
        );

        assert_eq!(report.state, HardwareReadinessState::Blocked);
        assert!(report.actions.iter().all(|action| !action.allowed));
    }
}
