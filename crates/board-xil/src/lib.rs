use openmineros_common::{BoardFamily, BoardProfile, Capability, CapabilitySet};

pub fn profile() -> BoardProfile {
    BoardProfile {
        family: BoardFamily::Xilinx,
        soc: "Zynq",
        recovery: "external microSD",
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
