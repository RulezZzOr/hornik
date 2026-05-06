use openmineros_common::{BoardFamily, BoardProfile, Capability, CapabilitySet};

pub fn profile() -> BoardProfile {
    BoardProfile {
        family: BoardFamily::Amlogic,
        soc: "A113D",
        recovery: "micro-USB OTG",
        capabilities: CapabilitySet::from_flags([
            Capability::InstallOtg,
            Capability::InstallCommander,
            Capability::UpdateAb,
            Capability::FanControl,
            Capability::ChainIsolation,
            Capability::SafeMode,
        ]),
    }
}
