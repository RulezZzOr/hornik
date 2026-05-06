use openmineros_common::{BoardFamily, BoardProfile, Capability, CapabilitySet};

pub fn profile() -> BoardProfile {
    BoardProfile {
        family: BoardFamily::BeagleBone,
        soc: "AM335x",
        recovery: "internal microSD",
        capabilities: CapabilitySet::from_flags([
            Capability::InstallSd,
            Capability::InstallCommander,
            Capability::UpdateAb,
            Capability::FanControl,
            Capability::ChainIsolation,
            Capability::SafeMode,
        ]),
    }
}
