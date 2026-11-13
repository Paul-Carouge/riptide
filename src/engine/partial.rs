//! Per-download working folders and the final, never-overwriting move into the output folder.
//!
//! Each download gets `<output_dir>/.riptide-partial/<id>-<pid>-<n>/`: same file system as the
//! output folder, so the final move is an atomic rename. yt-dlp writes everything there
//! (`.part`, thumbnails, pre-merge streams, the finished file); Riptide then moves the finished
//! file out under a free name and deletes the working folder.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::EngineError;
use super::probe::is_youtube_video_id;

/// Hidden, so file managers and music players skip in-progress downloads.
const PARTIAL_DOWNLOADS_DIR_NAME: &str = ".riptide-partial";
/// Linux `NAME_MAX` for ext4, btrfs, xfs…: a file name longer than this cannot be created.
const MAXIMUM_FILE_NAME_BYTES: usize = 255;
/// Used when a title sanitizes to nothing usable.
const FALLBACK_FILE_STEM: &str = "download";
/// `Title (2).mp3` … `Title (9999).mp3`, then give up with an error.
const MAXIMUM_FREE_NAME_ATTEMPTS: u32 = 9999;

static NEXT_DOWNLOAD_NUMBER: AtomicU64 = AtomicU64::new(0);

/// Deletes working folders left by crashed or killed Riptide instances.
///
/// Looks in `<dir>/.riptide-partial/` for each output folder, and in the pre-1.0 location
/// `$XDG_CACHE_HOME/riptide/partial/`. A folder is deleted only when the process id encoded in
/// its name is dead, so a second running instance keeps its in-progress downloads. Best effort:
/// errors are ignored, a leftover folder is harmless.
pub fn purge_stale_partial_downloads(output_directories: &[PathBuf]) {
    let current_partial_roots = output_directories
        .iter()
        .map(|directory| directory.join(PARTIAL_DOWNLOADS_DIR_NAME));
    let legacy_partial_root =
        dirs::cache_dir().map(|cache_dir| cache_dir.join("riptide").join("partial"));
    for partial_root in current_partial_roots.clone().chain(legacy_partial_root) {
        remove_working_folders_of_dead_processes(&partial_root);
    }
    for partial_root in current_partial_roots {
        // Fails (and is ignored) while another download still uses it.
        let _ = std::fs::remove_dir(&partial_root);
    }
}

