use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn verify_repro_renders_runtime_env_per_board() {
    let repo = repo_root();
    let cargo_target_dir = unique_temp_dir("openmineros-target");
    std::fs::create_dir_all(&cargo_target_dir).expect("create cargo target dir");

    for (board, model, media) in [
        ("s19-xil", "s19j-pro", "sd"),
        ("s19-bb", "s19", "sd"),
    ] {
        run_verify_repro(&repo, &cargo_target_dir, board, model, media);
    }
}

#[test]
fn collect_first_boot_script_uses_system_health_route_and_failfast_curl() {
    let repo = repo_root();
    let script = std::fs::read_to_string(repo.join("scripts/collect-s19-first-boot.sh"))
        .expect("read collect script");

    assert!(script.contains("/api/v1/system/health"));
    assert!(!script.contains("'/api/v1/health'"));
    assert!(script.contains("curl -fsS"));
    assert!(script.contains("failed to fetch $path from control plane"));
}

#[test]
fn macos_raw_sd_writer_verifies_hash_and_uses_pipefail() {
    let repo = repo_root();
    let script = std::fs::read_to_string(repo.join("scripts/macos-write-raw-sd.sh"))
        .expect("read macos writer script");

    assert!(script.contains("set -o pipefail"));
    assert!(script.contains("metadata=\"${image%.img.xz}.json\""));
    assert!(script.contains("compressed_sha256"));
    assert!(script.contains("shasum -a 256 \"$image\""));
    assert!(script.contains("image hash mismatch for $image"));
}

fn run_verify_repro(
    repo: &Path,
    cargo_target_dir: &Path,
    board: &str,
    model: &str,
    media: &str,
) {
    let output = Command::new("./scripts/verify-repro.sh")
        .current_dir(repo)
        .env("CARGO_TARGET_DIR", cargo_target_dir)
        .arg(board)
        .arg(model)
        .arg("0.1.0")
        .arg(media)
        .output()
        .expect("failed to execute verify-repro.sh");

    if !output.status.success() {
        panic!(
            "verify-repro.sh failed for {board}/{model} media={media}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("repo root")
        .to_path_buf()
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()))
}
