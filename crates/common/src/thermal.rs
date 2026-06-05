//! Thermal safety policy for S19-class hashboards.
//!
//! This is the safety-critical decision layer that the project requires before
//! any aggressive mining: given per-chain board (PCB) and chip temperatures it
//! produces a fan duty and a [`ThermalAction`] (run / throttle / shut down).
//!
//! # Grounding
//!
//! The stock `bmminer` monitors both a PCB temperature (read from the hashboard
//! PIC over I2C) and a chip temperature (read from the BM1398 on-die sensor),
//! compares each against a maximum, watches the temperature *rise*, and counts
//! consecutive overtemp events before forcing an exit
//! (`over max temp, pcb temp .. (max ..), chip temp ..(max ..) .. total_exit_failure ..`).
//! This module mirrors that policy. The default limits are conservative S19
//! values; the actual sensor and fan I/O paths are hardware specific and are
//! wired separately (NEEDS-HW-CONFIRM), but the *policy* here is pure and tested.

use serde::{Deserialize, Serialize};

/// A per-chain temperature sample.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChainTemps {
    pub chain: u8,
    /// Board / PCB temperature in degrees Celsius.
    pub board_c: f64,
    /// Hottest chip temperature on the chain in degrees Celsius.
    pub chip_max_c: f64,
}

impl ChainTemps {
    /// Whether both readings look physically plausible (a failed sensor often
    /// reports 0 or a wild value, which must be treated as a fault, not "cool").
    pub fn is_plausible(&self) -> bool {
        (5.0..=130.0).contains(&self.board_c) && (5.0..=130.0).contains(&self.chip_max_c)
    }
}

/// Conservative thermal limits and fan band for an S19-class board.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThermalLimits {
    /// Chip temperature that forces shutdown.
    pub chip_shutdown_c: f64,
    /// Chip temperature at which the fan should already be at maximum.
    pub chip_target_c: f64,
    /// Board temperature that forces shutdown.
    pub board_shutdown_c: f64,
    /// Board temperature at which the fan should already be at maximum.
    pub board_target_c: f64,
    /// Fan floor (never spin below this while mining), percent.
    pub min_fan_percent: u8,
    /// Fan ceiling, percent.
    pub max_fan_percent: u8,
    /// Consecutive overtemp samples tolerated before shutdown (rest = throttle).
    pub overtemp_grace_samples: u32,
}

impl Default for ThermalLimits {
    fn default() -> Self {
        Self {
            chip_shutdown_c: 105.0,
            chip_target_c: 85.0,
            board_shutdown_c: 85.0,
            board_target_c: 70.0,
            min_fan_percent: 30,
            max_fan_percent: 100,
            overtemp_grace_samples: 3,
        }
    }
}

/// What the runtime should do given the current temperatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThermalAction {
    /// Temperatures are within band; keep mining.
    Run,
    /// Hot or a transient overtemp: keep cooling at full fan but do not yet stop.
    Throttle,
    /// Persistent overtemp or a sensor fault: stop the ASICs.
    Shutdown,
}

/// The decision produced from a temperature sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThermalDecision {
    pub action: ThermalAction,
    pub fan_percent: u8,
    /// Hottest chip across all chains, if any plausible reading was seen.
    pub hottest_chip_c: Option<f64>,
    /// Hottest board across all chains, if any plausible reading was seen.
    pub hottest_board_c: Option<f64>,
    pub reasons: Vec<String>,
}

/// Linear fan duty for a temperature within `[target_floor, target_ceiling]`,
/// clamped to `[min, max]`. Below the floor it returns `min`, at/above the
/// ceiling it returns `max`.
fn ramp(value: f64, floor: f64, ceiling: f64, min: u8, max: u8) -> u8 {
    if value <= floor {
        return min;
    }
    if value >= ceiling || ceiling <= floor {
        return max;
    }
    let span = ceiling - floor;
    let frac = (value - floor) / span;
    let range = f64::from(max - min);
    min + (frac * range).round() as u8
}

/// Stateful thermal monitor: it tracks consecutive overtemp samples so a single
/// noisy reading throttles rather than shuts the miner down.
#[derive(Debug, Clone)]
pub struct ThermalMonitor {
    limits: ThermalLimits,
    consecutive_overtemp: u32,
}

impl ThermalMonitor {
    pub fn new(limits: ThermalLimits) -> Self {
        Self {
            limits,
            consecutive_overtemp: 0,
        }
    }

    pub fn limits(&self) -> ThermalLimits {
        self.limits
    }

    /// Consecutive overtemp samples seen so far (resets when back in band).
    pub fn consecutive_overtemp(&self) -> u32 {
        self.consecutive_overtemp
    }

