use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat {
    Mp3,
    M4a,
    Opus,
    Flac,
    Wav,
    /// OGG Vorbis.
    Ogg,
}
impl AudioFormat {
    pub const ALL: [AudioFormat; 6] = [
        Self::Mp3,
        Self::M4a,
        Self::Opus,
        Self::Ogg,
        Self::Flac,
        Self::Wav,
    ];
}

/// Ignored for lossless formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioQuality {
    Best,
    Kbps320,
    Kbps256,
    Kbps192,
    Kbps128,
}
impl AudioQuality {
    pub const ALL: [AudioQuality; 5] = [
        Self::Best,
        Self::Kbps320,
        Self::Kbps256,
        Self::Kbps192,
        Self::Kbps128,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoFormat {
    Mp4,
    Webm,
}
impl VideoFormat {
    pub const ALL: [VideoFormat; 2] = [Self::Mp4, Self::Webm];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoQuality {
    Best,
    P2160,
    P1440,
    P1080,
    P720,
    P480,
}
impl VideoQuality {
    pub const ALL: [VideoQuality; 6] = [
        Self::Best,
        Self::P2160,
        Self::P1440,
        Self::P1080,
        Self::P720,
        Self::P480,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputPreset {
    Audio(AudioFormat, AudioQuality),
    Video(VideoFormat, VideoQuality),
}

impl AudioFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::M4a => "M4A",
            Self::Opus => "OPUS",
            Self::Flac => "FLAC",
            Self::Wav => "WAV",
            Self::Ogg => "OGG",
        }
    }

    /// Value of yt-dlp's `--audio-format`.
    fn yt_dlp_name(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Opus => "opus",
            Self::Flac => "flac",
            Self::Wav => "wav",
            // yt-dlp names the codec; the file gets the `.ogg` extension.
            Self::Ogg => "vorbis",
        }
    }

    fn is_lossless(self) -> bool {
        matches!(self, Self::Flac | Self::Wav)
    }

    /// yt-dlp can embed cover art in every audio container except WAV (OGG via mutagen).
    pub(super) fn supports_embedded_cover(self) -> bool {
        self != Self::Wav
    }
}

impl AudioQuality {
    /// Value of yt-dlp's `--audio-quality`: 0 is the best VBR setting.
    fn yt_dlp_value(self) -> &'static str {
        match self {
            Self::Best => "0",
            Self::Kbps320 => "320K",
            Self::Kbps256 => "256K",
            Self::Kbps192 => "192K",
            Self::Kbps128 => "128K",
        }
    }

    fn label_fr(self) -> &'static str {
        match self {
            Self::Best => "meilleure qualité",
            Self::Kbps320 => "320 kbps",
            Self::Kbps256 => "256 kbps",
            Self::Kbps192 => "192 kbps",
            Self::Kbps128 => "128 kbps",
        }
    }
}

impl VideoFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Webm => "WEBM",
        }
    }

    /// yt-dlp can embed a thumbnail in MP4 but not in WebM.
    pub(super) fn supports_embedded_cover(self) -> bool {
        self == Self::Mp4
    }
}

impl VideoQuality {
    fn maximum_height(self) -> Option<u32> {
        match self {
            Self::Best => None,
            Self::P2160 => Some(2160),
            Self::P1440 => Some(1440),
            Self::P1080 => Some(1080),
            Self::P720 => Some(720),
            Self::P480 => Some(480),
        }
    }
}

impl OutputPreset {
    pub fn label_fr(&self) -> String {
        match *self {
            Self::Audio(format, _) if format.is_lossless() => {
                format!("{} · sans perte", format.label())
            }
            Self::Audio(format, quality) => format!("{} · {}", format.label(), quality.label_fr()),
            Self::Video(format, quality) => match quality.maximum_height() {
                Some(height) => format!("{} · {height}p", format.label()),
                None => format!("{} · meilleure qualité", format.label()),
            },
        }
    }

    pub fn is_lossless(&self) -> bool {
        matches!(self, Self::Audio(format, _) if format.is_lossless())
    }

    pub(super) fn supports_embedded_cover(&self) -> bool {
        match self {
            Self::Audio(format, _) => format.supports_embedded_cover(),
            Self::Video(format, _) => format.supports_embedded_cover(),
        }
    }

