use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub name: String,
    pub version: String,
    pub board: String,
    pub model: String,
    pub kind: String,
    pub flashable: bool,
    pub artifacts: Vec<ArtifactManifest>,
    pub signature: Option<ReleaseSignature>,
    pub warning: Option<String>,
}

impl ReleaseManifest {
    pub fn validate_policy(&self, allow_flashable_unsigned: bool) -> Result<(), ManifestError> {
        if self.schema_version != 1 {
            return Err(ManifestError::UnsupportedSchema(self.schema_version));
        }

        if self.artifacts.is_empty() {
            return Err(ManifestError::NoArtifacts);
        }

        if self.flashable && self.signature.is_none() && !allow_flashable_unsigned {
            return Err(ManifestError::UnsignedFlashableRelease);
        }

        if let Some(signature) = &self.signature {
            return Err(ManifestError::UnsupportedSignatureAlgorithm(
                signature.algorithm.clone(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub path: String,
    pub kind: String,
    pub sha256: String,
    pub bytes: u64,
}

impl ArtifactManifest {
    fn checked_path(&self, artifact_dir: &Path) -> Result<PathBuf, ManifestError> {
        let path = Path::new(&self.path);

        if path.is_absolute()
            || path
                .components()
                .any(|component| component.as_os_str() == "..")
        {
            return Err(ManifestError::UnsafeArtifactPath(self.path.clone()));
        }

        Ok(artifact_dir.join(path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseSignature {
    pub algorithm: String,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub manifest: String,
    pub flashable: bool,
    pub signature_required: bool,
    pub signature_present: bool,
    pub verified_artifacts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactBudget {
    pub max_install_image_bytes: u64,
    pub max_total_artifact_bytes: u64,
}

impl Default for ArtifactBudget {
    fn default() -> Self {
        Self {
            max_install_image_bytes: 64 * 1024 * 1024,
            max_total_artifact_bytes: 80 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactBudgetReport {
    pub manifest: String,
    pub max_install_image_bytes: u64,
    pub max_total_artifact_bytes: u64,
    pub total_artifact_bytes: u64,
    pub checked_artifacts: Vec<ArtifactBudgetEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactBudgetEntry {
    pub path: String,
    pub kind: String,
    pub bytes: u64,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to read manifest: {0}")]
    ReadManifest(#[source] std::io::Error),
    #[error("failed to parse manifest: {0}")]
    ParseManifest(#[source] serde_json::Error),
    #[error("unsupported manifest schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("manifest has no artifacts")]
    NoArtifacts,
    #[error("flashable releases must be signed")]
    UnsignedFlashableRelease,
    #[error("signature algorithm is not supported yet: {0}")]
    UnsupportedSignatureAlgorithm(String),
    #[error("unsafe artifact path in manifest: {0}")]
    UnsafeArtifactPath(String),
    #[error("failed to read artifact {path}: {source}")]
    ReadArtifact {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("artifact size mismatch for {path}: expected {expected}, got {actual}")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error("artifact sha256 mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("artifact budget exceeded for {path}: max {max}, got {actual}")]
    ArtifactBudgetExceeded { path: String, max: u64, actual: u64 },
    #[error("total artifact budget exceeded: max {max}, got {actual}")]
    TotalArtifactBudgetExceeded { max: u64, actual: u64 },
}

pub fn verify_manifest_file(
    manifest_path: impl AsRef<Path>,
    artifact_dir: impl AsRef<Path>,
    allow_flashable_unsigned: bool,
) -> Result<VerificationReport, ManifestError> {
    let manifest_path = manifest_path.as_ref();
    let artifact_dir = artifact_dir.as_ref();
    let manifest = load_manifest(manifest_path)?;

    manifest.validate_policy(allow_flashable_unsigned)?;

    let mut verified_artifacts = Vec::new();
    for artifact in &manifest.artifacts {
        verify_artifact(artifact, artifact_dir)?;
        verified_artifacts.push(artifact.path.clone());
    }

    Ok(VerificationReport {
        manifest: manifest_path.display().to_string(),
        flashable: manifest.flashable,
        signature_required: manifest.flashable && !allow_flashable_unsigned,
        signature_present: manifest.signature.is_some(),
        verified_artifacts,
    })
}

pub fn check_manifest_budget_file(
    manifest_path: impl AsRef<Path>,
    artifact_dir: impl AsRef<Path>,
    budget: ArtifactBudget,
) -> Result<ArtifactBudgetReport, ManifestError> {
    let manifest_path = manifest_path.as_ref();
    let artifact_dir = artifact_dir.as_ref();
    let manifest = load_manifest(manifest_path)?;
    manifest.validate_policy(false)?;

    let mut total_artifact_bytes = 0_u64;
    let mut checked_artifacts = Vec::new();

    for artifact in &manifest.artifacts {
        let path = artifact.checked_path(artifact_dir)?;
        let metadata = path
            .metadata()
            .map_err(|source| ManifestError::ReadArtifact {
                path: artifact.path.clone(),
                source,
            })?;
        let actual_bytes = metadata.len();

        if actual_bytes != artifact.bytes {
            return Err(ManifestError::SizeMismatch {
                path: artifact.path.clone(),
                expected: artifact.bytes,
                actual: actual_bytes,
            });
        }

        total_artifact_bytes = total_artifact_bytes.saturating_add(actual_bytes);
        let max_bytes = if artifact.kind == "install-image" {
            Some(budget.max_install_image_bytes)
        } else {
            None
        };

        if let Some(max) = max_bytes {
            if actual_bytes > max {
                return Err(ManifestError::ArtifactBudgetExceeded {
                    path: artifact.path.clone(),
                    max,
                    actual: actual_bytes,
                });
            }
        }

        checked_artifacts.push(ArtifactBudgetEntry {
            path: artifact.path.clone(),
            kind: artifact.kind.clone(),
            bytes: actual_bytes,
            max_bytes,
        });
    }

    if total_artifact_bytes > budget.max_total_artifact_bytes {
        return Err(ManifestError::TotalArtifactBudgetExceeded {
            max: budget.max_total_artifact_bytes,
            actual: total_artifact_bytes,
        });
    }

    Ok(ArtifactBudgetReport {
        manifest: manifest_path.display().to_string(),
        max_install_image_bytes: budget.max_install_image_bytes,
        max_total_artifact_bytes: budget.max_total_artifact_bytes,
        total_artifact_bytes,
        checked_artifacts,
    })
}

fn load_manifest(manifest_path: &Path) -> Result<ReleaseManifest, ManifestError> {
    let manifest_file = File::open(manifest_path).map_err(ManifestError::ReadManifest)?;
    serde_json::from_reader(manifest_file).map_err(ManifestError::ParseManifest)
}

fn verify_artifact(artifact: &ArtifactManifest, artifact_dir: &Path) -> Result<(), ManifestError> {
    let path = artifact.checked_path(artifact_dir)?;
    let file = File::open(&path).map_err(|source| ManifestError::ReadArtifact {
        path: artifact.path.clone(),
        source,
    })?;
    let metadata = file
        .metadata()
        .map_err(|source| ManifestError::ReadArtifact {
            path: artifact.path.clone(),
            source,
        })?;

    if metadata.len() != artifact.bytes {
        return Err(ManifestError::SizeMismatch {
            path: artifact.path.clone(),
            expected: artifact.bytes,
            actual: metadata.len(),
        });
    }

    let actual =
        sha256_hex(BufReader::new(file)).map_err(|source| ManifestError::ReadArtifact {
            path: artifact.path.clone(),
            source,
        })?;

    if actual != artifact.sha256 {
        return Err(ManifestError::HashMismatch {
            path: artifact.path.clone(),
            expected: artifact.sha256.clone(),
            actual,
        });
    }

    Ok(())
}

fn sha256_hex(mut reader: impl Read) -> Result<String, std::io::Error> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn rejects_flashable_unsigned_manifest() {
        let manifest = ReleaseManifest {
            schema_version: 1,
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            board: "s19-xil".to_string(),
            model: "s19j-pro".to_string(),
            kind: "development-placeholder".to_string(),
            flashable: true,
            artifacts: vec![ArtifactManifest {
                path: "image.img.xz".to_string(),
                kind: "install-image".to_string(),
                sha256: "abc".to_string(),
                bytes: 1,
            }],
            signature: None,
            warning: None,
        };

        assert!(matches!(
            manifest.validate_policy(false),
            Err(ManifestError::UnsignedFlashableRelease)
        ));
    }

    #[test]
    fn verifies_artifact_hashes() {
        let dir = temp_dir();
        let artifact_path = dir.join("image.img.xz");
        let mut artifact = File::create(&artifact_path).unwrap();
        artifact.write_all(b"placeholder\n").unwrap();

        let manifest_path = dir.join("manifest.json");
        let manifest = ReleaseManifest {
            schema_version: 1,
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            board: "s19-xil".to_string(),
            model: "s19j-pro".to_string(),
            kind: "development-placeholder".to_string(),
            flashable: false,
            artifacts: vec![ArtifactManifest {
                path: "image.img.xz".to_string(),
                kind: "install-image".to_string(),
                sha256: "2f73349cfc4630255319c6c8dfc1b46a8996ace9d14d8e07563b165915918ec2"
                    .to_string(),
                bytes: 12,
            }],
            signature: None,
            warning: Some("not flashable".to_string()),
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let report = verify_manifest_file(&manifest_path, &dir, false).unwrap();
        assert_eq!(report.verified_artifacts, vec!["image.img.xz"]);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checks_artifact_budget() {
        let dir = temp_dir();
        let artifact_path = dir.join("image.img.xz");
        let mut artifact = File::create(&artifact_path).unwrap();
        artifact.write_all(b"placeholder\n").unwrap();

        let manifest_path = dir.join("manifest.json");
        let manifest = ReleaseManifest {
            schema_version: 1,
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            board: "s19-xil".to_string(),
            model: "s19j-pro".to_string(),
            kind: "development-placeholder".to_string(),
            flashable: false,
            artifacts: vec![ArtifactManifest {
                path: "image.img.xz".to_string(),
                kind: "install-image".to_string(),
                sha256: "2f73349cfc4630255319c6c8dfc1b46a8996ace9d14d8e07563b165915918ec2"
                    .to_string(),
                bytes: 12,
            }],
            signature: None,
            warning: Some("not flashable".to_string()),
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let report =
            check_manifest_budget_file(&manifest_path, &dir, ArtifactBudget::default()).unwrap();

        assert_eq!(report.total_artifact_bytes, 12);
        assert_eq!(
            report.checked_artifacts[0].max_bytes,
            Some(64 * 1024 * 1024)
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_artifact_over_budget() {
        let dir = temp_dir();
        let artifact_path = dir.join("image.img.xz");
        let mut artifact = File::create(&artifact_path).unwrap();
        artifact.write_all(b"placeholder\n").unwrap();

        let manifest_path = dir.join("manifest.json");
        let manifest = ReleaseManifest {
            schema_version: 1,
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            board: "s19-xil".to_string(),
            model: "s19j-pro".to_string(),
            kind: "development-placeholder".to_string(),
            flashable: false,
            artifacts: vec![ArtifactManifest {
                path: "image.img.xz".to_string(),
                kind: "install-image".to_string(),
                sha256: "2f73349cfc4630255319c6c8dfc1b46a8996ace9d14d8e07563b165915918ec2"
                    .to_string(),
                bytes: 12,
            }],
            signature: None,
            warning: Some("not flashable".to_string()),
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let err = check_manifest_budget_file(
            &manifest_path,
            &dir,
            ArtifactBudget {
                max_install_image_bytes: 8,
                max_total_artifact_bytes: 80 * 1024 * 1024,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ManifestError::ArtifactBudgetExceeded { .. }));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_parent_directory_artifacts() {
        let artifact = ArtifactManifest {
            path: "../image.img.xz".to_string(),
            kind: "install-image".to_string(),
            sha256: String::new(),
            bytes: 0,
        };

        assert!(matches!(
            artifact.checked_path(Path::new(".")),
            Err(ManifestError::UnsafeArtifactPath(_))
        ));
    }

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("openmineros-manifest-test-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