fn remove_working_folders_of_dead_processes(partial_root: &Path) {
    let Ok(entries) = std::fs::read_dir(partial_root) else {
        return;
    };
    for entry in entries.flatten() {
        let is_directory = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        let owner_process_id = entry
            .file_name()
            .to_str()
            .and_then(owner_process_id_from_working_folder_name);
        if is_directory
            && let Some(process_id) = owner_process_id
            && !is_process_alive(process_id)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// `<id>-<pid>-<n>` → pid. The video id may itself contain `-`, so parse from the right.
fn owner_process_id_from_working_folder_name(folder_name: &str) -> Option<libc::pid_t> {
    let mut parts = folder_name.rsplitn(3, '-');
    let download_number = parts.next()?;
    let process_id = parts.next()?;
    let video_id = parts.next()?;
    let is_decimal =
        |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let is_well_formed =
        !video_id.is_empty() && is_decimal(download_number) && is_decimal(process_id);
    // pid 0 or negative would address process groups in kill(): never accept them.
    let process_id: libc::pid_t = process_id.parse().ok().filter(|pid| *pid > 0)?;
    is_well_formed.then_some(process_id)
}

fn is_process_alive(process_id: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks that the process exists; nothing is delivered. The caller
    // only passes positive pids, so this never targets a process group.
    let result = unsafe { libc::kill(process_id, 0) };
    // EPERM means it exists but belongs to someone else: alive, keep its folder.
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Creates `<output_dir>/.riptide-partial/<id>-<pid>-<n>/`.
pub(super) fn create_working_folder(
    output_dir: &Path,
    track_id: &str,
) -> Result<PathBuf, EngineError> {
    let partial_root = output_dir.join(PARTIAL_DOWNLOADS_DIR_NAME);
    std::fs::create_dir_all(&partial_root)
        .map_err(|error| EngineError::Io(format!("create temporary folder: {error}")))?;
    let safe_id = if is_youtube_video_id(track_id) {
        track_id
    } else {
        "video"
    };
    let download_number = NEXT_DOWNLOAD_NUMBER.fetch_add(1, Ordering::Relaxed);
    let working_folder = partial_root.join(format!(
        "{safe_id}-{}-{download_number}",
        std::process::id()
    ));
    std::fs::create_dir(&working_folder)
        .map_err(|error| EngineError::Io(format!("create temporary folder: {error}")))?;
    Ok(working_folder)
}

/// Deletes the working folder, then `.riptide-partial` if no other download uses it.
pub(super) fn remove_working_folder(working_folder: &Path) {
    // Best effort: a leftover folder is purged at next start and must not hide the real outcome.
    let _ = std::fs::remove_dir_all(working_folder);
    if let Some(partial_root) = working_folder.parent() {
        let _ = std::fs::remove_dir(partial_root);
    }
}

/// Moves `source` into `destination_dir` as `Name.ext`, else `Name (2).ext`, `Name (3).ext`…
/// Never replaces an existing file, even one created concurrently by another program.
pub(super) fn move_into_folder_without_overwriting(
    source: &Path,
    destination_dir: &Path,
) -> Result<PathBuf, EngineError> {
    let io_error = |error: std::io::Error| EngineError::Io(format!("move finished file: {error}"));
    let source_name = source
        .file_name()
        .ok_or_else(|| EngineError::Io("finished file has no name".to_string()))?
        .to_string_lossy();
    let (stem, extension) = split_file_name_into_stem_and_extension(&source_name);
    let stem = sanitize_file_name_part(stem);
    let extension = extension.map(sanitize_file_name_part);
    for attempt in 1..=MAXIMUM_FREE_NAME_ATTEMPTS {
        let destination =
            destination_dir.join(candidate_file_name(&stem, extension.as_deref(), attempt));
        match rename_without_replacing(source, &destination) {
            Ok(()) => return Ok(destination),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(EngineError::Io(format!(
        "no free file name for {source_name} in {}",
        destination_dir.display()
    )))
}

fn rename_without_replacing(source: &Path, destination: &Path) -> std::io::Result<()> {
    let to_c_string = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
    };
    let (source_c, destination_c) = (to_c_string(source)?, to_c_string(destination)?);
    // SAFETY: both pointers are valid NUL-terminated strings that outlive the call.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source_c.as_ptr(),
            libc::AT_FDCWD,
            destination_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        // File system (or kernel) without RENAME_NOREPLACE: link() also refuses to replace.
        Some(libc::EINVAL | libc::ENOSYS) => {
            std::fs::hard_link(source, destination)?;
            std::fs::remove_file(source)
        }
        _ => Err(error),
    }
}

/// `"Me at the zoo.mp3"` → `("Me at the zoo", Some("mp3"))`. A leading or trailing dot is part
/// of the stem (`.bashrc`, `name.`).
fn split_file_name_into_stem_and_extension(file_name: &str) -> (&str, Option<&str>) {
    match file_name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            (stem, Some(extension))
        }
        _ => (file_name, None),
    }
}

/// Removes what Linux forbids in a name (`/`, NUL) and the special names `.` and `..`.
fn sanitize_file_name_part(part: &str) -> String {
    let cleaned: String = part
        .chars()
        .map(|character| match character {
            '/' | '\0' => '_',
            other => other,
        })
        .collect();
    // Leading dots would make a hidden file in the user's library.
    let cleaned = cleaned.trim().trim_start_matches('.').trim_start();
    if cleaned.is_empty() {
        FALLBACK_FILE_STEM.to_string()
    } else {
        cleaned.to_string()
    }
}

/// Candidate n°`attempt` (1-based): `stem.ext`, then `stem (2).ext`…, cut on a UTF-8 boundary so
/// the whole name fits in `MAXIMUM_FILE_NAME_BYTES`.
fn candidate_file_name(stem: &str, extension: Option<&str>, attempt: u32) -> String {
    let suffix = if attempt <= 1 {
        String::new()
    } else {
        format!(" ({attempt})")
    };
    let extension_part = extension.map_or(String::new(), |extension| format!(".{extension}"));
    let stem_budget = MAXIMUM_FILE_NAME_BYTES
        .saturating_sub(suffix.len() + extension_part.len())
        .max(1);
    let stem = truncate_to_byte_length_on_char_boundary(stem, stem_budget).trim_end();
    let stem = if stem.is_empty() {
        FALLBACK_FILE_STEM
    } else {
        stem
    };
    format!("{stem}{suffix}{extension_part}")
}

fn truncate_to_byte_length_on_char_boundary(text: &str, maximum_bytes: usize) -> &str {
    let mut end = text.len().min(maximum_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "riptide-partial-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A pid that is certainly dead: a child we already reaped.
    fn dead_process_id() -> libc::pid_t {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let process_id = child.id() as libc::pid_t;
        child.wait().unwrap();
        process_id
    }

    #[test]
    fn parses_the_owner_pid_from_the_right() {
        assert_eq!(
            owner_process_id_from_working_folder_name("jNQXAC9IVRw-1234-0"),
            Some(1234)
        );
        assert_eq!(
            owner_process_id_from_working_folder_name("a-b-c_d-77-12"),
            Some(77)
        );
        for invalid in [
            "", "x", "x-1", "-1-2", "x-0-1", "x-+5-1", "x-abc-1", "x-12-", "x-12-a",
        ] {
            assert_eq!(
                owner_process_id_from_working_folder_name(invalid),
                None,
                "{invalid}"
            );
        }
    }

    #[test]
    fn purge_keeps_live_owners_and_removes_dead_ones() {
        let output_dir = scratch_folder("purge");
        let partial_root = output_dir.join(PARTIAL_DOWNLOADS_DIR_NAME);
        let alive = partial_root.join(format!("jNQXAC9IVRw-{}-0", std::process::id()));
        let dead = partial_root.join(format!("jNQXAC9IVRw-{}-0", dead_process_id()));
        let foreign = partial_root.join("not-ours");
        for folder in [&alive, &dead, &foreign] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("x.part"), b"x").unwrap();
        }

        purge_stale_partial_downloads(std::slice::from_ref(&output_dir));
        assert!(alive.is_dir());
        assert!(!dead.exists());
        assert!(foreign.is_dir());

        std::fs::remove_dir_all(&alive).unwrap();
        std::fs::remove_dir_all(&foreign).unwrap();
        purge_stale_partial_downloads(std::slice::from_ref(&output_dir));
        assert!(!partial_root.exists(), "empty root is removed");
        std::fs::remove_dir_all(&output_dir).unwrap();
    }

    #[test]
    fn working_folder_lives_in_the_output_folder_and_is_cleaned() {
        let output_dir = scratch_folder("working");
        let working_folder = create_working_folder(&output_dir, "../../etc").unwrap();
        assert_eq!(
            working_folder.parent().unwrap(),
            output_dir.join(PARTIAL_DOWNLOADS_DIR_NAME)
        );
        let folder_name = working_folder.file_name().unwrap().to_str().unwrap();
        assert!(folder_name.starts_with("video-"), "{folder_name}");
        assert_eq!(
            owner_process_id_from_working_folder_name(folder_name),
            Some(std::process::id() as libc::pid_t)
        );
        remove_working_folder(&working_folder);
        assert!(!output_dir.join(PARTIAL_DOWNLOADS_DIR_NAME).exists());
        std::fs::remove_dir_all(&output_dir).unwrap();
    }

    #[test]
    fn splits_stem_and_extension() {
        assert_eq!(
            split_file_name_into_stem_and_extension("Me at the zoo.mp3"),
            ("Me at the zoo", Some("mp3"))
        );
        assert_eq!(
            split_file_name_into_stem_and_extension("a.b.webm"),
            ("a.b", Some("webm"))
        );
        assert_eq!(
            split_file_name_into_stem_and_extension("noext"),
            ("noext", None)
        );
        assert_eq!(
            split_file_name_into_stem_and_extension(".hidden"),
            (".hidden", None)
        );
        assert_eq!(
            split_file_name_into_stem_and_extension("trailing."),
            ("trailing.", None)
        );
    }

    #[test]
    fn sanitizes_forbidden_and_special_names() {
        assert_eq!(sanitize_file_name_part("AC/DC"), "AC_DC");
        assert_eq!(sanitize_file_name_part("a\0b"), "a_b");
        assert_eq!(sanitize_file_name_part("Café ♫ 日本"), "Café ♫ 日本");
        assert_eq!(sanitize_file_name_part("..hidden mix"), "hidden mix");
        assert_eq!(sanitize_file_name_part("Mr. Brightside"), "Mr. Brightside");
        for unusable in ["", "   ", ".", ".."] {
            assert_eq!(sanitize_file_name_part(unusable), FALLBACK_FILE_STEM);
        }
    }

    #[test]
    fn numbers_candidates_from_the_second_one() {
        assert_eq!(
            candidate_file_name("Me at the zoo", Some("mp3"), 1),
            "Me at the zoo.mp3"
        );
        assert_eq!(
            candidate_file_name("Me at the zoo", Some("mp3"), 2),
            "Me at the zoo (2).mp3"
        );
        assert_eq!(candidate_file_name("x", None, 3), "x (3)");
    }

    #[test]
    fn long_names_are_cut_on_a_char_boundary_within_the_limit() {
        let long_stem = "é".repeat(300);
        for attempt in [1, 2, 9999] {
            let name = candidate_file_name(&long_stem, Some("mp3"), attempt);
            assert!(name.len() <= MAXIMUM_FILE_NAME_BYTES, "{}", name.len());
            assert!(name.ends_with(".mp3"));
        }
        assert!(
            candidate_file_name(&long_stem, Some("mp3"), 2).ends_with("é (2).mp3"),
            "suffix kept after truncation"
        );
    }

    #[test]
    fn move_never_overwrites_and_picks_the_next_free_name() {
        let folder = scratch_folder("move");
        let working = folder.join("work");
        let output = folder.join("out");
        std::fs::create_dir_all(&working).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("Song.mp3"), b"first").unwrap();

        let source = working.join("Song.mp3");
        std::fs::write(&source, b"second").unwrap();
        let moved = move_into_folder_without_overwriting(&source, &output).unwrap();
        assert_eq!(moved, output.join("Song (2).mp3"));
        assert_eq!(std::fs::read(output.join("Song.mp3")).unwrap(), b"first");
        assert_eq!(std::fs::read(&moved).unwrap(), b"second");
        assert!(!source.exists());

        std::fs::write(&source, b"third").unwrap();
        let moved = move_into_folder_without_overwriting(&source, &output).unwrap();
        assert_eq!(moved, output.join("Song (3).mp3"));
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