    /// yt-dlp arguments that select streams and convert them to this preset.
    pub(super) fn yt_dlp_format_arguments(&self) -> Vec<String> {
        match *self {
            Self::Audio(format, quality) => {
                let mut arguments = vec![
                    "-f".to_string(),
                    "ba/b".to_string(),
                    "-x".to_string(),
                    "--audio-format".to_string(),
                    format.yt_dlp_name().to_string(),
                ];
                if !format.is_lossless() {
                    arguments.push("--audio-quality".to_string());
                    arguments.push(quality.yt_dlp_value().to_string());
                }
                arguments
            }
            Self::Video(format, quality) => {
                let height_filter = quality
                    .maximum_height()
                    .map(|height| format!("[height<={height}]"))
                    .unwrap_or_default();
                let resolution_sort = quality
                    .maximum_height()
                    .map(|height| format!("res:{height}"))
                    .unwrap_or_else(|| "res".to_string());
                // Highest allowed resolution first, then the codecs the container plays best.
                let (selector, sort, container, conversion_flag) = match format {
                    VideoFormat::Mp4 => (
                        format!(
                            "bv*{height_filter}[vcodec^=avc1]+ba[ext=m4a]/bv*{height_filter}+ba[ext=m4a]/bv*{height_filter}+ba/b{height_filter}/b"
                        ),
                        format!("{resolution_sort},vcodec:h264,acodec:aac"),
                        "mp4",
                        "--remux-video",
                    ),
                    VideoFormat::Webm => (
                        format!(
                            "bv*{height_filter}[ext=webm]+ba[ext=webm]/bv*{height_filter}+ba/b{height_filter}/b"
                        ),
                        format!("{resolution_sort},vcodec:vp9,acodec:opus"),
                        "webm",
                        // WebM only holds VP8/VP9/AV1 + Vorbis/Opus, so a fallback stream must be re-encoded.
                        "--recode-video",
                    ),
                };
                vec![
                    "-f".to_string(),
                    selector,
                    "-S".to_string(),
                    sort,
                    "--merge-output-format".to_string(),
                    container.to_string(),
                    conversion_flag.to_string(),
                    container.to_string(),
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_presets() -> Vec<OutputPreset> {
        let audio = AudioFormat::ALL.iter().flat_map(|format| {
            AudioQuality::ALL
                .iter()
                .map(move |quality| OutputPreset::Audio(*format, *quality))
        });
        let video = VideoFormat::ALL.iter().flat_map(|format| {
            VideoQuality::ALL
                .iter()
                .map(move |quality| OutputPreset::Video(*format, *quality))
        });
        audio.chain(video).collect()
    }

    fn value_after<'a>(arguments: &'a [String], flag: &str) -> Option<&'a str> {
        let position = arguments.iter().position(|argument| argument == flag)?;
        arguments.get(position + 1).map(String::as_str)
    }

    #[test]
    fn labels_are_readable_french() {
        let mp3 = OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320);
        assert_eq!(mp3.label_fr(), "MP3 · 320 kbps");
        let opus = OutputPreset::Audio(AudioFormat::Opus, AudioQuality::Best);
        assert_eq!(opus.label_fr(), "OPUS · meilleure qualité");
        let flac = OutputPreset::Audio(AudioFormat::Flac, AudioQuality::Kbps128);
        assert_eq!(flac.label_fr(), "FLAC · sans perte");
        let mp4 = OutputPreset::Video(VideoFormat::Mp4, VideoQuality::P1080);
        assert_eq!(mp4.label_fr(), "MP4 · 1080p");
        let webm = OutputPreset::Video(VideoFormat::Webm, VideoQuality::Best);
        assert_eq!(webm.label_fr(), "WEBM · meilleure qualité");
    }

    #[test]
    fn only_flac_and_wav_are_lossless() {
        for preset in all_presets() {
            let expected = matches!(
                preset,
                OutputPreset::Audio(AudioFormat::Flac | AudioFormat::Wav, _)
            );
            assert_eq!(preset.is_lossless(), expected, "{preset:?}");
        }
    }

    #[test]
    fn audio_presets_extract_audio_with_the_requested_quality() {
        let arguments =
            OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320).yt_dlp_format_arguments();
        assert!(arguments.contains(&"-x".to_string()));
        assert_eq!(value_after(&arguments, "--audio-format"), Some("mp3"));
        assert_eq!(value_after(&arguments, "--audio-quality"), Some("320K"));

        let best =
            OutputPreset::Audio(AudioFormat::M4a, AudioQuality::Best).yt_dlp_format_arguments();
        assert_eq!(value_after(&best, "--audio-quality"), Some("0"));
    }

