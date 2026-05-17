use openmineros_common::{
    BoardFamily, BoardProfile, Capability, CapabilitySet, ChainStatus, HardwareIdentityObservation,
    HardwareIdentityReport, HardwareProbeReport, HealthStatus, MinerMode, MinerStatus, Model,
    ProbeCheck, ProbeStatus, RuntimeBackendMode, Severity, StratumJobTemplate, SupportLevel,
    TargetError, TuningPhase, TuningProtocolFrame, TuningProtocolSequenceSpec,
    infer_hardware_identity, supported_targets,
};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{File, OpenOptions},
    io::Write,
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
    uart_transport: Arc<Mutex<Option<File>>>,
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
        let support = target_support(model, board)?;
        let profile = board_profile(board);

        Ok(Self {
            model,
            profile,
            support,
            uart_path,
            protocol: AsicProtocol::for_board(board),
            uart_transport: Arc::new(Mutex::new(None)),
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
            let fd = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.uart_path)
                .map_err(|source| BackendError::DispatchIo {
                    path: self.uart_path.display().to_string(),
                    source,
                })?;
            *transport = Some(fd);
        }
        let fd = transport
            .as_mut()
            .expect("uart transport must be initialized");
        fd.write_all(&payload)
            .map_err(|source| BackendError::DispatchIo {
                path: self.uart_path.display().to_string(),
                source,
            })?;
        fd.flush().map_err(|source| BackendError::DispatchIo {
            path: self.uart_path.display().to_string(),
            source,
        })?;
        Ok(())
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
                name: "control UART",
                interface: "uart",
                path: "/dev/ttyPS0",
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
        assert_eq!(report.summary.total, 7);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.interface == "uart" && check.path == "/dev/ttyPS0")
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
}
