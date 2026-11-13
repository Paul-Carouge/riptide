use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use super::EngineError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStatus {
    pub yt_dlp_version: Option<String>,
    pub ffmpeg_available: bool,
}

/// Release installed on first run. Bump both constants together, from the release's `SHA2-256SUMS`.
const PINNED_YT_DLP_VERSION: &str = "2026.10.26";
const PINNED_YT_DLP_LINUX_SHA256: &str =
    "767ba9dce7052aa196f8b12652f433d76de128ba168966fa84b6394d5e6c6795";

const YT_DLP_RELEASE_ASSET_NAME: &str = "yt-dlp_linux";
const YT_DLP_CHECKSUMS_ASSET_NAME: &str = "SHA2-256SUMS";
const YT_DLP_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const HTTP_USER_AGENT: &str = concat!("riptide/", env!("CARGO_PKG_VERSION"));
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const HTTP_BODY_TIMEOUT: Duration = Duration::from_secs(600);
/// The standalone binary is ~35 MB; the cap only guards against a runaway response.
const MAXIMUM_DOWNLOAD_SIZE_IN_BYTES: u64 = 256 * 1024 * 1024;
const MAXIMUM_API_RESPONSE_SIZE_IN_BYTES: u64 = 4 * 1024 * 1024;
/// JavaScript runtimes yt-dlp can use to solve YouTube challenges, in yt-dlp's preference order.
const SUPPORTED_JAVASCRIPT_RUNTIMES: [&str; 3] = ["deno", "node", "bun"];

/// Fast, no network.
pub fn tool_status() -> ToolStatus {
    ToolStatus {
        yt_dlp_version: managed_yt_dlp_path()
            .ok()
            .and_then(|path| installed_yt_dlp_version(&path)),
        ffmpeg_available: is_program_runnable("ffmpeg", "-version")
            && is_program_runnable("ffprobe", "-version"),
    }
}

/// Installs the pinned yt-dlp if missing and checks ffmpeg.
pub fn ensure_tools() -> Result<ToolStatus, EngineError> {
    let binary_path = managed_yt_dlp_path()?;
    if installed_yt_dlp_version(&binary_path).is_none() {
        let agent = http_agent();
        let binary_url = format!(
            "https://github.com/yt-dlp/yt-dlp/releases/download/{PINNED_YT_DLP_VERSION}/{YT_DLP_RELEASE_ASSET_NAME}"
        );
        let binary_bytes = download_bytes(&agent, &binary_url, MAXIMUM_DOWNLOAD_SIZE_IN_BYTES)?;
        install_verified_binary(&binary_path, &binary_bytes, PINNED_YT_DLP_LINUX_SHA256)?;
    }
    let status = tool_status();
    if status.yt_dlp_version.is_none() {
        return Err(EngineError::Io(format!(
            "installed yt-dlp at {} does not run",
            binary_path.display()
        )));
    }
    if !status.ffmpeg_available {
        return Err(EngineError::FfmpegMissing);
    }
    Ok(status)
}

/// Installs the latest yt-dlp release (SHA-256 verified) and returns its version.
pub fn update_yt_dlp() -> Result<String, EngineError> {
    let binary_path = managed_yt_dlp_path()?;
    let agent = http_agent();
    let release = latest_release(&agent)?;
    if installed_yt_dlp_version(&binary_path).as_deref() == Some(release.tag_name.as_str()) {
        return Ok(release.tag_name);
    }
    let checksums_url = release.asset_download_url(YT_DLP_CHECKSUMS_ASSET_NAME)?;
    let binary_url = release.asset_download_url(YT_DLP_RELEASE_ASSET_NAME)?;
    let checksums_bytes =
        download_bytes(&agent, checksums_url, MAXIMUM_API_RESPONSE_SIZE_IN_BYTES)?;
    let checksums_text = String::from_utf8(checksums_bytes).map_err(|_| {
        EngineError::ToolDownloadFailed(format!("{YT_DLP_CHECKSUMS_ASSET_NAME} is not UTF-8"))
    })?;
    let expected_sha256 = sha256_for_file_in_checksums(&checksums_text, YT_DLP_RELEASE_ASSET_NAME)
        .ok_or_else(|| {
            EngineError::ToolDownloadFailed(format!(
                "{YT_DLP_RELEASE_ASSET_NAME} missing from {YT_DLP_CHECKSUMS_ASSET_NAME}"
            ))
        })?;
    let binary_bytes = download_bytes(&agent, binary_url, MAXIMUM_DOWNLOAD_SIZE_IN_BYTES)?;
    install_verified_binary(&binary_path, &binary_bytes, &expected_sha256)?;
    installed_yt_dlp_version(&binary_path).ok_or_else(|| {
        EngineError::Io(format!(
            "updated yt-dlp at {} does not run",
            binary_path.display()
        ))
    })
}

