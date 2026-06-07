use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotState {
    Active,
    Inactive,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotInfo {
    pub name: String,
    pub state: SlotState,
    pub bootable: bool,
    pub version: Option<String>,
    pub last_boot_successful: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub update_model: String,
    pub active_slot: String,
    pub inactive_slot: String,
    pub rollback_available: bool,
    pub boot_once_pending: bool,
    pub slots: Vec<SlotInfo>,
    pub notes: Vec<String>,
}

impl UpdateStatus {
    pub fn development_default(active_slot: impl Into<String>, version: impl Into<String>) -> Self {
        let active_slot = active_slot.into();
        let inactive_slot = match active_slot.as_str() {
            "slot_a" => "slot_b",
            "slot_b" => "slot_a",
            _ => "slot_b",
        }
        .to_string();
        let version = version.into();

        Self {
            update_model: "a_b".to_string(),
            active_slot: active_slot.clone(),
            inactive_slot: inactive_slot.clone(),
            rollback_available: true,
            boot_once_pending: false,
            slots: vec![
                SlotInfo {
                    name: active_slot,
                    state: SlotState::Active,
                    bootable: true,
                    version: Some(version),
                    last_boot_successful: Some(true),
                },
                SlotInfo {
                    name: inactive_slot,
                    state: SlotState::Inactive,
                    bootable: false,
                    version: None,
                    last_boot_successful: None,
                },
            ],
            notes: vec![
                "build 0.1.0 exposes update status only; it does not write slots".to_string(),
                "flashable releases must pass manifest and signature verification".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_pairs_slot_a_with_slot_b() {
        let status = UpdateStatus::development_default("slot_a", "0.1.0");

        assert_eq!(status.active_slot, "slot_a");
        assert_eq!(status.inactive_slot, "slot_b");
        assert!(status.rollback_available);
        assert_eq!(status.slots.len(), 2);
        assert_eq!(status.slots[0].state, SlotState::Active);
        assert_eq!(status.slots[1].state, SlotState::Inactive);
    }
}