    /// Evaluate a temperature sample and decide what to do.
    pub fn evaluate(&mut self, chains: &[ChainTemps]) -> ThermalDecision {
        let limits = self.limits;

        // A missing or implausible sensor is a fault: fail safe to full fan and
        // shutdown rather than trust a "cool" zero.
        if chains.is_empty() || chains.iter().any(|chain| !chain.is_plausible()) {
            self.consecutive_overtemp = self.consecutive_overtemp.saturating_add(1);
            return ThermalDecision {
                action: ThermalAction::Shutdown,
                fan_percent: limits.max_fan_percent,
                hottest_chip_c: None,
                hottest_board_c: None,
                reasons: vec![
                    "temperature sensor reading missing or implausible; failing safe".to_string(),
                ],
            };
        }

        let hottest_chip = chains
            .iter()
            .map(|chain| chain.chip_max_c)
            .fold(f64::MIN, f64::max);
        let hottest_board = chains
            .iter()
            .map(|chain| chain.board_c)
            .fold(f64::MIN, f64::max);

        let fan_from_chip = ramp(
            hottest_chip,
            limits.chip_target_c - 20.0,
            limits.chip_target_c,
            limits.min_fan_percent,
            limits.max_fan_percent,
        );
        let fan_from_board = ramp(
            hottest_board,
            limits.board_target_c - 20.0,
            limits.board_target_c,
            limits.min_fan_percent,
            limits.max_fan_percent,
        );
        let fan_percent = fan_from_chip.max(fan_from_board);

        let chip_over = hottest_chip >= limits.chip_shutdown_c;
        let board_over = hottest_board >= limits.board_shutdown_c;

        if chip_over || board_over {
            self.consecutive_overtemp = self.consecutive_overtemp.saturating_add(1);
            let mut reasons = Vec::new();
            if chip_over {
                reasons.push(format!(
                    "chip temp {hottest_chip:.1}C >= shutdown {:.1}C",
                    limits.chip_shutdown_c
                ));
            }
            if board_over {
                reasons.push(format!(
                    "board temp {hottest_board:.1}C >= shutdown {:.1}C",
                    limits.board_shutdown_c
                ));
            }
            let action = if self.consecutive_overtemp > limits.overtemp_grace_samples {
                reasons.push(format!(
                    "overtemp persisted {} samples (> grace {})",
                    self.consecutive_overtemp, limits.overtemp_grace_samples
                ));
                ThermalAction::Shutdown
            } else {
                ThermalAction::Throttle
            };
            return ThermalDecision {
                action,
                fan_percent: limits.max_fan_percent,
                hottest_chip_c: Some(hottest_chip),
                hottest_board_c: Some(hottest_board),
                reasons,
            };
        }

        // Back in band.
        self.consecutive_overtemp = 0;
        let action = if hottest_chip >= limits.chip_target_c
            || hottest_board >= limits.board_target_c
        {
            ThermalAction::Throttle
        } else {
            ThermalAction::Run
        };
        ThermalDecision {
            action,
            fan_percent,
            hottest_chip_c: Some(hottest_chip),
            hottest_board_c: Some(hottest_board),
            reasons: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temps(board: f64, chip: f64) -> Vec<ChainTemps> {
        vec![ChainTemps {
            chain: 0,
            board_c: board,
            chip_max_c: chip,
        }]
    }

    #[test]
    fn cool_board_runs_at_min_fan() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        let decision = monitor.evaluate(&temps(45.0, 55.0));
        assert_eq!(decision.action, ThermalAction::Run);
        assert_eq!(decision.fan_percent, 30);
    }

    #[test]
    fn warm_chip_ramps_fan_and_throttles_near_target() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        let decision = monitor.evaluate(&temps(60.0, 90.0));
        // Above chip target (85) but below shutdown (105): throttle, full fan.
        assert_eq!(decision.action, ThermalAction::Throttle);
        assert_eq!(decision.fan_percent, 100);
    }

    #[test]
    fn single_overtemp_throttles_but_does_not_shut_down() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        let decision = monitor.evaluate(&temps(60.0, 110.0));
        assert_eq!(decision.action, ThermalAction::Throttle);
        assert_eq!(decision.fan_percent, 100);
        assert_eq!(monitor.consecutive_overtemp(), 1);
    }

    #[test]
    fn persistent_overtemp_forces_shutdown_after_grace() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        // grace = 3, so the 4th consecutive overtemp shuts down.
        for _ in 0..3 {
            assert_eq!(monitor.evaluate(&temps(60.0, 110.0)).action, ThermalAction::Throttle);
        }
        assert_eq!(monitor.evaluate(&temps(60.0, 110.0)).action, ThermalAction::Shutdown);
    }

    #[test]
    fn returning_to_band_resets_the_overtemp_counter() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        monitor.evaluate(&temps(60.0, 110.0));
        monitor.evaluate(&temps(60.0, 110.0));
        assert_eq!(monitor.consecutive_overtemp(), 2);
        monitor.evaluate(&temps(50.0, 70.0));
        assert_eq!(monitor.consecutive_overtemp(), 0);
    }

    #[test]
    fn implausible_sensor_fails_safe_to_shutdown_and_full_fan() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        let decision = monitor.evaluate(&temps(0.0, 0.0));
        assert_eq!(decision.action, ThermalAction::Shutdown);
        assert_eq!(decision.fan_percent, 100);
        assert!(decision.hottest_chip_c.is_none());
    }

    #[test]
    fn empty_sample_fails_safe() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        assert_eq!(monitor.evaluate(&[]).action, ThermalAction::Shutdown);
    }

    #[test]
    fn overtemp_on_board_alone_also_acts() {
        let mut monitor = ThermalMonitor::new(ThermalLimits::default());
        let decision = monitor.evaluate(&temps(90.0, 70.0));
        assert_eq!(decision.action, ThermalAction::Throttle);
        assert!(decision.reasons.iter().any(|reason| reason.contains("board temp")));
    }

    #[test]
    fn ramp_is_monotonic_and_clamped() {
        assert_eq!(ramp(10.0, 65.0, 85.0, 30, 100), 30);
        assert_eq!(ramp(85.0, 65.0, 85.0, 30, 100), 100);
        assert_eq!(ramp(200.0, 65.0, 85.0, 30, 100), 100);
        let mid = ramp(75.0, 65.0, 85.0, 30, 100);
        assert!((30..=100).contains(&mid));
    }
}
