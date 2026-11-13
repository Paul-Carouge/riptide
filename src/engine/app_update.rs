//! Self-update of Riptide from its GitHub releases.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::EngineError;
use super::tools::{
    GithubRelease, MAXIMUM_API_RESPONSE_SIZE_IN_BYTES, MAXIMUM_DOWNLOAD_SIZE_IN_BYTES, fetch_bytes,
    http_agent, install_executable_atomically, sha256_for_file_in_checksums, sha256_hex,
};

pub const RIPTIDE_VERSION: &str = env!("CARGO_PKG_VERSION");

const RIPTIDE_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/Paul-Carouge/riptide/releases/latest";
/// Assets are only accepted from this repository's own release downloads.
const RIPTIDE_RELEASE_DOWNLOAD_URL_PREFIX: &str =
    "https://github.com/Paul-Carouge/riptide/releases/download/";
const RIPTIDE_RELEASE_ASSET_NAME: &str = "riptide-x86_64-linux";
const RIPTIDE_CHECKSUMS_ASSET_NAME: &str = "SHA256SUMS";
/// Linux names the executable of a running process this way once its file was replaced.
const REPLACED_EXECUTABLE_SUFFIX: &str = " (deleted)";
const HTTP_STATUS_NOT_FOUND: u16 = 404;

/// A published Riptide release newer than the running one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRelease {
    /// Without the leading `v`, e.g. `1.2.0`.
    pub version: String,
    /// Release notes (Markdown), possibly empty.
    pub notes: String,
    pub asset_url: String,
    pub checksums_url: String,
}

/// Asks GitHub for the latest release; `Some` only when it is strictly newer than this build.
pub fn check_for_app_update() -> Result<Option<AppRelease>, EngineError> {
    let agent = http_agent();
    let body = match fetch_bytes(
        &agent,
        RIPTIDE_LATEST_RELEASE_API_URL,
        MAXIMUM_API_RESPONSE_SIZE_IN_BYTES,
    ) {
        // GitHub answers 404 while the repository has no published release.
        Err(ureq::Error::StatusCode(HTTP_STATUS_NOT_FOUND)) => return Ok(None),
        Err(error) => {
            return Err(EngineError::AppUpdateFailed(format!(
                "{RIPTIDE_LATEST_RELEASE_API_URL}: {error}"
            )));
        }
        Ok(body) => body,
    };
    newer_release_from_github_json(&body, RIPTIDE_VERSION)
}

/// Downloads `release`, verifies its SHA-256, then atomically replaces the running executable.
/// Returns the installed version. The new version runs after `restart_app`.
pub fn install_app_update(release: &AppRelease) -> Result<String, EngineError> {
    let executable_path = running_executable_path()?;
    let directory = executable_path
        .parent()
        .ok_or_else(|| EngineError::Io(format!("{} has no parent", executable_path.display())))?;
    // Checked before downloading so a system-wide install fails fast with a clear message.
    if !is_directory_writable(directory) {
        return Err(EngineError::AppInstallDirectoryNotWritable(
            directory.display().to_string(),
        ));
    }
    let agent = http_agent();
    let download = |url: &str, maximum_size_in_bytes: u64| {
        fetch_bytes(&agent, url, maximum_size_in_bytes)
            .map_err(|error| EngineError::AppUpdateFailed(format!("{url}: {error}")))
    };
    let checksums_bytes = download(&release.checksums_url, MAXIMUM_API_RESPONSE_SIZE_IN_BYTES)?;
    let checksums_text = String::from_utf8(checksums_bytes).map_err(|_| {
        EngineError::AppUpdateFailed(format!("{RIPTIDE_CHECKSUMS_ASSET_NAME} is not UTF-8"))
    })?;
    let expected_sha256 = sha256_for_file_in_checksums(&checksums_text, RIPTIDE_RELEASE_ASSET_NAME)
        .ok_or_else(|| {
            EngineError::AppUpdateFailed(format!(
                "{RIPTIDE_RELEASE_ASSET_NAME} missing from {RIPTIDE_CHECKSUMS_ASSET_NAME}"
            ))
        })?;
    let binary_bytes = download(&release.asset_url, MAXIMUM_DOWNLOAD_SIZE_IN_BYTES)?;
    if sha256_hex(&binary_bytes) != expected_sha256 {
        return Err(EngineError::AppUpdateChecksumMismatch);
    }
    install_executable_atomically(&executable_path, &binary_bytes).map_err(|error| match error
        .kind()
    {
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
            EngineError::AppInstallDirectoryNotWritable(directory.display().to_string())
        }
        _ => EngineError::Io(format!("install Riptide update: {error}")),
    })?;
    Ok(release.version.clone())
}

