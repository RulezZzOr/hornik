pub mod axi;
pub mod bm1398;
pub mod bm1398_driver;
pub mod work_builder;

use openmineros_common::{
    BoardFamily, BoardProfile, Capability, CapabilitySet, ChainStatus, HardwareIdentityObservation,
    HardwareIdentityReport, HardwareProbeReport, HealthStatus, MinerMode, MinerStatus, Model,
    ProbeCheck, ProbeStatus, RuntimeBackendMode, Severity, StratumJobTemplate,
    StratumShareCandidate, SupportLevel, TargetError, TuningPhase, TuningProtocolFrame,
    TuningProtocolSequenceSpec, infer_hardware_identity, supported_targets,
};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    env,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error(transparent)]
    Target(#[from] TargetError),
    #[error("asic dispatch is not available in backend mode {0}")]
    DispatchUnavailable(RuntimeBackendMode),
    #[error("failed to dispatch asic job to {path}: {source}")]
    DispatchIo {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to receive asic result from {path}: {source}")]
    ReceiveIo {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse asic result from {path}: {source}")]
    ReceiveFrame {
        path: String,
        #[source]
        source: AsicFrameError,
    },
    #[error("bm1398 fpga path unavailable: {reason}")]
    FpgaUnavailable { reason: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AsicFrameError {
    #[error("asic frame is empty")]
    Empty,
    #[error("asic frame has invalid prefix: {0}")]
    InvalidPrefix(String),
    #[error("asic frame is not a nonce report")]
    NotNonceReport,
    #[error("asic frame missing required field: {0}")]
    MissingField(&'static str),
    #[error("asic frame has invalid field {field}: {value}")]
    InvalidField { field: &'static str, value: String },
    #[error("asic frame checksum mismatch")]
    ChecksumMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsicNonceReport {
    pub board: BoardFamily,
    pub job_id: String,
    pub chip_id: u16,
    pub extranonce2: String,
    pub ntime: String,
    pub nonce: String,
}

impl AsicNonceReport {
    pub fn into_share_candidate(self, worker: impl Into<String>) -> StratumShareCandidate {
        StratumShareCandidate {
            worker: worker.into(),
            job_id: self.job_id,
            extranonce2: self.extranonce2,
            ntime: self.ntime,
            nonce: self.nonce,
            version: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendRuntimeStatus {
    pub state: HealthStatus,
    pub severity: Severity,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum BackendHandle {
    Simulated(SimulatedBackend),
    HardwareProbe(HardwareProbeBackend),
    HardwareMining(HardwareMiningBackend),
}

pub trait AsicJobDispatcher {
    fn dispatch_job(&self, job: &StratumJobTemplate) -> Result<(), BackendError>;

    fn collect_share_candidates(
        &self,
        worker: &str,
        max_reports: usize,
    ) -> Result<Vec<StratumShareCandidate>, BackendError> {
        let _ = (worker, max_reports);
        Ok(Vec::new())
    }

    /// Dispatch a fully-built BM1398 work item. Default is a no-op so backends
    /// without a real ASIC path are unaffected.
    fn dispatch_work_item(&self, work: &bm1398::WorkItem) -> Result<(), BackendError> {
        let _ = work;
        Ok(())
    }

    /// Drain decoded nonce entries returned by the chain. Default is empty.
    fn collect_nonce_entries(
        &self,
        max_entries: usize,
    ) -> Result<Vec<bm1398::NonceEntry>, BackendError> {
        let _ = max_entries;
        Ok(Vec::new())
    }
}

pub fn build_tuning_sequence_frames(
    board: BoardFamily,
    chip_id: u16,
    phases: &[TuningPhase],
    base_frequency_mhz: u16,
    base_voltage_mv: u16,
    step_frequency_mhz: u16,
    step_voltage_mv: u16,
) -> Vec<Vec<u8>> {
    AsicProtocol::for_board(board).tuning_sequence(
        chip_id,
        phases,
        base_frequency_mhz,
        base_voltage_mv,
        step_frequency_mhz,
        step_voltage_mv,
    )
}

pub fn build_tuning_protocol_frames(spec: &TuningProtocolSequenceSpec) -> Vec<TuningProtocolFrame> {
    let protocol = AsicProtocol::for_board(spec.board_family);
    let mut frequency_mhz = spec.base_frequency_mhz;
    let mut voltage_mv = spec.base_voltage_mv;

    spec.phases
        .iter()
        .enumerate()
        .map(|(index, phase)| {
            let (command, frame) = match phase {
                TuningPhase::Baseline => (
                    "set_frequency",
                    protocol.set_frequency(spec.chip_id, frequency_mhz),
                ),
                TuningPhase::DownclockEfficiency => {
                    frequency_mhz = frequency_mhz.saturating_sub(spec.frequency_step_mhz);
                    (
                        "set_frequency",
                        protocol.set_frequency(spec.chip_id, frequency_mhz),
                    )
                }
                TuningPhase::UpclockStability => {
                    frequency_mhz = frequency_mhz.saturating_add(spec.frequency_step_mhz);
                    (
                        "set_frequency",
                        protocol.set_frequency(spec.chip_id, frequency_mhz),
                    )
                }
                TuningPhase::VoltageTrim => {
                    voltage_mv = voltage_mv.saturating_sub(spec.voltage_step_mv);
                    (
                        "set_voltage",
                        protocol.set_voltage(spec.chip_id, voltage_mv),
                    )
                }
            };

            TuningProtocolFrame {
                order: (index + 1) as u8,
                phase: *phase,
                command: command.to_string(),
                target_frequency_mhz: frequency_mhz,
                target_voltage_mv: voltage_mv,
                min_duration_seconds: spec.min_step_duration_seconds,
                frame: String::from_utf8(frame)
                    .expect("tuning frame must be valid utf-8")
                    .trim_end()
                    .to_string(),
            }
        })
        .collect()
}

pub fn build_nonce_report_frame(
    board: BoardFamily,
    job_id: &str,
    chip_id: u16,
    extranonce2: &str,
    ntime: &str,
    nonce: &str,
) -> Vec<u8> {
    AsicProtocol::for_board(board).nonce_report(job_id, chip_id, extranonce2, ntime, nonce)
}

pub fn parse_nonce_report_frame(frame: &str) -> Result<AsicNonceReport, AsicFrameError> {
    let frame = frame.trim();
    if frame.is_empty() {
        return Err(AsicFrameError::Empty);
    }

    let Some((body, checksum)) = frame.rsplit_once("|checksum=") else {
        return Err(AsicFrameError::MissingField("checksum"));
    };
    if checksum != checksum_hex(body.as_bytes()) {
        return Err(AsicFrameError::ChecksumMismatch);
    }

    let mut parts = body.split('|');
    let prefix = parts.next().ok_or(AsicFrameError::Empty)?;
    let board = match prefix {
        "omo-asic/xilinx/v1" => BoardFamily::Xilinx,
        "omo-asic/beaglebone/v1" => BoardFamily::BeagleBone,
        "omo-asic/amlogic/v1" => BoardFamily::Amlogic,
        "omo-asic/cvitek/v1" => BoardFamily::Cvitek,
        other => return Err(AsicFrameError::InvalidPrefix(other.to_string())),
    };

    let mut board_tag = None;
    let mut command = None;
    let mut job_id = None;
    let mut chip_id = None;
    let mut extranonce2 = None;
    let mut ntime = None;
    let mut nonce = None;

    for part in parts {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "board" => board_tag = Some(value.to_string()),
            "cmd" => command = Some(value.to_string()),
            "job_id" => job_id = Some(value.to_string()),
            "chip_id" => {
                chip_id = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| AsicFrameError::InvalidField {
                            field: "chip_id",
                            value: value.to_string(),
                        })?,
                )
            }
            "extranonce2" => extranonce2 = Some(value.to_string()),
            "ntime" => ntime = Some(value.to_string()),
            "nonce" => nonce = Some(value.to_string()),
            _ => {}
        }
    }

    let expected_board_tag = AsicProtocol::for_board(board).board_tag();
    if board_tag.as_deref() != Some(expected_board_tag) {
        return Err(AsicFrameError::InvalidField {
            field: "board",
            value: board_tag.unwrap_or_default(),
        });
    }
    if command.as_deref() != Some("nonce") {
        return Err(AsicFrameError::NotNonceReport);
    }

    let job_id = required_field(job_id, "job_id")?;
    let chip_id = chip_id.ok_or(AsicFrameError::MissingField("chip_id"))?;
    let extranonce2 = required_hex_field(extranonce2, "extranonce2", None)?;
    let ntime = required_hex_field(ntime, "ntime", Some(8))?;
    let nonce = required_hex_field(nonce, "nonce", Some(8))?;

    Ok(AsicNonceReport {
        board,
        job_id,
        chip_id,
        extranonce2,
        ntime,
        nonce,
    })
}

impl BackendHandle {
    pub fn new(
        mode: RuntimeBackendMode,
        model: Model,
        board: BoardFamily,
    ) -> Result<Self, BackendError> {
        match mode {
            RuntimeBackendMode::Simulated => {
                Ok(Self::Simulated(SimulatedBackend::new(model, board)?))
            }
            RuntimeBackendMode::HardwareProbe => Ok(Self::HardwareProbe(
                HardwareProbeBackend::new(model, board)?,
            )),
            RuntimeBackendMode::HardwareMining => Ok(Self::HardwareMining(
                HardwareMiningBackend::new(model, board)?,
            )),
        }
    }

    pub fn mode(&self) -> RuntimeBackendMode {
        match self {
            Self::Simulated(backend) => backend.mode(),
            Self::HardwareProbe(backend) => backend.mode(),
            Self::HardwareMining(backend) => backend.mode(),
        }
    }

    pub fn model(&self) -> Model {
        match self {
            Self::Simulated(backend) => backend.model(),
            Self::HardwareProbe(backend) => backend.model(),
            Self::HardwareMining(backend) => backend.model(),
        }
    }

    pub fn profile(&self) -> &BoardProfile {
        match self {
            Self::Simulated(backend) => backend.profile(),
            Self::HardwareProbe(backend) => backend.profile(),
            Self::HardwareMining(backend) => backend.profile(),
        }
    }

    pub fn support(&self) -> SupportLevel {
        match self {
            Self::Simulated(backend) => backend.support(),
            Self::HardwareProbe(backend) => backend.support(),
            Self::HardwareMining(backend) => backend.support(),
        }
    }

    pub fn runtime_status(&self) -> BackendRuntimeStatus {
        match self {
            Self::Simulated(backend) => backend.runtime_status(),
            Self::HardwareProbe(backend) => backend.runtime_status(),
            Self::HardwareMining(backend) => backend.runtime_status(),
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        match self {
            Self::Simulated(backend) => backend.miner_status(),
            Self::HardwareProbe(backend) => backend.miner_status(),
            Self::HardwareMining(backend) => backend.miner_status(),
        }
    }

    pub fn chain_statuses(&self) -> Vec<ChainStatus> {
        match self {
            Self::Simulated(backend) => backend.chain_statuses(),
            Self::HardwareProbe(backend) => backend.chain_statuses(),
            Self::HardwareMining(backend) => backend.chain_statuses(),
        }
    }

    pub fn probe_report(&self) -> HardwareProbeReport {
        match self {
            Self::Simulated(backend) => backend.probe_report(),
            Self::HardwareProbe(backend) => backend.probe_report(),
            Self::HardwareMining(backend) => backend.probe_report(),
        }
    }

    pub fn identity_report(&self) -> HardwareIdentityReport {
        match self {
            Self::Simulated(backend) => backend.identity_report(),
            Self::HardwareProbe(backend) => backend.identity_report(),
            Self::HardwareMining(backend) => backend.identity_report(),
        }
    }

    pub fn dispatch_job(&self, job: &StratumJobTemplate) -> Result<(), BackendError> {
        match self {
            Self::Simulated(_) => Ok(()),
            Self::HardwareProbe(_) => Err(BackendError::DispatchUnavailable(
                RuntimeBackendMode::HardwareProbe,
            )),
            Self::HardwareMining(backend) => backend.dispatch_job(job),
        }
    }

    pub fn collect_share_candidates(
        &self,
        worker: &str,
        max_reports: usize,
    ) -> Result<Vec<StratumShareCandidate>, BackendError> {
        match self {
            Self::HardwareMining(backend) => backend.collect_share_candidates(worker, max_reports),
            Self::Simulated(_) | Self::HardwareProbe(_) => Ok(Vec::new()),
        }
    }

    /// Dispatch a built BM1398 work item, routing to the gated FPGA path only on
    /// an armed hardware-mining Xilinx backend; otherwise a no-op.
    pub fn dispatch_work_item(&self, work: &bm1398::WorkItem) -> Result<(), BackendError> {
        match self {
            Self::HardwareMining(backend) if backend.fpga_armed() => backend.fpga_submit_work(work),
            _ => Ok(()),
        }
    }

    /// Drain decoded nonce entries from the gated FPGA path on an armed Xilinx
    /// hardware-mining backend; otherwise an empty vector.
    pub fn collect_nonce_entries(
        &self,
        max_entries: usize,
    ) -> Result<Vec<bm1398::NonceEntry>, BackendError> {
        match self {
            Self::HardwareMining(backend) if backend.fpga_armed() => {
                backend.fpga_poll_nonces(max_entries)
            }
            _ => Ok(Vec::new()),
        }
    }
}

pub fn build_tuning_execution_steps(
    spec: &TuningProtocolSequenceSpec,
) -> Vec<openmineros_common::TuningExecutionStep> {
    build_tuning_protocol_frames(spec)
        .into_iter()
        .map(|frame| openmineros_common::TuningExecutionStep {
            order: frame.order,
            phase: frame.phase,
            command: frame.command,
            target_frequency_mhz: frame.target_frequency_mhz,
            target_voltage_mv: frame.target_voltage_mv,
            min_duration_seconds: frame.min_duration_seconds,
        })
        .collect()
}

impl AsicJobDispatcher for BackendHandle {
    fn dispatch_job(&self, job: &StratumJobTemplate) -> Result<(), BackendError> {
        Self::dispatch_job(self, job)
    }

    fn collect_share_candidates(
        &self,
        worker: &str,
        max_reports: usize,
    ) -> Result<Vec<StratumShareCandidate>, BackendError> {
        Self::collect_share_candidates(self, worker, max_reports)
    }

    fn dispatch_work_item(&self, work: &bm1398::WorkItem) -> Result<(), BackendError> {
        Self::dispatch_work_item(self, work)
    }

    fn collect_nonce_entries(
        &self,
        max_entries: usize,
    ) -> Result<Vec<bm1398::NonceEntry>, BackendError> {
        Self::collect_nonce_entries(self, max_entries)
    }
}

#[derive(Debug, Clone)]
pub struct SimulatedBackend {
    model: Model,
    profile: BoardProfile,
    support: SupportLevel,
}

impl SimulatedBackend {
    pub fn new(model: Model, board: BoardFamily) -> Result<Self, BackendError> {
        let support = target_support(model, board)?;
        let profile = board_profile(board);

        Ok(Self {
            model,
            profile,
            support,
        })
    }

    pub fn mode(&self) -> RuntimeBackendMode {
        RuntimeBackendMode::Simulated
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn profile(&self) -> &BoardProfile {
        &self.profile
    }

    pub fn support(&self) -> SupportLevel {
        self.support
    }

    pub fn runtime_status(&self) -> BackendRuntimeStatus {
        BackendRuntimeStatus {
            state: HealthStatus::Mining,
            severity: Severity::Ok,
            issues: Vec::new(),
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        simulated_miner_status(self.model)
    }

    pub fn chain_statuses(&self) -> Vec<ChainStatus> {
        (0..3)
            .map(|id| ChainStatus {
                id,
                present: true,
                enabled: true,
                asic_detected: 42,
                temp_board_c: 62.0 + f64::from(id),
                temp_chip_max_c: 78.0 + f64::from(id),
                fault: None,
            })
            .collect()
    }

    pub fn probe_report(&self) -> HardwareProbeReport {
        HardwareProbeReport::new(
            self.mode(),
            self.model,
            self.profile.family,
            None,
            probe_plan(self.profile.family, ProbeMode::Skipped),
            vec![
                "simulated backend does not inspect host hardware".to_string(),
                "start with --backend hardware-probe for read-only board bring-up checks"
                    .to_string(),
            ],
        )
    }

    pub fn identity_report(&self) -> HardwareIdentityReport {
        HardwareIdentityReport::simulated(self.profile.family, self.model)
    }
}

#[derive(Debug, Clone)]
pub struct HardwareProbeBackend {
    model: Model,
    profile: BoardProfile,
    support: SupportLevel,
    probe_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct HardwareMiningBackend {
    model: Model,
    profile: BoardProfile,
    support: SupportLevel,
    uart_path: PathBuf,
    protocol: AsicProtocol,
    uart_transport: Arc<Mutex<Option<UartTransport>>>,
    /// Path to the real FPGA register device used by the BM1398 chain driver.
    fpga_device_path: PathBuf,
    /// Whether real FPGA writes are explicitly armed (opt-in, off by default).
    fpga_armed: bool,
    /// Lazily-opened real BM1398 chain driver (S19 XIL only).
    fpga_session: Arc<Mutex<Option<bm1398_driver::Bm1398Driver>>>,
}

#[derive(Debug)]
struct UartTransport {
    file: File,
    rx_buffer: Vec<u8>,
}

/// Result of bringing up a real BM1398 chain over the FPGA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainBringUp {
    /// FPGA hardware/version word read back from the board.
    pub fpga_version: u32,
    /// Number of chips the enumeration walk addressed.
    pub enumerated_chips: u16,
    /// Frequency actually realized by the PLL solver, in MHz.
    pub frequency_mhz: u16,
}

impl HardwareMiningBackend {
    pub fn new(model: Model, board: BoardFamily) -> Result<Self, BackendError> {
        Self::with_uart_path(model, board, asic_uart_path(board))
    }

    fn with_uart_path(
        model: Model,
        board: BoardFamily,
        uart_path: PathBuf,
    ) -> Result<Self, BackendError> {
        let fpga_device_path = env::var_os("OPENMINEROS_FPGA_DEV")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/dev/axi_fpga_dev"));
        let fpga_armed = env::var_os("OPENMINEROS_ALLOW_FPGA")
            .is_some_and(|value| value == "1" || value == "true");
        Self::with_config(model, board, uart_path, fpga_device_path, fpga_armed)
    }

    fn with_config(
        model: Model,
        board: BoardFamily,
        uart_path: PathBuf,
        fpga_device_path: PathBuf,
        fpga_armed: bool,
    ) -> Result<Self, BackendError> {
        let support = target_support(model, board)?;
        let profile = board_profile(board);

        Ok(Self {
            model,
            profile,
            support,
            uart_path,
            protocol: AsicProtocol::for_board(board),
            uart_transport: Arc::new(Mutex::new(None)),
            fpga_device_path,
            fpga_armed,
            fpga_session: Arc::new(Mutex::new(None)),
        })
    }

    pub fn mode(&self) -> RuntimeBackendMode {
        RuntimeBackendMode::HardwareMining
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn profile(&self) -> &BoardProfile {
        &self.profile
    }

    pub fn support(&self) -> SupportLevel {
        self.support
    }

    pub fn runtime_status(&self) -> BackendRuntimeStatus {
        let uart_exists = self.uart_path.exists();
        BackendRuntimeStatus {
            state: if uart_exists {
                HealthStatus::Mining
            } else {
                HealthStatus::Degraded
            },
            severity: if uart_exists {
                Severity::Ok
            } else {
                Severity::Warn
            },
            issues: if uart_exists {
                Vec::new()
            } else {
                vec![format!(
                    "asic uart path {} is missing on this host",
                    self.uart_path.display()
                )]
            },
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        MinerStatus {
            hashrate_ths: 0.0,
            power_w: 0,
            efficiency_j_th: 0.0,
            accepted_shares: 0,
            rejected_shares: 0,
            mode: MinerMode::Balanced,
            paused: false,
            pause_reason: None,
        }
    }

    pub fn chain_statuses(&self) -> Vec<ChainStatus> {
        let present = self.uart_path.exists();
        (0..3)
            .map(|id| ChainStatus {
                id,
                present,
                enabled: present,
                asic_detected: if present { 1 } else { 0 },
                temp_board_c: 0.0,
                temp_chip_max_c: 0.0,
                fault: (!present).then(|| {
                    format!(
                        "asic uart path {} is missing on this host",
                        self.uart_path.display()
                    )
                }),
            })
            .collect()
    }

    pub fn probe_report(&self) -> HardwareProbeReport {
        HardwareProbeReport::new(
            self.mode(),
            self.model,
            self.profile.family,
            None,
            probe_plan(
                self.profile.family,
                ProbeMode::ReadOnlyFilesystem(PathBuf::from("/")),
            ),
            vec![
                "live backend keeps probe checks read-only for identification".to_string(),
                format!(
                    "asic job dispatch writes frames to {}",
                    self.uart_path.display()
                ),
            ],
        )
    }

    pub fn identity_report(&self) -> HardwareIdentityReport {
        infer_hardware_identity(
            self.mode(),
            self.profile.family,
            self.model,
            identity_observations(
                self.profile.family,
                ProbeMode::ReadOnlyFilesystem(PathBuf::from("/")),
            ),
        )
    }

    pub fn dispatch_job(&self, job: &StratumJobTemplate) -> Result<(), BackendError> {
        let payload = self.protocol.notify(job);
        let mut transport = self
            .uart_transport
            .lock()
            .expect("uart transport mutex poisoned");
        if transport.is_none() {
            *transport =
                Some(
                    self.open_uart_transport()
                        .map_err(|source| BackendError::DispatchIo {
                            path: self.uart_path.display().to_string(),
                            source,
                        })?,
                );
        }
        let transport = transport
            .as_mut()
            .expect("uart transport must be initialized");
        transport
            .file
            .write_all(&payload)
            .map_err(|source| BackendError::DispatchIo {
                path: self.uart_path.display().to_string(),
                source,
            })?;
        transport
            .file
            .flush()
            .map_err(|source| BackendError::DispatchIo {
                path: self.uart_path.display().to_string(),
                source,
            })?;
        Ok(())
    }

    pub fn read_nonce_reports(
        &self,
        max_reports: usize,
    ) -> Result<Vec<AsicNonceReport>, BackendError> {
        if max_reports == 0 {
            return Ok(Vec::new());
        }

        let mut transport = self
            .uart_transport
            .lock()
            .expect("uart transport mutex poisoned");
        if transport.is_none() {
            *transport =
                Some(
                    self.open_uart_transport()
                        .map_err(|source| BackendError::ReceiveIo {
                            path: self.uart_path.display().to_string(),
                            source,
                        })?,
                );
        }
        let transport = transport
            .as_mut()
            .expect("uart transport must be initialized");

        let mut reports = Vec::new();
        let mut scratch = [0_u8; 512];
        loop {
            match transport.file.read(&mut scratch) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    transport
                        .rx_buffer
                        .extend_from_slice(&scratch[..bytes_read]);
                    while reports.len() < max_reports {
                        let Some(line) = pop_line(&mut transport.rx_buffer) else {
                            break;
                        };
                        match parse_nonce_report_frame(&line) {
                            Ok(report) => reports.push(report),
                            Err(AsicFrameError::Empty | AsicFrameError::NotNonceReport) => {}
                            Err(source) => {
                                return Err(BackendError::ReceiveFrame {
                                    path: self.uart_path.display().to_string(),
                                    source,
                                });
                            }
                        }
                    }
                    if reports.len() >= max_reports {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(source) => {
                    return Err(BackendError::ReceiveIo {
                        path: self.uart_path.display().to_string(),
                        source,
                    });
                }
            }
        }

        Ok(reports)
    }

    pub fn collect_share_candidates(
        &self,
        worker: &str,
        max_reports: usize,
    ) -> Result<Vec<StratumShareCandidate>, BackendError> {
        self.read_nonce_reports(max_reports).map(|reports| {
            reports
                .into_iter()
                .map(|report| report.into_share_candidate(worker.to_string()))
                .collect()
        })
    }

    pub fn build_set_frequency_frame(&self, chip_id: u16, frequency_mhz: u16) -> Vec<u8> {
        self.protocol.set_frequency(chip_id, frequency_mhz)
    }

    pub fn build_set_voltage_frame(&self, chip_id: u16, voltage_mv: u16) -> Vec<u8> {
        self.protocol.set_voltage(chip_id, voltage_mv)
    }

    pub fn build_tuning_sequence_frames(
        &self,
        chip_id: u16,
        phases: &[TuningPhase],
        base_frequency_mhz: u16,
        base_voltage_mv: u16,
        step_frequency_mhz: u16,
        step_voltage_mv: u16,
    ) -> Vec<Vec<u8>> {
        self.protocol.tuning_sequence(
            chip_id,
            phases,
            base_frequency_mhz,
            base_voltage_mv,
            step_frequency_mhz,
            step_voltage_mv,
        )
    }

    /// Whether this backend drives a real BM1398 chain (S19 XIL / BHB428xx).
    pub fn supports_bm1398(&self) -> bool {
        self.profile.family == BoardFamily::Xilinx
    }

    /// Resolve the PLL dividers for `frequency_mhz` on the BM1398 chain.
    ///
    /// Returns `None` on non-Xilinx boards or an unsolvable target.
    pub fn bm1398_pll(&self, frequency_mhz: u16) -> Option<bm1398::Bm1398Pll> {
        self.supports_bm1398()
            .then(|| bm1398::solve_pll(frequency_mhz))
            .flatten()
    }

    /// Build the real VIL frame that sets the BM1398 PLL frequency.
    ///
    /// When `chip_address` is `None` the write is broadcast to every chip.
    /// Returns `None` on non-Xilinx boards or an unsolvable target.
    pub fn bm1398_set_frequency_frame(
        &self,
        chip_address: Option<u8>,
        frequency_mhz: u16,
    ) -> Option<Vec<u8>> {
        let pll = self.bm1398_pll(frequency_mhz)?;
        Some(
            bm1398::VilCommand::WriteRegister {
                all: chip_address.is_none(),
                chip_address: chip_address.unwrap_or(0),
                register: bm1398::reg::PLL0_PARAMETER,
                value: pll.to_register(),
            }
            .to_frame(),
        )
    }

    /// Build the VIL command stream that enumerates the BM1398 chain.
    ///
    /// Returns an empty stream on non-Xilinx boards.
    pub fn bm1398_enumerate_chain(&self, chip_count: u16, interval: u8) -> Vec<Vec<u8>> {
        if self.supports_bm1398() {
            bm1398::enumerate_chain(chip_count, interval)
        } else {
            Vec::new()
        }
    }

    /// Decode one BM1398 nonce FIFO block (four words: regs 4/5/4/5).
    pub fn bm1398_decode_nonce(&self, words: &[u32; 4]) -> Option<bm1398::NonceEntry> {
        self.supports_bm1398()
            .then(|| bm1398::decode_nonce_block(words))
    }

    /// Whether the real FPGA path is usable: an S19 XIL board with writes armed.
    ///
    /// Arming requires `OPENMINEROS_ALLOW_FPGA=1`; it is off by default so the
    /// runtime never touches `/dev/axi_fpga_dev` unless an operator opts in. This
    /// is the in-backend half of the hardware safety gate for the BM1398 path.
    pub fn fpga_armed(&self) -> bool {
        self.fpga_armed && self.supports_bm1398()
    }

    /// Acquire the lazily-opened BM1398 chain driver, opening the FPGA device on
    /// first use. Errors (kept inert) when the path is not armed, the board is
    /// not Xilinx, or the device is absent — e.g. on a development host.
    fn fpga_session(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<bm1398_driver::Bm1398Driver>>, BackendError> {
        if self.profile.family != BoardFamily::Xilinx {
            return Err(BackendError::FpgaUnavailable {
                reason: "the BM1398 FPGA path is only available on S19 XIL (Xilinx) boards"
                    .to_string(),
            });
        }
        if !self.fpga_armed {
            return Err(BackendError::FpgaUnavailable {
                reason: "FPGA writes are not armed; set OPENMINEROS_ALLOW_FPGA=1 to enable"
                    .to_string(),
            });
        }
        let mut guard = self
            .fpga_session
            .lock()
            .expect("fpga session mutex poisoned");
        if guard.is_none() {
            if !self.fpga_device_path.exists() {
                return Err(BackendError::FpgaUnavailable {
                    reason: format!(
                        "fpga device {} is not present on this host",
                        self.fpga_device_path.display()
                    ),
                });
            }
            let driver = bm1398_driver::Bm1398Driver::open_device(&self.fpga_device_path)
                .map_err(|source| BackendError::FpgaUnavailable {
                    reason: format!(
                        "failed to open fpga device {}: {source}",
                        self.fpga_device_path.display()
                    ),
                })?;
            *guard = Some(driver);
        }
        Ok(guard)
    }

    /// Bring up the real BM1398 chain: read the FPGA version, enumerate
    /// `chip_count` chips, and set the chain frequency. Gated by [`Self::fpga_armed`].
    pub fn bring_up_chain(
        &self,
        chain: u8,
        chip_count: u16,
        frequency_mhz: u16,
    ) -> Result<ChainBringUp, BackendError> {
        let mut guard = self.fpga_session()?;
        let driver = guard.as_mut().expect("fpga session initialized");
        let fpga_version = driver.fpga_version();
        driver.enable_nonce_rx();
        let enumerated_chips = driver.enumerate(chain, chip_count);
        let pll = driver
            .set_frequency_all(chain, frequency_mhz)
            .ok_or(BackendError::FpgaUnavailable {
                reason: format!("frequency {frequency_mhz} MHz is not solvable for BM1398"),
            })?;
        Ok(ChainBringUp {
            fpga_version,
            enumerated_chips,
            frequency_mhz: pll.realized_mhz as u16,
        })
    }

    /// Build a BM1398 work item from a full Stratum job and submit it to the
    /// real chain. Ties the work builder to the gated FPGA path.
    pub fn fpga_submit_job(
        &self,
        job: &openmineros_common::mining::MiningJob,
        extranonce1: &[u8],
        extranonce2: &[u8],
        version_rolls: [u32; openmineros_common::mining::VERSION_ROLL_MIDSTATES],
        work_id: u8,
    ) -> Result<(), BackendError> {
        let work = work_builder::build_work_item(
            job,
            extranonce1,
            extranonce2,
            version_rolls,
            work_id,
            0,
        );
        self.fpga_submit_work(&work)
    }

    /// Submit a prepared [`bm1398::WorkItem`] to the real chain over the FPGA.
    pub fn fpga_submit_work(&self, work: &bm1398::WorkItem) -> Result<(), BackendError> {
        let mut guard = self.fpga_session()?;
        let driver = guard.as_mut().expect("fpga session initialized");
        driver
            .submit_work_item(work)
            .map_err(|source| BackendError::FpgaUnavailable {
                reason: format!("invalid work item: {source:?}"),
            })
    }

    /// Drain and decode up to `max_entries` nonces from the real chain.
    pub fn fpga_poll_nonces(
        &self,
        max_entries: usize,
    ) -> Result<Vec<bm1398::NonceEntry>, BackendError> {
        let mut guard = self.fpga_session()?;
        let driver = guard.as_mut().expect("fpga session initialized");
        Ok(driver.poll_nonces(max_entries))
    }

    fn open_uart_transport(&self) -> std::io::Result<UartTransport> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        options.open(&self.uart_path).map(|file| UartTransport {
            file,
            rx_buffer: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
struct AsicProtocol {
    variant: AsicProtocolVariant,
}

#[derive(Debug, Clone)]
enum AsicCommand {
    Notify(AsicNotifyCommand),
    SetFrequency(AsicSetFrequencyCommand),
    SetVoltage(AsicSetVoltageCommand),
}

#[derive(Debug, Clone)]
struct AsicNotifyCommand {
    job_id: String,
    prev_hash: String,
    merkle_branch_len: usize,
    version: String,
    bits: String,
    time: String,
    clean_jobs: bool,
}

#[derive(Debug, Clone)]
struct AsicSetFrequencyCommand {
    chip_id: u16,
    frequency_mhz: u16,
}

#[derive(Debug, Clone)]
struct AsicSetVoltageCommand {
    chip_id: u16,
    voltage_mv: u16,
}

#[derive(Debug, Clone, Copy)]
enum AsicProtocolVariant {
    XilinxV1,
    BeagleBoneV1,
    AmlogicV1,
    CvitekV1,
}

impl AsicProtocol {
    fn for_board(board: BoardFamily) -> Self {
        let variant = match board {
            BoardFamily::Xilinx => AsicProtocolVariant::XilinxV1,
            BoardFamily::BeagleBone => AsicProtocolVariant::BeagleBoneV1,
            BoardFamily::Amlogic => AsicProtocolVariant::AmlogicV1,
            BoardFamily::Cvitek => AsicProtocolVariant::CvitekV1,
        };
        Self { variant }
    }

    fn frame_prefix(&self) -> &'static str {
        match self.variant {
            AsicProtocolVariant::XilinxV1 => "omo-asic/xilinx/v1",
            AsicProtocolVariant::BeagleBoneV1 => "omo-asic/beaglebone/v1",
            AsicProtocolVariant::AmlogicV1 => "omo-asic/amlogic/v1",
            AsicProtocolVariant::CvitekV1 => "omo-asic/cvitek/v1",
        }
    }

    fn board_tag(&self) -> &'static str {
        match self.variant {
            AsicProtocolVariant::XilinxV1 => "xilinx",
            AsicProtocolVariant::BeagleBoneV1 => "beaglebone",
            AsicProtocolVariant::AmlogicV1 => "amlogic",
            AsicProtocolVariant::CvitekV1 => "cvitek",
        }
    }

    fn notify(&self, job: &StratumJobTemplate) -> Vec<u8> {
        self.encode(AsicCommand::Notify(AsicNotifyCommand {
            job_id: job.job_id.clone(),
            prev_hash: job.prev_hash.clone(),
            merkle_branch_len: job.merkle_branch_len,
            version: job.version.clone(),
            bits: job.bits.clone(),
            time: job.time.clone(),
            clean_jobs: job.clean_jobs,
        }))
    }

    fn nonce_report(
        &self,
        job_id: &str,
        chip_id: u16,
        extranonce2: &str,
        ntime: &str,
        nonce: &str,
    ) -> Vec<u8> {
        let body = format!(
            "{}|board={}|cmd=nonce|job_id={}|chip_id={}|extranonce2={}|ntime={}|nonce={}",
            self.frame_prefix(),
            self.board_tag(),
            job_id,
            chip_id,
            extranonce2,
            ntime,
            nonce
        );
        let checksum = checksum_hex(body.as_bytes());
        format!("{body}|checksum={checksum}\n").into_bytes()
    }

    fn set_frequency(&self, chip_id: u16, frequency_mhz: u16) -> Vec<u8> {
        self.encode(AsicCommand::SetFrequency(AsicSetFrequencyCommand {
            chip_id,
            frequency_mhz,
        }))
    }

    fn set_voltage(&self, chip_id: u16, voltage_mv: u16) -> Vec<u8> {
        self.encode(AsicCommand::SetVoltage(AsicSetVoltageCommand {
            chip_id,
            voltage_mv,
        }))
    }

    fn tuning_sequence(
        &self,
        chip_id: u16,
        phases: &[TuningPhase],
        base_frequency_mhz: u16,
        base_voltage_mv: u16,
        step_frequency_mhz: u16,
        step_voltage_mv: u16,
    ) -> Vec<Vec<u8>> {
        let mut sequence = Vec::new();
        let mut frequency_mhz = base_frequency_mhz;
        let mut voltage_mv = base_voltage_mv;

        for phase in phases {
            sequence.push(match phase {
                TuningPhase::Baseline => self.set_frequency(chip_id, frequency_mhz),
                TuningPhase::DownclockEfficiency => {
                    frequency_mhz = frequency_mhz.saturating_sub(step_frequency_mhz);
                    self.set_frequency(chip_id, frequency_mhz)
                }
                TuningPhase::UpclockStability => {
                    frequency_mhz = frequency_mhz.saturating_add(step_frequency_mhz);
                    self.set_frequency(chip_id, frequency_mhz)
                }
                TuningPhase::VoltageTrim => {
                    voltage_mv = voltage_mv.saturating_sub(step_voltage_mv);
                    self.set_voltage(chip_id, voltage_mv)
                }
            });
        }

        sequence
    }

    fn encode(&self, command: AsicCommand) -> Vec<u8> {
        match command {
            AsicCommand::Notify(command) => {
                let body = format!(
                    "{}|board={}|cmd=notify|job_id={}|prev_hash={}|merkle_branch_len={}|version={}|bits={}|time={}|clean_jobs={}",
                    self.frame_prefix(),
                    self.board_tag(),
                    command.job_id,
                    command.prev_hash,
                    command.merkle_branch_len,
                    command.version,
                    command.bits,
                    command.time,
                    u8::from(command.clean_jobs)
                );
                let checksum = checksum_hex(body.as_bytes());
                format!("{body}|checksum={checksum}\n").into_bytes()
            }
            AsicCommand::SetFrequency(command) => {
                let body = format!(
                    "{}|board={}|cmd=set_frequency|chip_id={}|frequency_mhz={}",
                    self.frame_prefix(),
                    self.board_tag(),
                    command.chip_id,
                    command.frequency_mhz
                );
                let checksum = checksum_hex(body.as_bytes());
                format!("{body}|checksum={checksum}\n").into_bytes()
            }
            AsicCommand::SetVoltage(command) => {
                let body = format!(
                    "{}|board={}|cmd=set_voltage|chip_id={}|voltage_mv={}",
                    self.frame_prefix(),
                    self.board_tag(),
                    command.chip_id,
                    command.voltage_mv
                );
                let checksum = checksum_hex(body.as_bytes());
                format!("{body}|checksum={checksum}\n").into_bytes()
            }
        }
    }
}

fn checksum_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn pop_line(buffer: &mut Vec<u8>) -> Option<String> {
    let newline = buffer.iter().position(|byte| *byte == b'\n')?;
    let line: Vec<u8> = buffer.drain(..=newline).collect();
    Some(String::from_utf8_lossy(&line).trim().to_string())
}

fn required_field(value: Option<String>, field: &'static str) -> Result<String, AsicFrameError> {
    let value = value.ok_or(AsicFrameError::MissingField(field))?;
    if value.trim().is_empty() {
        return Err(AsicFrameError::InvalidField { field, value });
    }
    Ok(value)
}

fn required_hex_field(
    value: Option<String>,
    field: &'static str,
    exact_len: Option<usize>,
) -> Result<String, AsicFrameError> {
    let value = required_field(value, field)?;
    let has_valid_len = exact_len.map_or(value.len() % 2 == 0, |len| value.len() == len);
    if !has_valid_len || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AsicFrameError::InvalidField { field, value });
    }
    Ok(value)
}

impl HardwareProbeBackend {
    pub fn new(model: Model, board: BoardFamily) -> Result<Self, BackendError> {
        let probe_root = env::var_os("OPENMINEROS_PROBE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self::with_probe_root(model, board, probe_root)
    }

    pub fn with_probe_root(
        model: Model,
        board: BoardFamily,
        probe_root: PathBuf,
    ) -> Result<Self, BackendError> {
        let support = target_support(model, board)?;
        let profile = board_profile(board);

        Ok(Self {
            model,
            profile,
            support,
            probe_root,
        })
    }

    pub fn mode(&self) -> RuntimeBackendMode {
        RuntimeBackendMode::HardwareProbe
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn profile(&self) -> &BoardProfile {
        &self.profile
    }

    pub fn support(&self) -> SupportLevel {
        self.support
    }

    pub fn runtime_status(&self) -> BackendRuntimeStatus {
        BackendRuntimeStatus {
            state: HealthStatus::Recovering,
            severity: Severity::Warn,
            issues: vec![
                format!(
                    "hardware-probe backend scans read-only evidence under {}",
                    self.probe_root.display()
                ),
                "ASIC bus probing is read-only and no mining writes are issued".to_string(),
            ],
        }
    }

    pub fn miner_status(&self) -> MinerStatus {
        MinerStatus {
            hashrate_ths: 0.0,
            power_w: 0,
            efficiency_j_th: 0.0,
            accepted_shares: 0,
            rejected_shares: 0,
            mode: MinerMode::SafeMode,
            paused: false,
            pause_reason: None,
        }
    }

    pub fn chain_statuses(&self) -> Vec<ChainStatus> {
        (0..3)
            .map(|id| ChainStatus {
                id,
                present: false,
                enabled: false,
                asic_detected: 0,
                temp_board_c: 0.0,
                temp_chip_max_c: 0.0,
                fault: Some("hardware probing is not implemented in build 0.1.0".to_string()),
            })
            .collect()
    }

    pub fn probe_report(&self) -> HardwareProbeReport {
        HardwareProbeReport::new(
            self.mode(),
            self.model,
            self.profile.family,
            Some(self.probe_root.display().to_string()),
            probe_plan(
                self.profile.family,
                ProbeMode::ReadOnlyFilesystem(self.probe_root.clone()),
            ),
            vec![
                format!(
                    "checks read-only OS paths under {}",
                    self.probe_root.display()
                ),
                "no GPIO, UART, I2C, SPI, fan, voltage, clock, or ASIC commands are issued"
                    .to_string(),
            ],
        )
    }

    pub fn identity_report(&self) -> HardwareIdentityReport {
        infer_hardware_identity(
            self.mode(),
            self.profile.family,
            self.model,
            identity_observations(
                self.profile.family,
                ProbeMode::ReadOnlyFilesystem(self.probe_root.clone()),
            ),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProbeMode {
    Skipped,
    ReadOnlyFilesystem(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProbeExpectation {
    name: &'static str,
    interface: &'static str,
    path: &'static str,
    required: bool,
}

fn probe_plan(board: BoardFamily, mode: ProbeMode) -> Vec<ProbeCheck> {
    probe_expectations(board)
        .into_iter()
        .map(|expectation| probe_check(expectation, mode.clone()))
        .collect()
}

fn identity_observations(board: BoardFamily, mode: ProbeMode) -> Vec<HardwareIdentityObservation> {
    let mut observations = Vec::new();

    for expectation in probe_expectations(board) {
        if let ProbeMode::ReadOnlyFilesystem(ref root) = mode {
            let path = probe_root_path(root, expectation.path);
            if path.is_file() {
                if let Ok(value) = std::fs::read_to_string(&path) {
                    observations.push(HardwareIdentityObservation {
                        source: expectation.interface.to_string(),
                        key: expectation.name.to_string(),
                        value: sanitize_observation_value(&value),
                    });
                }
            } else if path.exists() {
                observations.push(HardwareIdentityObservation {
                    source: expectation.interface.to_string(),
                    key: expectation.name.to_string(),
                    value: format!("{} present", expectation.path),
                });
            }
        }
    }

    observations
}

fn sanitize_observation_value(value: &str) -> String {
    value
        .replace('\0', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn probe_check(expectation: ProbeExpectation, mode: ProbeMode) -> ProbeCheck {
    let (status, detail) = match mode {
        ProbeMode::Skipped => (
            ProbeStatus::Skipped,
            "simulated backend skipped host filesystem probing".to_string(),
        ),
        ProbeMode::ReadOnlyFilesystem(root) => {
            let path = probe_root_path(&root, expectation.path);
            if path.exists() {
                let detail = if path.is_file() {
                    match std::fs::read_to_string(&path) {
                        Ok(value) => format!(
                            "detected read-only evidence: {}",
                            sanitize_observation_value(&value)
                        ),
                        Err(_) => "expected path exists on this host".to_string(),
                    }
                } else {
                    "expected path exists on this host".to_string()
                };
                (ProbeStatus::Detected, detail)
            } else {
                (
                    ProbeStatus::Missing,
                    "expected path is not present on this host".to_string(),
                )
            }
        }
    };

    ProbeCheck {
        name: expectation.name.to_string(),
        interface: expectation.interface.to_string(),
        path: expectation.path.to_string(),
        required: expectation.required,
        status,
        detail,
    }
}

fn probe_root_path(root: &Path, probe_path: &str) -> PathBuf {
    let relative = probe_path.strip_prefix('/').unwrap_or(probe_path);
    root.join(relative)
}

fn probe_expectations(board: BoardFamily) -> Vec<ProbeExpectation> {
    match board {
        BoardFamily::Xilinx => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: true,
            },
            ProbeExpectation {
                name: "device tree compatible",
                interface: "device-tree",
                path: "/proc/device-tree/compatible",
                required: true,
            },
            ProbeExpectation {
                name: "device tree serial number",
                interface: "device-tree",
                path: "/proc/device-tree/serial-number",
                required: false,
            },
            ProbeExpectation {
                name: "FPGA ASIC bridge",
                interface: "axi-fpga",
                path: "/dev/axi_fpga_dev",
                required: true,
            },
            ProbeExpectation {
                name: "console UART",
                interface: "uart",
                path: "/dev/ttyPS0",
                required: false,
            },
            ProbeExpectation {
                name: "GPIO control",
                interface: "gpio",
                path: "/sys/class/gpio",
                required: true,
            },
            ProbeExpectation {
                name: "hardware monitor sensors",
                interface: "hwmon",
                path: "/sys/class/hwmon",
                required: false,
            },
            ProbeExpectation {
                name: "thermal zones",
                interface: "thermal",
                path: "/sys/class/thermal",
                required: false,
            },
        ],
        BoardFamily::BeagleBone => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: true,
            },
            ProbeExpectation {
                name: "device tree compatible",
                interface: "device-tree",
                path: "/proc/device-tree/compatible",
                required: true,
            },
            ProbeExpectation {
                name: "device tree serial number",
                interface: "device-tree",
                path: "/proc/device-tree/serial-number",
                required: false,
            },
            ProbeExpectation {
                name: "control UART",
                interface: "uart",
                path: "/dev/ttyO1",
                required: true,
            },
            ProbeExpectation {
                name: "GPIO control",
                interface: "gpio",
                path: "/sys/class/gpio",
                required: true,
            },
            ProbeExpectation {
                name: "IIO sensor bus",
                interface: "iio",
                path: "/sys/bus/iio/devices",
                required: false,
            },
            ProbeExpectation {
                name: "thermal zones",
                interface: "thermal",
                path: "/sys/class/thermal",
                required: false,
            },
        ],
        BoardFamily::Amlogic => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: true,
            },
            ProbeExpectation {
                name: "device tree compatible",
                interface: "device-tree",
                path: "/proc/device-tree/compatible",
                required: true,
            },
            ProbeExpectation {
                name: "device tree serial number",
                interface: "device-tree",
                path: "/proc/device-tree/serial-number",
                required: false,
            },
            ProbeExpectation {
                name: "control UART",
                interface: "uart",
                path: "/dev/ttyS1",
                required: true,
            },
            ProbeExpectation {
                name: "GPIO control",
                interface: "gpio",
                path: "/sys/class/gpio",
                required: true,
            },
            ProbeExpectation {
                name: "hardware monitor sensors",
                interface: "hwmon",
                path: "/sys/class/hwmon",
                required: false,
            },
            ProbeExpectation {
                name: "thermal zones",
                interface: "thermal",
                path: "/sys/class/thermal",
                required: false,
            },
        ],
        BoardFamily::Cvitek => vec![
            ProbeExpectation {
                name: "device tree model",
                interface: "device-tree",
                path: "/proc/device-tree/model",
                required: false,
            },
            ProbeExpectation {
                name: "safe mode only",
                interface: "support-policy",
                path: "unsupported",
                required: false,
            },
        ],
    }
}

fn target_support(model: Model, board: BoardFamily) -> Result<SupportLevel, BackendError> {
    supported_targets()
        .into_iter()
        .find(|target| target.model == model && target.board == board)
        .map(|target| target.support)
        .ok_or(TargetError::UnsupportedTarget { model, board })
        .map_err(BackendError::Target)
}

fn board_profile(board: BoardFamily) -> BoardProfile {
    match board {
        BoardFamily::Xilinx => openmineros_board_xil::profile(),
        BoardFamily::BeagleBone => openmineros_board_bb::profile(),
        BoardFamily::Amlogic => openmineros_board_aml::profile(),
        BoardFamily::Cvitek => BoardProfile {
            family: BoardFamily::Cvitek,
            soc: "CV1835",
            recovery: "unsupported in 0.1.0",
            capabilities: CapabilitySet::from_flags([Capability::SafeMode]),
        },
    }
}

fn asic_uart_path(board: BoardFamily) -> PathBuf {
    PathBuf::from(match board {
        BoardFamily::Xilinx => "/dev/ttyPS0",
        BoardFamily::BeagleBone => "/dev/ttyO1",
        BoardFamily::Amlogic => "/dev/ttyS1",
        BoardFamily::Cvitek => "/dev/null",
    })
}

fn simulated_miner_status(model: Model) -> MinerStatus {
    let hashrate_ths = match model {
        Model::S19 => 95.0,
        Model::S19Pro => 110.0,
        Model::S19j => 90.0,
        Model::S19jPro => 104.0,
        Model::S19Xp => 141.0,
        Model::T19 => 84.0,
    };
    let power_w = match model {
        Model::S19Xp => 3010,
        Model::S19Pro => 3250,
        _ => 3068,
    };

    MinerStatus {
        hashrate_ths,
        power_w,
        efficiency_j_th: f64::from(power_w) / hashrate_ths,
        accepted_shares: 0,
        rejected_shares: 0,
        mode: MinerMode::Balanced,
        paused: false,
        pause_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmineros_common::HardwareIdentityState;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_uart_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("openmineros-asic-uart-{}.log", unique))
    }

    fn temp_probe_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("openmineros-probe-root-{}", unique))
    }

    #[test]
    fn constructs_all_mvp_simulated_backends() {
        for board in [
            BoardFamily::Xilinx,
            BoardFamily::BeagleBone,
            BoardFamily::Amlogic,
        ] {
            let backend = SimulatedBackend::new(Model::S19jPro, board).unwrap();
            assert_eq!(backend.mode(), RuntimeBackendMode::Simulated);
            assert_eq!(backend.profile().family, board);
            assert_eq!(backend.chain_statuses().len(), 3);
        }
    }

    #[test]
    fn hardware_mining_backend_uses_board_uart_path() {
        let backend = HardwareMiningBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let err = backend.dispatch_job(&StratumJobTemplate {
            job_id: "job-1".to_string(),
            prev_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            merkle_branch_len: 0,
            version: "20000000".to_string(),
            bits: "1d00ffff".to_string(),
            time: "5f5e1000".to_string(),
            clean_jobs: true,
        });

        assert!(matches!(err, Err(BackendError::DispatchIo { .. })));
    }

    #[test]
    fn hardware_mining_backend_reuses_uart_transport_and_writes_structured_frames() {
        let uart_path = temp_uart_path();
        fs::File::create(&uart_path).unwrap();
        let backend = HardwareMiningBackend::with_uart_path(
            Model::S19jPro,
            BoardFamily::Xilinx,
            uart_path.clone(),
        )
        .unwrap();
        let job = StratumJobTemplate {
            job_id: "job-1".to_string(),
            prev_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            merkle_branch_len: 2,
            version: "20000000".to_string(),
            bits: "1d00ffff".to_string(),
            time: "5f5e1000".to_string(),
            clean_jobs: true,
        };

        backend.dispatch_job(&job).unwrap();
        backend.dispatch_job(&job).unwrap();

        let content = fs::read_to_string(&uart_path).unwrap();
        assert!(content.contains("omo-asic/xilinx/v1|board=xilinx|cmd=notify|job_id=job-1"));
        assert!(content.contains("|checksum="));
        assert_eq!(content.matches("omo-asic/xilinx/v1").count(), 2);

        let _ = fs::remove_file(&uart_path);
    }

    #[test]
    fn nonce_report_frame_round_trips_into_share_candidate() {
        let frame = build_nonce_report_frame(
            BoardFamily::Xilinx,
            "job-42",
            7,
            "00000002",
            "5f5e1000",
            "00000001",
        );
        let frame = String::from_utf8(frame).unwrap();
        let report = parse_nonce_report_frame(&frame).unwrap();

        assert_eq!(report.board, BoardFamily::Xilinx);
        assert_eq!(report.job_id, "job-42");
        assert_eq!(report.chip_id, 7);

        let candidate = report.into_share_candidate("acct.worker");
        assert_eq!(candidate.worker, "acct.worker");
        assert_eq!(candidate.job_id, "job-42");
        assert_eq!(candidate.extranonce2, "00000002");
        assert_eq!(candidate.ntime, "5f5e1000");
        assert_eq!(candidate.nonce, "00000001");
    }

    #[test]
    fn nonce_report_rejects_bad_checksum() {
        let mut frame = String::from_utf8(build_nonce_report_frame(
            BoardFamily::Xilinx,
            "job-42",
            7,
            "00000002",
            "5f5e1000",
            "00000001",
        ))
        .unwrap();
        frame = frame.replace("00000001", "00000002");

        assert_eq!(
            parse_nonce_report_frame(&frame).unwrap_err(),
            AsicFrameError::ChecksumMismatch
        );
    }

    #[test]
    fn nonce_report_rejects_invalid_nonce() {
        let body = "omo-asic/xilinx/v1|board=xilinx|cmd=nonce|job_id=job-42|chip_id=7|extranonce2=00000002|ntime=5f5e1000|nonce=nothex";
        let checksum = checksum_hex(body.as_bytes());
        let frame = format!("{body}|checksum={checksum}\n");

        assert!(matches!(
            parse_nonce_report_frame(&frame),
            Err(AsicFrameError::InvalidField { field: "nonce", .. })
        ));
    }

    #[test]
    fn nonce_report_rejects_mismatched_board_tag() {
        let body = "omo-asic/xilinx/v1|board=beaglebone|cmd=nonce|job_id=job-42|chip_id=7|extranonce2=00000002|ntime=5f5e1000|nonce=00000001";
        let checksum = checksum_hex(body.as_bytes());
        let frame = format!("{body}|checksum={checksum}\n");

        assert!(matches!(
            parse_nonce_report_frame(&frame),
            Err(AsicFrameError::InvalidField { field: "board", .. })
        ));
    }

    #[test]
    fn hardware_mining_backend_collects_nonce_reports_from_uart() {
        let uart_path = temp_uart_path();
        fs::File::create(&uart_path).unwrap();
        let backend = HardwareMiningBackend::with_uart_path(
            Model::S19jPro,
            BoardFamily::Xilinx,
            uart_path.clone(),
        )
        .unwrap();
        let job = StratumJobTemplate {
            job_id: "job-42".to_string(),
            prev_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            merkle_branch_len: 0,
            version: "20000000".to_string(),
            bits: "1d00ffff".to_string(),
            time: "5f5e1000".to_string(),
            clean_jobs: true,
        };
        backend.dispatch_job(&job).unwrap();
        let nonce_report = build_nonce_report_frame(
            BoardFamily::Xilinx,
            "job-42",
            3,
            "00000002",
            "5f5e1000",
            "00000001",
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(&uart_path)
            .unwrap()
            .write_all(&nonce_report)
            .unwrap();

        let candidates = backend.collect_share_candidates("acct.worker", 4).unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].worker, "acct.worker");
        assert_eq!(candidates[0].job_id, "job-42");
        assert_eq!(candidates[0].nonce, "00000001");

        let _ = fs::remove_file(&uart_path);
    }

    #[test]
    fn hardware_mining_backend_builds_frequency_frames_for_external_tuning() {
        let backend = HardwareMiningBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let frame = backend.build_set_frequency_frame(7, 725);
        let frame = String::from_utf8(frame).unwrap();

        assert!(frame.starts_with("omo-asic/xilinx/v1|board=xilinx|cmd=set_frequency"));
        assert!(frame.contains("|chip_id=7|frequency_mhz=725|checksum="));
        assert!(frame.ends_with('\n'));
    }

    #[test]
    fn asic_protocol_uses_board_specific_prefixes() {
        let job = StratumJobTemplate {
            job_id: "job-1".to_string(),
            prev_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            merkle_branch_len: 2,
            version: "20000000".to_string(),
            bits: "1d00ffff".to_string(),
            time: "5f5e1000".to_string(),
            clean_jobs: true,
        };

        let xilinx = AsicProtocol::for_board(BoardFamily::Xilinx).notify(&job);
        let beaglebone = AsicProtocol::for_board(BoardFamily::BeagleBone).notify(&job);
        let amlogic = AsicProtocol::for_board(BoardFamily::Amlogic).notify(&job);

        assert!(
            String::from_utf8(xilinx)
                .unwrap()
                .starts_with("omo-asic/xilinx/v1|board=xilinx|cmd=notify")
        );
        assert!(
            String::from_utf8(beaglebone)
                .unwrap()
                .starts_with("omo-asic/beaglebone/v1|board=beaglebone|cmd=notify")
        );
        assert!(
            String::from_utf8(amlogic)
                .unwrap()
                .starts_with("omo-asic/amlogic/v1|board=amlogic|cmd=notify")
        );
    }

    #[test]
    fn notify_frame_contains_checksum_and_clean_job_flag() {
        let job = StratumJobTemplate {
            job_id: "job-2".to_string(),
            prev_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            merkle_branch_len: 1,
            version: "20000000".to_string(),
            bits: "1d00ffff".to_string(),
            time: "5f5e1000".to_string(),
            clean_jobs: false,
        };

        let frame = AsicProtocol::for_board(BoardFamily::Xilinx).notify(&job);
        let frame = String::from_utf8(frame).unwrap();

        assert!(frame.contains("|cmd=notify|"));
        assert!(frame.contains("|clean_jobs=0|checksum="));
        assert!(frame.ends_with('\n'));
    }

    #[test]
    fn set_frequency_frame_contains_board_tag_and_frequency_value() {
        let frame = AsicProtocol::for_board(BoardFamily::BeagleBone).set_frequency(3, 725);
        let frame = String::from_utf8(frame).unwrap();

        assert!(frame.starts_with("omo-asic/beaglebone/v1|board=beaglebone|cmd=set_frequency"));
        assert!(frame.contains("|chip_id=3|frequency_mhz=725|checksum="));
        assert!(frame.ends_with('\n'));
    }

    #[test]
    fn set_frequency_frame_differs_by_board_prefix() {
        let xilinx =
            String::from_utf8(AsicProtocol::for_board(BoardFamily::Xilinx).set_frequency(1, 600))
                .unwrap();
        let amlogic =
            String::from_utf8(AsicProtocol::for_board(BoardFamily::Amlogic).set_frequency(1, 600))
                .unwrap();

        assert!(xilinx.starts_with("omo-asic/xilinx/v1|board=xilinx|cmd=set_frequency"));
        assert!(amlogic.starts_with("omo-asic/amlogic/v1|board=amlogic|cmd=set_frequency"));
    }

    #[test]
    fn set_voltage_frame_contains_board_tag_and_voltage_value() {
        let frame = AsicProtocol::for_board(BoardFamily::Amlogic).set_voltage(5, 785);
        let frame = String::from_utf8(frame).unwrap();

        assert!(frame.starts_with("omo-asic/amlogic/v1|board=amlogic|cmd=set_voltage"));
        assert!(frame.contains("|chip_id=5|voltage_mv=785|checksum="));
        assert!(frame.ends_with('\n'));
    }

    #[test]
    fn hardware_mining_backend_builds_voltage_frames_for_external_tuning() {
        let backend = HardwareMiningBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let frame = backend.build_set_voltage_frame(9, 790);
        let frame = String::from_utf8(frame).unwrap();

        assert!(frame.starts_with("omo-asic/xilinx/v1|board=xilinx|cmd=set_voltage"));
        assert!(frame.contains("|chip_id=9|voltage_mv=790|checksum="));
        assert!(frame.ends_with('\n'));
    }

    #[test]
    fn hardware_mining_backend_builds_tuning_sequence_frames() {
        let backend = HardwareMiningBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let frames = backend.build_tuning_sequence_frames(
            11,
            &[
                TuningPhase::Baseline,
                TuningPhase::DownclockEfficiency,
                TuningPhase::UpclockStability,
                TuningPhase::VoltageTrim,
            ],
            725,
            800,
            5,
            5,
        );
        let frames: Vec<String> = frames
            .into_iter()
            .map(|frame| String::from_utf8(frame).unwrap())
            .collect();

        assert_eq!(frames.len(), 4);
        assert!(frames[0].contains("cmd=set_frequency"));
        assert!(frames[1].contains("cmd=set_frequency"));
        assert!(frames[2].contains("cmd=set_frequency"));
        assert!(frames[3].contains("cmd=set_voltage"));
        assert!(frames[3].contains("|chip_id=11|voltage_mv=795|checksum="));
    }

    #[test]
    fn build_tuning_protocol_frames_include_structured_targets() {
        let frames =
            build_tuning_protocol_frames(&openmineros_common::TuningProtocolSequenceSpec {
                board_family: BoardFamily::Xilinx,
                chip_id: 11,
                phases: vec![
                    TuningPhase::Baseline,
                    TuningPhase::DownclockEfficiency,
                    TuningPhase::UpclockStability,
                    TuningPhase::VoltageTrim,
                ],
                base_frequency_mhz: 725,
                base_voltage_mv: 800,
                frequency_step_mhz: 5,
                voltage_step_mv: 5,
                min_step_duration_seconds: 300,
            });

        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].order, 1);
        assert_eq!(frames[0].target_frequency_mhz, 725);
        assert_eq!(frames[0].target_voltage_mv, 800);
        assert_eq!(frames[1].target_frequency_mhz, 720);
        assert_eq!(frames[2].target_frequency_mhz, 725);
        assert_eq!(frames[3].target_voltage_mv, 795);
        assert_eq!(frames[3].min_duration_seconds, 300);
        assert!(frames[3].frame.contains("cmd=set_voltage"));
    }

    #[test]
    fn hardware_probe_backend_does_not_pretend_to_mine() {
        let backend = HardwareProbeBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let runtime = backend.runtime_status();
        let miner = backend.miner_status();
        let chains = backend.chain_statuses();

        assert_eq!(backend.mode(), RuntimeBackendMode::HardwareProbe);
        assert_eq!(runtime.state, HealthStatus::Recovering);
        assert_eq!(runtime.severity, Severity::Warn);
        assert_eq!(miner.hashrate_ths, 0.0);
        assert_eq!(miner.mode, MinerMode::SafeMode);
        assert!(chains.iter().all(|chain| !chain.present));
    }

    #[test]
    fn hardware_probe_backend_reads_probe_root_for_identity_and_checks() {
        let root = temp_probe_root();
        fs::create_dir_all(root.join("proc/device-tree")).unwrap();
        fs::create_dir_all(root.join("dev")).unwrap();
        fs::create_dir_all(root.join("sys/class/gpio")).unwrap();
        fs::create_dir_all(root.join("sys/class/hwmon")).unwrap();
        fs::create_dir_all(root.join("sys/class/thermal")).unwrap();
        fs::write(
            root.join("proc/device-tree/model"),
            b"Antminer S19j Pro Xilinx Zynq\0",
        )
        .unwrap();
        fs::write(
            root.join("proc/device-tree/compatible"),
            b"antminer,s19j-pro-xilinx\0",
        )
        .unwrap();
        fs::write(
            root.join("proc/device-tree/serial-number"),
            b"S19JPRO-0001\0",
        )
        .unwrap();
        fs::write(root.join("dev/ttyPS0"), b"").unwrap();
        fs::write(root.join("dev/axi_fpga_dev"), b"").unwrap();

        let backend = HardwareProbeBackend::with_probe_root(
            Model::S19jPro,
            BoardFamily::Xilinx,
            root.clone(),
        )
        .unwrap();
        let report = backend.probe_report();
        let identity = backend.identity_report();

        assert_eq!(report.probe_root.as_deref(), Some(root.to_str().unwrap()));
        assert_eq!(report.summary.missing_required, 0);
        assert_eq!(report.summary.detected, report.summary.total);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains(root.to_str().unwrap()))
        );
        assert_eq!(identity.state, HardwareIdentityState::Inferred);
        assert_eq!(identity.detected_board, Some(BoardFamily::Xilinx));
        assert_eq!(identity.detected_model, Some(Model::S19jPro));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn simulated_probe_report_is_skipped() {
        let backend = SimulatedBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let report = backend.probe_report();

        assert_eq!(report.backend, RuntimeBackendMode::Simulated);
        assert!(report.safe_read_only);
        assert_eq!(report.summary.skipped, report.summary.total);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == ProbeStatus::Skipped)
        );
    }

    #[test]
    fn simulated_identity_uses_configured_target_only() {
        let backend = SimulatedBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let identity = backend.identity_report();

        assert_eq!(
            identity.state,
            openmineros_common::HardwareIdentityState::ConfiguredOnly
        );
        assert_eq!(identity.configured_board, BoardFamily::Xilinx);
        assert_eq!(identity.configured_model, Model::S19jPro);
        assert!(identity.detected_board.is_none());
    }

    #[test]
    fn hardware_probe_report_has_required_xilinx_checks() {
        let backend = HardwareProbeBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let report = backend.probe_report();

        assert_eq!(report.backend, RuntimeBackendMode::HardwareProbe);
        assert_eq!(report.board_family, BoardFamily::Xilinx);
        assert!(report.safe_read_only);
        assert_eq!(report.summary.total, 8);
        // The real ASIC transport is the FPGA AXI bridge, and it is required.
        assert!(
            report.checks.iter().any(|check| check.interface == "axi-fpga"
                && check.path == "/dev/axi_fpga_dev"
                && check.required)
        );
        // The console UART still exists but is no longer the ASIC path.
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.interface == "uart" && check.path == "/dev/ttyPS0" && !check.required)
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.interface == "gpio" && check.required)
        );
    }

    #[test]
    fn hardware_probe_identity_is_read_only() {
        let backend = HardwareProbeBackend::new(Model::S19jPro, BoardFamily::Xilinx).unwrap();
        let identity = backend.identity_report();

        assert!(identity.safe_read_only);
        assert_eq!(identity.backend, RuntimeBackendMode::HardwareProbe);
        assert_eq!(identity.configured_board, BoardFamily::Xilinx);
        assert_eq!(identity.configured_model, Model::S19jPro);
    }

    #[test]
    fn fpga_path_is_unavailable_on_non_xilinx_boards() {
        let backend = HardwareMiningBackend::with_config(
            Model::S19jPro,
            BoardFamily::BeagleBone,
            PathBuf::from("/dev/null"),
            PathBuf::from("/nonexistent-fpga"),
            true,
        )
        .unwrap();
        assert!(!backend.fpga_armed());
        assert!(matches!(
            backend.bring_up_chain(0, 76, 525),
            Err(BackendError::FpgaUnavailable { .. })
        ));
    }

    #[test]
    fn fpga_path_is_disarmed_by_default() {
        let backend = HardwareMiningBackend::with_config(
            Model::S19jPro,
            BoardFamily::Xilinx,
            asic_uart_path(BoardFamily::Xilinx),
            PathBuf::from("/nonexistent-fpga"),
            false,
        )
        .unwrap();
        assert!(!backend.fpga_armed());
        match backend.bring_up_chain(0, 76, 525) {
            Err(BackendError::FpgaUnavailable { reason }) => assert!(reason.contains("not armed")),
            other => panic!("expected disarmed error, got {other:?}"),
        }
    }

    #[test]
    fn fpga_bring_up_runs_against_a_mapped_device() {
        // Back the register window with a regular file sized to the window.
        let path = temp_uart_path();
        let file = fs::File::create(&path).unwrap();
        file.set_len(crate::axi::WINDOW_BYTES as u64).unwrap();
        drop(file);

        let backend = HardwareMiningBackend::with_config(
            Model::S19jPro,
            BoardFamily::Xilinx,
            asic_uart_path(BoardFamily::Xilinx),
            path.clone(),
            true,
        )
        .unwrap();
        assert!(backend.fpga_armed());

        let report = backend.bring_up_chain(0, 76, 525).expect("bring up");
        assert_eq!(report.enumerated_chips, 76);
        assert_eq!(report.frequency_mhz, 525);

        // Work submission and nonce drain also reach the mapped device.
        let work = bm1398::WorkItem {
            work_id: 1,
            starting_nonce: 0,
            nbits: 0x1700_7fff,
            ntime: 0x6500_0000,
            merkle_root_tail: [0; 4],
            midstates: vec![[0u8; 32]; bm1398::WORK_MIDSTATES],
        };
        backend.fpga_submit_work(&work).expect("submit work");
        assert!(backend.fpga_poll_nonces(4).expect("poll").is_empty());

        let _ = fs::remove_file(&path);
    }
}