    #[test]
    fn ogg_is_vorbis_with_a_bitrate_and_a_cover() {
        let preset = OutputPreset::Audio(AudioFormat::Ogg, AudioQuality::Kbps320);
        assert_eq!(preset.label_fr(), "OGG · 320 kbps");
        assert!(!preset.is_lossless());
        assert!(preset.supports_embedded_cover());
        let arguments = preset.yt_dlp_format_arguments();
        assert!(arguments.contains(&"-x".to_string()));
        assert_eq!(value_after(&arguments, "--audio-format"), Some("vorbis"));
        assert_eq!(value_after(&arguments, "--audio-quality"), Some("320K"));
    }

    #[test]
    fn settings_written_before_ogg_still_load() {
        let old: OutputPreset = serde_json::from_str(r#"{"Audio":["Opus","Kbps192"]}"#).unwrap();
        assert_eq!(
            old,
            OutputPreset::Audio(AudioFormat::Opus, AudioQuality::Kbps192)
        );
        let ogg = OutputPreset::Audio(AudioFormat::Ogg, AudioQuality::Best);
        let round_trip: OutputPreset =
            serde_json::from_str(&serde_json::to_string(&ogg).unwrap()).unwrap();
        assert_eq!(round_trip, ogg);
    }

    #[test]
    fn lossless_audio_ignores_the_quality() {
        let arguments =
            OutputPreset::Audio(AudioFormat::Flac, AudioQuality::Kbps128).yt_dlp_format_arguments();
        assert_eq!(value_after(&arguments, "--audio-format"), Some("flac"));
        assert_eq!(value_after(&arguments, "--audio-quality"), None);
    }

    #[test]
    fn mp4_prefers_avc1_and_m4a_under_the_height_limit() {
        let arguments =
            OutputPreset::Video(VideoFormat::Mp4, VideoQuality::P720).yt_dlp_format_arguments();
        let selector = value_after(&arguments, "-f").unwrap_or_default();
        assert!(selector.starts_with("bv*[height<=720][vcodec^=avc1]+ba[ext=m4a]/"));
        assert!(
            selector.ends_with("/b"),
            "a last-resort fallback keeps the download working"
        );
        assert_eq!(
            value_after(&arguments, "-S"),
            Some("res:720,vcodec:h264,acodec:aac")
        );
        assert_eq!(
            value_after(&arguments, "--merge-output-format"),
            Some("mp4")
        );
        assert_eq!(value_after(&arguments, "--remux-video"), Some("mp4"));
        assert!(!arguments.contains(&"-x".to_string()));
    }

    #[test]
    fn best_video_has_no_height_limit() {
        let arguments =
            OutputPreset::Video(VideoFormat::Webm, VideoQuality::Best).yt_dlp_format_arguments();
        let selector = value_after(&arguments, "-f").unwrap_or_default();
        assert!(!selector.contains("height"));
        assert_eq!(
            value_after(&arguments, "-S"),
            Some("res,vcodec:vp9,acodec:opus")
        );
        assert_eq!(
            value_after(&arguments, "--merge-output-format"),
            Some("webm")
        );
        assert_eq!(value_after(&arguments, "--recode-video"), Some("webm"));
    }

    #[test]
    fn every_preset_has_a_label_and_a_format_selector() {
        for preset in all_presets() {
            assert!(!preset.label_fr().is_empty());
            assert!(value_after(&preset.yt_dlp_format_arguments(), "-f").is_some());
        }
    }

    #[test]
    fn cover_embedding_is_skipped_for_wav_and_webm() {
        assert!(
            !OutputPreset::Audio(AudioFormat::Wav, AudioQuality::Best).supports_embedded_cover()
        );
        assert!(
            !OutputPreset::Video(VideoFormat::Webm, VideoQuality::Best).supports_embedded_cover()
        );
        assert!(
            OutputPreset::Audio(AudioFormat::Opus, AudioQuality::Best).supports_embedded_cover()
        );
        assert!(
            OutputPreset::Video(VideoFormat::Mp4, VideoQuality::P720).supports_embedded_cover()
        );
    }
}
