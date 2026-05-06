use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoardFamily {
    Xilinx,
    BeagleBone,
    Amlogic,
    Cvitek,
}

impl BoardFamily {
    pub const fn board_id(self) -> &'static str {
        match self {
            Self::Xilinx => "s19-xil",
            Self::BeagleBone => "s19-bb",
            Self::Amlogic => "s19-aml",
            Self::Cvitek => "s19-cvitek",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Xilinx => "Xilinx / Zynq",
            Self::BeagleBone => "BeagleBone Black",
            Self::Amlogic => "Amlogic",
            Self::Cvitek => "CVitek",
        }
    }
}

impl fmt::Display for BoardFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.board_id())
    }
}

impl FromStr for BoardFamily {
    type Err = TargetError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "s19-xil" | "xil" | "xilinx" | "zynq" => Ok(Self::Xilinx),
            "s19-bb" | "bb" | "bbb" | "beaglebone" | "beaglebone-black" => Ok(Self::BeagleBone),
            "s19-aml" | "aml" | "amlogic" => Ok(Self::Amlogic),
            "s19-cvitek" | "cvitek" | "cv1835" => Ok(Self::Cvitek),
            other => Err(TargetError::UnknownBoard(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Model {
    S19,
    S19Pro,
    S19j,
    S19jPro,
    S19Xp,
    T19,
}

impl Model {
    pub const fn model_id(self) -> &'static str {
        match self {
            Self::S19 => "s19",
            Self::S19Pro => "s19-pro",
            Self::S19j => "s19j",
            Self::S19jPro => "s19j-pro",
            Self::S19Xp => "s19-xp",
            Self::T19 => "t19",
        }
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.model_id())
    }
}

impl FromStr for Model {
    type Err = TargetError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "s19" => Ok(Self::S19),
            "s19-pro" | "s19pro" => Ok(Self::S19Pro),
            "s19j" => Ok(Self::S19j),
            "s19j-pro" | "s19jpro" | "jpro" => Ok(Self::S19jPro),
            "s19-xp" | "s19xp" => Ok(Self::S19Xp),
            "t19" => Ok(Self::T19),
            other => Err(TargetError::UnknownModel(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    InstallSd,
    InstallOtg,
    InstallCommander,
    UpdateAb,
    TelemetryPowerNative,
    FanControl,
    ChainIsolation,
    SafeMode,
}

impl Capability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InstallSd => "install.sd",
            Self::InstallOtg => "install.otg",
            Self::InstallCommander => "install.commander",
            Self::UpdateAb => "update.ab",
            Self::TelemetryPowerNative => "telemetry.power_native",
            Self::FanControl => "fan.control",
            Self::ChainIsolation => "chain.isolation",
            Self::SafeMode => "safe_mode",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub flags: Vec<String>,
}

impl CapabilitySet {
    pub fn from_flags(flags: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            flags: flags
                .into_iter()
                .map(Capability::as_str)
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn has(&self, capability: Capability) -> bool {
        self.flags.iter().any(|flag| flag == capability.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SupportLevel {
    MvpStable,
    Experimental,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardProfile {
    pub family: BoardFamily,
    pub soc: &'static str,
    pub recovery: &'static str,
    pub capabilities: CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportedTarget {
    pub model: Model,
    pub board: BoardFamily,
    pub support: SupportLevel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardCatalogEntry {
    pub family: BoardFamily,
    pub board_id: String,
    pub display_name: String,
    pub soc: String,
    pub recovery: String,
    pub capabilities: CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCatalog {
    pub schema_version: u8,
    pub boards: Vec<BoardCatalogEntry>,
    pub targets: Vec<SupportedTarget>,
    pub notes: Vec<String>,
}

impl TargetCatalog {
    pub const SCHEMA_VERSION: u8 = 1;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TargetError {
    #[error("unknown board family: {0}")]
    UnknownBoard(String),
    #[error("unknown model: {0}")]
    UnknownModel(String),
    #[error("unsupported target: {model}/{board}")]
    UnsupportedTarget { model: Model, board: BoardFamily },
}

pub fn supported_targets() -> Vec<SupportedTarget> {
    let supported_boards = [
        BoardFamily::Xilinx,
        BoardFamily::BeagleBone,
        BoardFamily::Amlogic,
    ];
    let all_boards = [
        BoardFamily::Xilinx,
        BoardFamily::BeagleBone,
        BoardFamily::Amlogic,
        BoardFamily::Cvitek,
    ];
    let all_models = [
        Model::S19,
        Model::S19Pro,
        Model::S19j,
        Model::S19jPro,
        Model::S19Xp,
        Model::T19,
    ];
    let mut targets = Vec::new();

    for model in all_models {
        for board in all_boards {
            let support = if model == Model::S19jPro && supported_boards.contains(&board) {
                SupportLevel::MvpStable
            } else if supported_boards.contains(&board) {
                SupportLevel::Experimental
            } else {
                SupportLevel::Unsupported
            };
            targets.push(SupportedTarget {
                model,
                board,
                support,
            });
        }
    }

    targets
}

pub fn target_catalog() -> TargetCatalog {
    TargetCatalog {
        schema_version: TargetCatalog::SCHEMA_VERSION,
        boards: board_catalog(),
        targets: supported_targets(),
        notes: vec![
            "build 0.1.0 treats S19j Pro on Xilinx, BeagleBone Black, and Amlogic as MVP-stable targets".to_string(),
            "other S19-class Xilinx, BeagleBone Black, and Amlogic targets are exposed as experimental until tested on hardware".to_string(),
            "CVitek is listed for operator identification but is not supported by build 0.1.0".to_string(),
        ],
    }
}

pub fn board_catalog() -> Vec<BoardCatalogEntry> {
    [
        (
            BoardFamily::Xilinx,
            "Zynq",
            "external microSD",
            CapabilitySet::from_flags([
                Capability::InstallSd,
                Capability::InstallCommander,
                Capability::UpdateAb,
                Capability::FanControl,
                Capability::ChainIsolation,
                Capability::SafeMode,
            ]),
        ),
        (
            BoardFamily::BeagleBone,
            "AM335x",
            "internal microSD",
            CapabilitySet::from_flags([
                Capability::InstallSd,
                Capability::InstallCommander,
                Capability::UpdateAb,
                Capability::FanControl,
                Capability::ChainIsolation,
                Capability::SafeMode,
            ]),
        ),
        (
            BoardFamily::Amlogic,
            "A113D",
            "micro-USB OTG",
            CapabilitySet::from_flags([
                Capability::InstallOtg,
                Capability::InstallCommander,
                Capability::UpdateAb,
                Capability::FanControl,
                Capability::ChainIsolation,
                Capability::SafeMode,
            ]),
        ),
        (
            BoardFamily::Cvitek,
            "CV1835",
            "unsupported in 0.1.0",
            CapabilitySet::from_flags([Capability::SafeMode]),
        ),
    ]
    .into_iter()
    .map(|(family, soc, recovery, capabilities)| BoardCatalogEntry {
        family,
        board_id: family.board_id().to_string(),
        display_name: family.display_name().to_string(),
        soc: soc.to_string(),
        recovery: recovery.to_string(),
        capabilities,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_board_aliases() {
        assert_eq!("xil".parse::<BoardFamily>(), Ok(BoardFamily::Xilinx));
        assert_eq!("bb".parse::<BoardFamily>(), Ok(BoardFamily::BeagleBone));
        assert_eq!("aml".parse::<BoardFamily>(), Ok(BoardFamily::Amlogic));
    }

    #[test]
    fn includes_all_mvp_jpro_boards() {
        let stable: Vec<_> = supported_targets()
            .into_iter()
            .filter(|target| target.model == Model::S19jPro)
            .filter(|target| target.support == SupportLevel::MvpStable)
            .map(|target| target.board)
            .collect();

        assert_eq!(
            stable,
            vec![
                BoardFamily::Xilinx,
                BoardFamily::BeagleBone,
                BoardFamily::Amlogic
            ]
        );
    }

    #[test]
    fn target_catalog_includes_all_model_board_pairs() {
        let catalog = target_catalog();

        assert_eq!(catalog.schema_version, TargetCatalog::SCHEMA_VERSION);
        assert_eq!(catalog.boards.len(), 4);
        assert_eq!(catalog.targets.len(), 24);
        assert!(catalog.targets.iter().any(|target| {
            target.model == Model::S19jPro
                && target.board == BoardFamily::Cvitek
                && target.support == SupportLevel::Unsupported
        }));
        assert!(catalog.targets.iter().any(|target| {
            target.model == Model::S19Xp
                && target.board == BoardFamily::Xilinx
                && target.support == SupportLevel::Experimental
        }));
    }
}