/// A `yt-dlp` command on the managed binary with the options every call shares.
pub(super) fn yt_dlp_command() -> Result<Command, EngineError> {
    let binary_path = managed_yt_dlp_path()?;
    if !binary_path.is_file() {
        return Err(EngineError::Io(format!(
            "yt-dlp is not installed at {}",
            binary_path.display()
        )));
    }
    let mut command = Command::new(binary_path);
    // The user's own yt-dlp config must not change what the app downloads or where.
    command.args(["--ignore-config", "--no-colors"]);
    if let Some(runtime) = first_available_javascript_runtime() {
        command.args(["--js-runtimes", runtime]);
    }
    command.stdin(Stdio::null());
    Ok(command)
}

fn managed_yt_dlp_path() -> Result<PathBuf, EngineError> {
    dirs::data_dir()
        .map(|data_directory| data_directory.join("riptide").join("bin").join("yt-dlp"))
        .ok_or_else(|| EngineError::Io("no XDG data directory (is $HOME set?)".to_string()))
}

fn installed_yt_dlp_version(binary_path: &Path) -> Option<String> {
    if !binary_path.is_file() {
        return None;
    }
    let output = Command::new(binary_path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

fn is_program_runnable(program: &str, version_flag: &str) -> bool {
    Command::new(program)
        .arg(version_flag)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// yt-dlp only enables deno by default; without a runtime YouTube hides most formats.
fn first_available_javascript_runtime() -> Option<&'static str> {
    let search_path = std::env::var_os("PATH")?;
    SUPPORTED_JAVASCRIPT_RUNTIMES.into_iter().find(|runtime| {
        std::env::split_paths(&search_path).any(|directory| directory.join(runtime).is_file())
    })
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(HTTP_USER_AGENT)
        .timeout_connect(Some(HTTP_CONNECT_TIMEOUT))
        .timeout_recv_body(Some(HTTP_BODY_TIMEOUT))
        .build()
        .new_agent()
}

fn download_bytes(
    agent: &ureq::Agent,
    url: &str,
    maximum_size_in_bytes: u64,
) -> Result<Vec<u8>, EngineError> {
    let to_error = |error: ureq::Error| EngineError::ToolDownloadFailed(format!("{url}: {error}"));
    let mut response = agent.get(url).call().map_err(to_error)?;
    response
        .body_mut()
        .with_config()
        .limit(maximum_size_in_bytes)
        .read_to_vec()
        .map_err(to_error)
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

impl GithubRelease {
    fn asset_download_url(&self, asset_name: &str) -> Result<&str, EngineError> {
        self.assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .map(|asset| asset.browser_download_url.as_str())
            .ok_or_else(|| {
                EngineError::ToolDownloadFailed(format!(
                    "release {} has no {asset_name}",
                    self.tag_name
                ))
            })
    }
}

fn latest_release(agent: &ureq::Agent) -> Result<GithubRelease, EngineError> {
    let body = download_bytes(
        agent,
        YT_DLP_LATEST_RELEASE_API_URL,
        MAXIMUM_API_RESPONSE_SIZE_IN_BYTES,
    )?;
    serde_json::from_slice(&body).map_err(|error| {
        EngineError::ToolDownloadFailed(format!("unexpected GitHub API response: {error}"))
    })
}

/// Finds the lowercase hex SHA-256 of `file_name` in a `sha256sum`-style listing.
fn sha256_for_file_in_checksums(checksums_text: &str, file_name: &str) -> Option<String> {
    checksums_text.lines().find_map(|line| {
        let (hash, listed_name) = line.trim().split_once(char::is_whitespace)?;
        // `sha256sum` marks binary-mode entries with a leading '*'.
        let listed_name = listed_name.trim_start().trim_start_matches('*');
        let is_valid_hash = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit());
        (listed_name == file_name && is_valid_hash).then(|| hash.to_ascii_lowercase())
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Checks the hash in memory, then writes a non-executable temp file, makes it executable and
/// renames it over the old binary, so a crash never leaves a half-written `yt-dlp`.
fn install_verified_binary(
    binary_path: &Path,
    binary_bytes: &[u8],
    expected_sha256: &str,
) -> Result<(), EngineError> {
    if !sha256_hex(binary_bytes).eq_ignore_ascii_case(expected_sha256) {
        return Err(EngineError::ToolChecksumMismatch);
    }
    let io_error =
        |action: &str, error: std::io::Error| EngineError::Io(format!("{action}: {error}"));
    let directory = binary_path
        .parent()
        .ok_or_else(|| EngineError::Io(format!("{} has no parent", binary_path.display())))?;
    fs::create_dir_all(directory).map_err(|error| io_error("create bin directory", error))?;
    let temporary_path = directory.join(format!(".yt-dlp.download-{}", std::process::id()));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary_path)?;
        file.write_all(binary_bytes)?;
        file.sync_all()?;
        fs::set_permissions(&temporary_path, fs::Permissions::from_mode(0o755))?;
        fs::rename(&temporary_path, binary_path)
    })();
    if let Err(error) = write_result {
        // Best effort: the temp file is useless and must not be mistaken for the binary.
        let _ = fs::remove_file(&temporary_path);
        return Err(io_error("install yt-dlp", error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECKSUMS_EXCERPT: &str = "\
e51e69e89489f199530f13d32a4342da3ddb2cf34842f364603a066db05afbf3  yt-dlp
f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d  yt-dlp.exe
767BA9DCE7052AA196F8B12652F433D76DE128BA168966FA84B6394D5E6C6795  yt-dlp_linux
1111111111111111111111111111111111111111111111111111111111111111 *yt-dlp_linux_aarch64
";

    #[test]
    fn checksums_parser_finds_the_exact_file_name() {
        assert_eq!(
            sha256_for_file_in_checksums(CHECKSUMS_EXCERPT, "yt-dlp_linux").as_deref(),
            Some(PINNED_YT_DLP_LINUX_SHA256)
        );
        assert_eq!(
            sha256_for_file_in_checksums(CHECKSUMS_EXCERPT, "yt-dlp").as_deref(),
            Some("e51e69e89489f199530f13d32a4342da3ddb2cf34842f364603a066db05afbf3")
        );
    }

    #[test]
    fn checksums_parser_handles_binary_marker_and_missing_names() {
        assert_eq!(
            sha256_for_file_in_checksums(CHECKSUMS_EXCERPT, "yt-dlp_linux_aarch64").as_deref(),
            Some("1111111111111111111111111111111111111111111111111111111111111111")
        );
        assert_eq!(
            sha256_for_file_in_checksums(CHECKSUMS_EXCERPT, "yt-dlp_macos"),
            None
        );
        assert_eq!(sha256_for_file_in_checksums("", "yt-dlp_linux"), None);
    }

    #[test]
    fn checksums_parser_rejects_malformed_hashes() {
        assert_eq!(
            sha256_for_file_in_checksums("abc  yt-dlp_linux", "yt-dlp_linux"),
            None
        );
        let not_hex = format!("{}  yt-dlp_linux", "z".repeat(64));
        assert_eq!(sha256_for_file_in_checksums(&not_hex, "yt-dlp_linux"), None);
    }

    #[test]
    fn sha256_hex_matches_the_known_empty_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn install_refuses_a_binary_with_the_wrong_hash_and_writes_nothing() {
        let directory =
            std::env::temp_dir().join(format!("riptide-tools-test-{}", std::process::id()));
        let binary_path = directory.join("yt-dlp");
        let result =
            install_verified_binary(&binary_path, b"not yt-dlp", PINNED_YT_DLP_LINUX_SHA256);
        assert_eq!(result, Err(EngineError::ToolChecksumMismatch));
        assert!(!binary_path.exists());
    }

    #[test]
    fn install_writes_an_executable_when_the_hash_matches() {
        let directory =
            std::env::temp_dir().join(format!("riptide-tools-ok-{}", std::process::id()));
        let binary_path = directory.join("yt-dlp");
        let bytes = b"#!/bin/sh\necho 1.0\n";
        install_verified_binary(&binary_path, bytes, &sha256_hex(bytes)).unwrap();
        let mode = fs::metadata(&binary_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            installed_yt_dlp_version(&binary_path).as_deref(),
            Some("1.0")
        );
        fs::remove_dir_all(&directory).unwrap();
    }
}
