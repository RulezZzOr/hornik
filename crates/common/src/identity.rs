use crate::{BoardFamily, Model, RuntimeBackendMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareIdentityState {
    ConfiguredOnly,
    Inferred,
    Conflict,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareIdentityConfidence {
    None,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareIdentityEvidence {
    pub source: String,
    pub key: String,
    pub value: String,
    pub matched_board: Option<BoardFamily>,
    pub matched_model: Option<Model>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareIdentityObservation {
    pub source: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareIdentityReport {
    pub schema_version: u8,
    pub backend: RuntimeBackendMode,
    pub safe_read_only: bool,
    pub state: HardwareIdentityState,
    pub confidence: HardwareIdentityConfidence,
    pub configured_board: BoardFamily,
    pub configured_model: Model,
    pub detected_board: Option<BoardFamily>,
    pub detected_model: Option<Model>,
    pub evidence: Vec<HardwareIdentityEvidence>,
    pub notes: Vec<String>,
}

impl HardwareIdentityReport {
    pub const SCHEMA_VERSION: u8 = 1;

    pub fn simulated(configured_board: BoardFamily, configured_model: Model) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            backend: RuntimeBackendMode::Simulated,
            safe_read_only: true,
            state: HardwareIdentityState::ConfiguredOnly,
            confidence: HardwareIdentityConfidence::Low,
            configured_board,
            configured_model,
            detected_board: None,
            detected_model: None,
            evidence: Vec::new(),
            notes: vec![
                "simulated backend does not inspect host hardware".to_string(),
                "configured board/model are used as the identity source".to_string(),
            ],
        }
    }
}

pub fn infer_hardware_identity(
    backend: RuntimeBackendMode,
    configured_board: BoardFamily,
    configured_model: Model,
    observations: impl IntoIterator<Item = HardwareIdentityObservation>,
) -> HardwareIdentityReport {
    let mut evidence = observations
        .into_iter()
        .map(|observation| evidence_from_observation(&observation))
        .collect::<Vec<_>>();
    evidence.retain(|item| item.matched_board.is_some() || item.matched_model.is_some());

    let detected_board = single_match(evidence.iter().filter_map(|item| item.matched_board));
    let detected_model = single_match(evidence.iter().filter_map(|item| item.matched_model));
    let board_conflict = detected_board.is_some_and(|board| board != configured_board);
    let model_conflict = detected_model.is_some_and(|model| model != configured_model);
    let state = match (
        detected_board,
        detected_model,
        board_conflict || model_conflict,
    ) {
        (_, _, true) => HardwareIdentityState::Conflict,
        (Some(_), _, false) | (_, Some(_), false) => HardwareIdentityState::Inferred,
        (None, None, false) if backend == RuntimeBackendMode::Simulated => {
            HardwareIdentityState::ConfiguredOnly
        }
        (None, None, false) => HardwareIdentityState::Unknown,
    };
    let confidence = match state {
        HardwareIdentityState::Conflict => HardwareIdentityConfidence::Medium,
        HardwareIdentityState::Inferred if detected_board.is_some() && detected_model.is_some() => {
            HardwareIdentityConfidence::High
        }
        HardwareIdentityState::Inferred => HardwareIdentityConfidence::Medium,
        HardwareIdentityState::ConfiguredOnly => HardwareIdentityConfidence::Low,
        HardwareIdentityState::Unknown => HardwareIdentityConfidence::None,
    };

    HardwareIdentityReport {
        schema_version: HardwareIdentityReport::SCHEMA_VERSION,
        backend,
        safe_read_only: true,
        state,
        confidence,
        configured_board,
        configured_model,
        detected_board,
        detected_model,
        evidence,
        notes: identity_notes(state),
    }
}

fn evidence_from_observation(
    observation: &HardwareIdentityObservation,
) -> HardwareIdentityEvidence {
    let value = observation.value.to_ascii_lowercase();
    let matched_board = detect_board(&value);
    let matched_model = detect_model(&value);
    let detail = match (matched_board, matched_model) {
        (Some(board), Some(model)) => format!("matched {board} and {model}"),
        (Some(board), None) => format!("matched {board}"),
        (None, Some(model)) => format!("matched {model}"),
        (None, None) => "no supported S19 identity match".to_string(),
    };

    HardwareIdentityEvidence {
        source: observation.source.clone(),
        key: observation.key.clone(),
        value: observation.value.clone(),
        matched_board,
        matched_model,
        detail,
    }
}

fn detect_board(value: &str) -> Option<BoardFamily> {
    if contains_any(value, &["xilinx", "zynq", "zc702", "xc7z"]) {
        Some(BoardFamily::Xilinx)
    } else if contains_any(value, &["beaglebone", "am335", "am33xx", "ti am335"]) {
        Some(BoardFamily::BeagleBone)
    } else if contains_any(value, &["amlogic", "meson", "gxl", "g12"]) {
        Some(BoardFamily::Amlogic)
    } else if contains_any(value, &["cvitek", "cv1835", "cvitek cv"]) {
        Some(BoardFamily::Cvitek)
    } else {
        None
    }
}

fn detect_model(value: &str) -> Option<Model> {
    if contains_any(value, &["s19j pro", "s19j-pro", "s19jpro"]) {
        Some(Model::S19jPro)
    } else if contains_any(value, &["s19 xp", "s19-xp", "s19xp"]) {
        Some(Model::S19Xp)
    } else if contains_any(value, &["s19 pro", "s19-pro", "s19pro"]) {
        Some(Model::S19Pro)
    } else if contains_any(value, &["s19j"]) {
        Some(Model::S19j)
    } else if contains_any(value, &["t19"]) {
        Some(Model::T19)
    } else if contains_any(value, &["s19"]) {
        Some(Model::S19)
    } else {
        None
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn single_match<T: Copy + Eq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    if values.all(|value| value == first) {
        Some(first)
    } else {
        None
    }
}

fn identity_notes(state: HardwareIdentityState) -> Vec<String> {
    match state {
        HardwareIdentityState::ConfiguredOnly => vec![
            "hardware identity is using configured board/model only".to_string(),
            "automatic target switching is disabled in build 0.1.0".to_string(),
        ],
        HardwareIdentityState::Inferred => vec![
            "hardware identity was inferred from read-only evidence".to_string(),
            "automatic target switching is disabled in build 0.1.0".to_string(),
        ],
        HardwareIdentityState::Conflict => vec![
            "detected identity conflicts with configured board/model".to_string(),
            "the configured target remains active; review before any install or mining action"
                .to_string(),
        ],
        HardwareIdentityState::Unknown => vec![
            "no supported S19 identity evidence was detected".to_string(),
            "the configured target remains active and hardware actions stay disabled".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_xilinx_s19j_pro_from_read_only_observations() {
        let report = infer_hardware_identity(
            RuntimeBackendMode::HardwareProbe,
            BoardFamily::Xilinx,
            Model::S19jPro,
            [
                observation("device-tree", "model", "Antminer S19j Pro Xilinx Zynq"),
                observation("stock-api", "miner_type", "S19j Pro"),
            ],
        );

        assert_eq!(report.state, HardwareIdentityState::Inferred);
        assert_eq!(report.confidence, HardwareIdentityConfidence::High);
        assert_eq!(report.detected_board, Some(BoardFamily::Xilinx));
        assert_eq!(report.detected_model, Some(Model::S19jPro));
    }

    #[test]
    fn flags_conflict_without_changing_configured_target() {
        let report = infer_hardware_identity(
            RuntimeBackendMode::HardwareProbe,
            BoardFamily::Xilinx,
            Model::S19jPro,
            [observation(
                "device-tree",
                "model",
                "BeagleBone Black AM335x",
            )],
        );

        assert_eq!(report.state, HardwareIdentityState::Conflict);
        assert_eq!(report.configured_board, BoardFamily::Xilinx);
        assert_eq!(report.detected_board, Some(BoardFamily::BeagleBone));
    }

    #[test]
    fn unknown_when_probe_has_no_s19_identity() {
        let report = infer_hardware_identity(
            RuntimeBackendMode::HardwareProbe,
            BoardFamily::Amlogic,
            Model::S19jPro,
            [observation("device-tree", "model", "generic linux host")],
        );

        assert_eq!(report.state, HardwareIdentityState::Unknown);
        assert_eq!(report.confidence, HardwareIdentityConfidence::None);
        assert_eq!(report.detected_board, None);
    }

    #[test]
    fn simulated_report_is_configured_only() {
        let report = HardwareIdentityReport::simulated(BoardFamily::Amlogic, Model::S19Xp);

        assert_eq!(report.state, HardwareIdentityState::ConfiguredOnly);
        assert_eq!(report.configured_board, BoardFamily::Amlogic);
        assert_eq!(report.configured_model, Model::S19Xp);
        assert_eq!(report.detected_board, None);
    }

    fn observation(source: &str, key: &str, value: &str) -> HardwareIdentityObservation {
        HardwareIdentityObservation {
            source: source.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }
}