/// Starts the (possibly just updated) executable again with the same arguments, detached from
/// this process. The caller quits the app right after.
pub fn restart_app() -> Result<(), EngineError> {
    let executable_path = running_executable_path()?;
    Command::new(&executable_path)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::null())
        // Own process group: signals aimed at the old app (e.g. Ctrl+C in its terminal) spare it.
        .process_group(0)
        .spawn()
        .map(drop)
        .map_err(|error| EngineError::Io(format!("restart {}: {error}", executable_path.display())))
}

/// `current_exe`, without the ` (deleted)` marker Linux adds once the file was replaced.
fn running_executable_path() -> Result<PathBuf, EngineError> {
    let path = std::env::current_exe()
        .map_err(|error| EngineError::Io(format!("locate the Riptide executable: {error}")))?;
    if path.exists() {
        return Ok(path);
    }
    Ok(path
        .to_str()
        .and_then(|text| text.strip_suffix(REPLACED_EXECUTABLE_SUFFIX))
        .map(PathBuf::from)
        .unwrap_or(path))
}

fn is_directory_writable(directory: &Path) -> bool {
    let Ok(directory_c_string) = CString::new(directory.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `directory_c_string` is a valid NUL-terminated string that outlives the call.
    unsafe { libc::access(directory_c_string.as_ptr(), libc::W_OK) == 0 }
}

/// Parses a GitHub `releases/latest` response; `Some` only if its tag is newer than
/// `current_version` and both expected assets are hosted by this repository.
fn newer_release_from_github_json(
    json_bytes: &[u8],
    current_version: &str,
) -> Result<Option<AppRelease>, EngineError> {
    let release: GithubRelease = serde_json::from_slice(json_bytes).map_err(|error| {
        EngineError::AppUpdateFailed(format!("unexpected GitHub API response: {error}"))
    })?;
    let published_version = parse_release_version(&release.tag_name).ok_or_else(|| {
        EngineError::AppUpdateFailed(format!("unexpected release tag {}", release.tag_name))
    })?;
    let running_version = parse_release_version(current_version).ok_or_else(|| {
        EngineError::AppUpdateFailed(format!("unexpected running version {current_version}"))
    })?;
    if published_version <= running_version {
        return Ok(None);
    }
    let asset_url = |asset_name: &str| {
        release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .map(|asset| asset.browser_download_url.clone())
            .filter(|url| url.starts_with(RIPTIDE_RELEASE_DOWNLOAD_URL_PREFIX))
            .ok_or_else(|| {
                EngineError::AppUpdateFailed(format!(
                    "release {} has no {asset_name} hosted on GitHub",
                    release.tag_name
                ))
            })
    };
    Ok(Some(AppRelease {
        version: format_release_version(published_version),
        notes: release.body.clone().unwrap_or_default(),
        asset_url: asset_url(RIPTIDE_RELEASE_ASSET_NAME)?,
        checksums_url: asset_url(RIPTIDE_CHECKSUMS_ASSET_NAME)?,
    }))
}

/// `X.Y.Z` or `vX.Y.Z`, digits only (no pre-release or build suffix). Arrays compare in
/// lexicographic order, which is semver precedence for plain versions.
fn parse_release_version(tag: &str) -> Option<[u64; 3]> {
    let parse_number = |part: &str| {
        let is_digits_only = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
        is_digits_only.then(|| part.parse().ok()).flatten()
    };
    let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
    let version = [
        parse_number(parts.next()?)?,
        parse_number(parts.next()?)?,
        parse_number(parts.next()?)?,
    ];
    parts.next().is_none().then_some(version)
}

fn format_release_version([major, minor, patch]: [u64; 3]) -> String {
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST_RELEASE_FIXTURE: &[u8] = include_bytes!("fixtures/riptide_latest_release.json");

    #[test]
    fn version_parser_accepts_plain_and_v_prefixed_versions() {
        assert_eq!(parse_release_version("v1.2.3"), Some([1, 2, 3]));
        assert_eq!(parse_release_version("0.10.0"), Some([0, 10, 0]));
        assert!(parse_release_version(RIPTIDE_VERSION).is_some());
    }

    #[test]
    fn version_parser_rejects_anything_else() {
        for tag in [
            "",
            "v",
            "1.2",
            "1.2.3.4",
            "v1.2.3-rc1",
            "1.2.x",
            "+1.2.3",
            "1..3",
            "V1.2.3",
            "v 1.2.3",
        ] {
            assert_eq!(parse_release_version(tag), None, "{tag:?}");
        }
    }

    #[test]
    fn versions_compare_numerically_not_as_text() {
        let version = |tag| parse_release_version(tag).unwrap();
        assert!(version("v0.10.0") > version("0.9.9"));
        assert!(version("v1.0.0") > version("0.99.99"));
        assert!(version("v0.1.1") > version("0.1.0"));
        assert!(version("v0.1.0") == version("0.1.0"));
    }

    #[test]
    fn newer_release_is_parsed_from_the_github_fixture() {
        let release = newer_release_from_github_json(LATEST_RELEASE_FIXTURE, "0.1.0")
            .unwrap()
            .unwrap();
        assert_eq!(
            release,
            AppRelease {
                version: "0.2.0".to_string(),
                notes: "- Mise à jour intégrée".to_string(),
                asset_url: "https://github.com/Paul-Carouge/riptide/releases/download/v0.2.0/riptide-x86_64-linux".to_string(),
                checksums_url: "https://github.com/Paul-Carouge/riptide/releases/download/v0.2.0/SHA256SUMS".to_string(),
            }
        );
    }

    #[test]
    fn same_or_older_release_is_not_an_update() {
        for running_version in ["0.2.0", "0.2.1", "1.0.0"] {
            assert_eq!(
                newer_release_from_github_json(LATEST_RELEASE_FIXTURE, running_version),
                Ok(None)
            );
        }
    }

    #[test]
    fn release_without_the_expected_assets_is_an_error() {
        let json = br#"{"tag_name":"v9.0.0","body":null,"assets":[
            {"name":"SHA256SUMS","browser_download_url":"https://github.com/Paul-Carouge/riptide/releases/download/v9.0.0/SHA256SUMS"}]}"#;
        assert!(matches!(
            newer_release_from_github_json(json, "0.1.0"),
            Err(EngineError::AppUpdateFailed(_))
        ));
    }

    #[test]
    fn assets_hosted_outside_the_repository_are_refused() {
        let json = br#"{"tag_name":"v9.0.0","body":"","assets":[
            {"name":"riptide-x86_64-linux","browser_download_url":"https://evil.example/riptide"},
            {"name":"SHA256SUMS","browser_download_url":"https://github.com/Paul-Carouge/riptide/releases/download/v9.0.0/SHA256SUMS"}]}"#;
        assert!(matches!(
            newer_release_from_github_json(json, "0.1.0"),
            Err(EngineError::AppUpdateFailed(_))
        ));
    }

    #[test]
    fn malformed_json_or_tag_is_an_error() {
        assert!(newer_release_from_github_json(b"not json", "0.1.0").is_err());
        let json = br#"{"tag_name":"nightly","body":null,"assets":[]}"#;
        assert!(newer_release_from_github_json(json, "0.1.0").is_err());
    }

    #[test]
    fn release_checksums_listing_is_parsed() {
        let hash = "a".repeat(64);
        let listing = format!("{hash}  {RIPTIDE_RELEASE_ASSET_NAME}\n");
        assert_eq!(
            sha256_for_file_in_checksums(&listing, RIPTIDE_RELEASE_ASSET_NAME),
            Some(hash)
        );
    }

    #[test]
    fn directory_writability_is_detected() {
        assert!(is_directory_writable(&std::env::temp_dir()));
        assert!(!is_directory_writable(Path::new("/proc")));
    }
}
