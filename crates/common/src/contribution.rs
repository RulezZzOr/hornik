use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTRIBUTION_BENEFICIARY: &str = "bc1qp6d4vxmenug97ghcy027vsn3902yadcj77ka6j";
pub const MAX_CONTRIBUTION_RATE_PERCENT: f64 = 3.0;
pub const CONTRIBUTION_TARGET_LOCKED: bool = true;
pub const MUTABLE_CONTRIBUTION_FIELDS: [&str; 2] = ["enabled", "rate_percent"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributionEndpoint {
    pub url: String,
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ContributionConfig {
    pub enabled: bool,
    pub rate_percent: f64,
}

impl Default for ContributionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rate_percent: 0.0,
        }
    }
}

impl ContributionConfig {
    pub fn scheduled_seconds_per_day(self) -> u32 {
        if !self.enabled {
            return 0;
        }

        let clamped = self.rate_percent.clamp(0.0, MAX_CONTRIBUTION_RATE_PERCENT);
        ((24.0 * 60.0 * 60.0) * (clamped / 100.0)).round() as u32
    }
}

pub fn default_contribution_endpoints() -> Vec<ContributionEndpoint> {
    [
        "stratum+tcp://ss.antpool.com:3333",
        "stratum+tcp://ss.antpool.com:443",
        "stratum+tcp://ss.antpool.com:25",
    ]
    .into_iter()
    .map(|url| ContributionEndpoint {
        url: url.to_string(),
        user: format!("{DEFAULT_CONTRIBUTION_BENEFICIARY}.openmineros"),
        password: "x".to_string(),
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributionWindow {
    pub start_second_of_day: u32,
    pub duration_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContributionStatus {
    pub enabled: bool,
    pub rate_percent: f64,
    pub max_rate_percent: f64,
    pub target_locked: bool,
    pub mutable_fields: Vec<String>,
    pub beneficiary: String,
    pub endpoints: Vec<ContributionEndpoint>,
    pub today_scheduled_seconds: u32,
    pub today_executed_seconds: u32,
    pub current_window: Option<ContributionWindow>,
}

impl From<ContributionConfig> for ContributionStatus {
    fn from(config: ContributionConfig) -> Self {
        Self {
            enabled: config.enabled,
            rate_percent: config
                .rate_percent
                .clamp(0.0, MAX_CONTRIBUTION_RATE_PERCENT),
            max_rate_percent: MAX_CONTRIBUTION_RATE_PERCENT,
            target_locked: CONTRIBUTION_TARGET_LOCKED,
            mutable_fields: MUTABLE_CONTRIBUTION_FIELDS
                .into_iter()
                .map(str::to_string)
                .collect(),
            beneficiary: DEFAULT_CONTRIBUTION_BENEFICIARY.to_string(),
            endpoints: default_contribution_endpoints(),
            today_scheduled_seconds: config.scheduled_seconds_per_day(),
            today_executed_seconds: 0,
            current_window: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribution_defaults_to_zero() {
        let status = ContributionStatus::from(ContributionConfig::default());
        assert!(!status.enabled);
        assert_eq!(status.rate_percent, 0.0);
        assert_eq!(status.max_rate_percent, 3.0);
        assert!(status.target_locked);
        assert_eq!(status.mutable_fields, ["enabled", "rate_percent"]);
        assert_eq!(status.beneficiary, DEFAULT_CONTRIBUTION_BENEFICIARY);
        assert_eq!(status.today_scheduled_seconds, 0);
    }

    #[test]
    fn one_percent_maps_to_864_seconds_per_day() {
        let config = ContributionConfig {
            enabled: true,
            rate_percent: 1.0,
        };

        assert_eq!(config.scheduled_seconds_per_day(), 864);
    }

    #[test]
    fn contribution_rate_is_capped_at_three_percent() {
        let config = ContributionConfig {
            enabled: true,
            rate_percent: 10.0,
        };

        assert_eq!(config.scheduled_seconds_per_day(), 2592);
        assert_eq!(ContributionStatus::from(config).rate_percent, 3.0);
    }

    #[test]
    fn default_endpoints_are_public_and_auditable() {
        let endpoints = default_contribution_endpoints();

        assert_eq!(endpoints.len(), 3);
        assert!(
            endpoints
                .iter()
                .all(|endpoint| endpoint.user.starts_with(DEFAULT_CONTRIBUTION_BENEFICIARY))
        );
    }

    #[test]
    fn official_build_only_allows_rate_and_enabled_to_change() {
        let status = ContributionStatus::from(ContributionConfig {
            enabled: true,
            rate_percent: 2.5,
        });

        assert!(status.target_locked);
        assert_eq!(status.mutable_fields, vec!["enabled", "rate_percent"]);
        assert_eq!(status.beneficiary, DEFAULT_CONTRIBUTION_BENEFICIARY);
        assert_eq!(status.endpoints, default_contribution_endpoints());
    }
}
