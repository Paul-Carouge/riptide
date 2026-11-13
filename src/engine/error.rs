/// Every failure the engine can report to the UI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("ffmpeg or ffprobe not found on PATH")]
    FfmpegMissing,
    #[error("yt-dlp download failed: {0}")]
    ToolDownloadFailed(String),
    #[error("yt-dlp checksum mismatch")]
    ToolChecksumMismatch,
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("probe failed: {0}")]
    ProbeFailed(String),
    #[error("download failed: {0}")]
    DownloadFailed(String),
    #[error("i/o error: {0}")]
    Io(String),
    #[error("Riptide update failed: {0}")]
    AppUpdateFailed(String),
    #[error("Riptide update checksum mismatch")]
    AppUpdateChecksumMismatch,
    #[error("Riptide install directory is not writable: {0}")]
    AppInstallDirectoryNotWritable(String),
}

impl EngineError {
    /// Readable French message for the UI.
    pub fn user_message_fr(&self) -> String {
        match self {
            Self::FfmpegMissing => {
                "ffmpeg est introuvable. Installe-le avec le gestionnaire de paquets \
                 (par exemple « sudo pacman -S ffmpeg ») puis relance Riptide."
                    .to_string()
            }
            Self::ToolDownloadFailed(detail) => format!(
                "Impossible de télécharger yt-dlp. Vérifie ta connexion internet. ({detail})"
            ),
            Self::ToolChecksumMismatch => {
                "Le fichier yt-dlp téléchargé est corrompu ou a été modifié \
                 (empreinte SHA-256 incorrecte). Il n'a pas été installé."
                    .to_string()
            }
            Self::InvalidUrl(_) => "Ce lien n'est pas un lien YouTube valide. \
                 Colle l'adresse d'une vidéo, d'un Short ou d'une playlist YouTube."
                .to_string(),
            Self::ProbeFailed(detail) => format!(
                "Impossible de lire ce lien : la vidéo est peut-être privée, supprimée \
                 ou indisponible dans ton pays. ({detail})"
            ),
            Self::DownloadFailed(detail) => format!("Le téléchargement a échoué. ({detail})"),
            Self::Io(detail) => format!("Erreur de fichier ou de processus : {detail}"),
            Self::AppUpdateFailed(detail) => format!(
                "Impossible de mettre à jour Riptide. Vérifie ta connexion internet. ({detail})"
            ),
            Self::AppUpdateChecksumMismatch => {
                "La mise à jour téléchargée est corrompue ou a été modifiée \
                 (empreinte SHA-256 incorrecte). Riptide n'a pas été modifié."
                    .to_string()
            }
            Self::AppInstallDirectoryNotWritable(directory) => format!(
                "Riptide est installé dans « {directory} », un dossier où il ne peut pas écrire. \
                 Mets-le à jour par le moyen utilisé pour l'installer, ou réinstalle-le \
                 dans ~/.local/bin avec le script d'installation."
            ),
        }
    }
}

/// The most useful line of yt-dlp's stderr: the last `ERROR:` line, else the last non-empty one.
pub(super) fn last_yt_dlp_error_line(stderr_text: &str) -> String {
    let lines = || {
        stderr_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
    };
    lines()
        .rev()
        .find_map(|line| line.strip_prefix("ERROR:"))
        .or_else(|| lines().next_back())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "yt-dlp exited without an error message".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_line_prefers_the_last_error_over_warnings() {
        let stderr = "WARNING: slow\nERROR: [youtube] abc: Private video\nWARNING: later\n";
        assert_eq!(
            last_yt_dlp_error_line(stderr),
            "[youtube] abc: Private video"
        );
        assert_eq!(last_yt_dlp_error_line("first\n\nsecond\n  \n"), "second");
        assert_eq!(
            last_yt_dlp_error_line(""),
            "yt-dlp exited without an error message"
        );
    }

    #[test]
    fn every_error_has_a_french_message_that_keeps_the_detail() {
        let detail = "détail technique";
        for error in [
            EngineError::ToolDownloadFailed(detail.into()),
            EngineError::ProbeFailed(detail.into()),
            EngineError::DownloadFailed(detail.into()),
            EngineError::Io(detail.into()),
            EngineError::AppUpdateFailed(detail.into()),
            EngineError::AppInstallDirectoryNotWritable(detail.into()),
        ] {
            assert!(error.user_message_fr().contains(detail), "{error:?}");
        }
        for error in [
            EngineError::FfmpegMissing,
            EngineError::ToolChecksumMismatch,
            EngineError::AppUpdateChecksumMismatch,
            EngineError::InvalidUrl("x".into()),
        ] {
            assert!(!error.user_message_fr().is_empty());
        }
    }
}
