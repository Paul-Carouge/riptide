//! Suno songs. yt-dlp refuses suno.com outright, and Suno serves the song's own audio
//! encrypted, so Riptide reads the public clip JSON itself and downloads the song's public
//! video (`https://cdn1.suno.ai/<uuid>.mp4`, plain AAC audio) through yt-dlp, which then
//! converts, tags and adds the cover like for YouTube.

use serde::Deserialize;
use serde_json::json;

use super::tools::{fetch_bytes, http_agent};
use super::{EngineError, MediaKind, ProbedLink, Track};

pub(super) const ALLOWED_SUNO_PAGE_HOSTS: [&str; 3] = ["suno.com", "www.suno.com", "app.suno.ai"];
/// Suno's image and video CDNs: the only hosts Riptide fetches covers or audio from.
const ALLOWED_SUNO_MEDIA_HOSTS: [&str; 2] = ["cdn1.suno.ai", "cdn2.suno.ai"];
const SUNO_AUDIO_HOST: &str = "cdn1.suno.ai";
/// The JSON the song page itself loads (no account needed for public and shared songs).
const SUNO_CLIP_API_URL: &str = "https://studio-api-prod.suno.com/api/clip/";
const SUNO_SHORT_LINK_URL: &str = "https://suno.com/s/";
const MAXIMUM_CLIP_JSON_BYTES: u64 = 2 * 1024 * 1024;
const MAXIMUM_SHORT_CODE_LENGTH: usize = 64;
const UUID_LENGTH: usize = 36;
const UUID_DASH_POSITIONS: [usize; 4] = [8, 13, 18, 23];

/// A Suno page link, already reduced to what identifies the song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SunoLink {
    /// `/song/<uuid>`, uuid lowercased.
    Song(String),
    /// `/s/<code>`: a share link that redirects to `/song/<uuid>`.
    Short(String),
}

impl SunoLink {
    pub(super) fn normalized_url(&self) -> String {
        match self {
            Self::Song(uuid) => format!("https://suno.com/song/{uuid}"),
            Self::Short(code) => format!("{SUNO_SHORT_LINK_URL}{code}"),
        }
    }
}

pub(super) fn is_suno_uuid(candidate: &str) -> bool {
    candidate.len() == UUID_LENGTH
        && candidate.char_indices().all(|(index, character)| {
            if UUID_DASH_POSITIONS.contains(&index) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

/// Reads `/song/<uuid>` or `/s/<code>` from the path (query and fragment ignored).
pub(super) fn parse_suno_path(path_and_query: &str) -> Option<SunoLink> {
    let path = path_and_query
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    let mut segments = path.strip_prefix('/')?.split('/');
    let (Some(kind), Some(value), None) = (segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    match kind {
        "song" if is_suno_uuid(value) => Some(SunoLink::Song(value.to_ascii_lowercase())),
        "s" if !value.is_empty()
            && value.len() <= MAXIMUM_SHORT_CODE_LENGTH
            && value.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            Some(SunoLink::Short(value.to_string()))
        }
        _ => None,
    }
}

/// The uuid when `url` is exactly `https://cdn1.suno.ai/<uuid>.mp4`, the only Suno URL a
/// download accepts.
pub(super) fn suno_audio_uuid(url: &str) -> Option<&str> {
    let uuid = url
        .strip_prefix("https://")?
        .strip_prefix(SUNO_AUDIO_HOST)?
        .strip_prefix('/')?
        .strip_suffix(".mp4")?;
    (is_suno_uuid(uuid) && uuid == uuid.to_ascii_lowercase()).then_some(uuid)
}

fn is_allowed_suno_media_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    ALLOWED_SUNO_MEDIA_HOSTS.contains(&host)
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Blocking: resolves a short link if needed, then reads the song's public clip JSON.
pub(super) fn probe_suno_link(link: &SunoLink) -> Result<ProbedLink, EngineError> {
    let uuid = match link {
        SunoLink::Song(uuid) => uuid.clone(),
        SunoLink::Short(code) => resolve_short_link(code)?,
    };
    let clip_json = fetch_bytes(
        &http_agent(),
        &format!("{SUNO_CLIP_API_URL}{uuid}"),
        MAXIMUM_CLIP_JSON_BYTES,
    )
    .map_err(|error| EngineError::ProbeFailed(format!("Suno: {error}")))?;
    let track = parse_suno_clip_json(&clip_json, &uuid)?;
    Ok(ProbedLink {
        kind: MediaKind::Single,
        title: track.title.clone(),
        uploader: track.uploader.clone(),
        thumbnail_url: track.thumbnail_url.clone(),
        tracks: vec![track],
    })
}

/// `https://suno.com/s/<code>` answers a redirect to `/song/<uuid>?sh=<code>`; it is read,
/// never followed, so the request cannot leave suno.com.
fn resolve_short_link(code: &str) -> Result<String, EngineError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into();
    let response = agent
        .get(&format!("{SUNO_SHORT_LINK_URL}{code}"))
        .call()
        .map_err(|error| EngineError::ProbeFailed(format!("Suno short link: {error}")))?;
    response
        .headers()
        .get("location")
        .and_then(|location| location.to_str().ok())
        .and_then(uuid_from_redirect_location)
        .ok_or_else(|| {
            EngineError::ProbeFailed("Suno short link does not lead to a song".to_string())
        })
}

fn uuid_from_redirect_location(location: &str) -> Option<String> {
    let path = location
        .strip_prefix("https://suno.com")
        .or_else(|| location.strip_prefix("https://www.suno.com"))
        .unwrap_or(location);
    match parse_suno_path(path)? {
        SunoLink::Song(uuid) => Some(uuid),
        SunoLink::Short(_) => None,
    }
}

#[derive(Debug, Deserialize)]
struct SunoClip {
    id: String,
    title: Option<String>,
    display_name: Option<String>,
    handle: Option<String>,
    image_large_url: Option<String>,
    image_url: Option<String>,
    video_url: Option<String>,
    metadata: Option<SunoClipMetadata>,
}

#[derive(Debug, Deserialize)]
struct SunoClipMetadata {
    duration: Option<f64>,
}

fn parse_suno_clip_json(json_bytes: &[u8], expected_uuid: &str) -> Result<Track, EngineError> {
    let clip: SunoClip = serde_json::from_slice(json_bytes)
        .map_err(|error| EngineError::ProbeFailed(format!("unreadable Suno answer: {error}")))?;
    if !clip.id.eq_ignore_ascii_case(expected_uuid) {
        return Err(EngineError::ProbeFailed(
            "Suno answered for another song".to_string(),
        ));
    }
    let audio_url = clip
        .video_url
        .filter(|url| suno_audio_uuid(url) == Some(expected_uuid))
        .ok_or(EngineError::SunoSongNotDownloadable)?;
    let title = clip
        .title
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| expected_uuid.to_string());
    Ok(Track {
        id: expected_uuid.to_string(),
        title,
        uploader: clip
            .display_name
            .or(clip.handle)
            .filter(|name| !name.trim().is_empty()),
        duration_seconds: clip
            .metadata
            .and_then(|metadata| metadata.duration)
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            // Saturating cast: songs never come close to u32::MAX seconds.
            .map(|seconds| seconds.round() as u32),
        thumbnail_url: clip
            .image_large_url
            .into_iter()
            .chain(clip.image_url)
            .find(|url| is_allowed_suno_media_url(url)),
        url: audio_url,
    })
}

/// The `--load-info-json` file for a Suno download: yt-dlp gets the title, artist and cover
/// Riptide read from Suno, and fetches only the validated CDN URL.
pub(super) fn yt_dlp_info_json(track: &Track, audio_url: &str, uuid: &str) -> String {
    let thumbnails: Vec<_> = track
        .thumbnail_url
        .iter()
        .filter(|url| is_allowed_suno_media_url(url))
        .map(|url| json!({ "url": url }))
        .collect();
    json!({
        "_type": "video",
        "id": uuid,
        "title": track.title,
        "artist": track.uploader,
        "uploader": track.uploader,
        "duration": track.duration_seconds,
        "thumbnails": thumbnails,
        "url": audio_url,
        "ext": "mp4",
        "webpage_url": audio_url,
        "extractor": "generic",
        "extractor_key": "Generic",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC_CLIP_FIXTURE: &str = include_str!("fixtures/suno_public_clip.json");
    const PUBLIC_UUID: &str = "0ee28dcc-929d-410b-a60c-6f5076b46f15";

    #[test]
    fn reads_song_and_short_paths() {
        assert_eq!(
            parse_suno_path("/song/0EE28DCC-929d-410b-a60c-6f5076b46f15/?sh=x"),
            Some(SunoLink::Song(PUBLIC_UUID.to_string()))
        );
        assert_eq!(
            parse_suno_path("/s/m2J4HaE56D8teUEw"),
            Some(SunoLink::Short("m2J4HaE56D8teUEw".to_string()))
        );
        for rejected in [
            "/",
            "/song/",
            "/song/not-a-uuid",
            "/song/0ee28dcc929d410ba60c6f5076b46f15xxxx",
            "/song/0ee28dcc-929d-410b-a60c-6f5076b46f15/extra",
            "/s/",
            "/s/a-b",
            "/playlist/0ee28dcc-929d-410b-a60c-6f5076b46f15",
            "/@someone",
        ] {
            assert_eq!(parse_suno_path(rejected), None, "{rejected}");
        }
    }

    #[test]
    fn only_the_exact_cdn_video_url_is_a_download_url() {
        let url = format!("https://cdn1.suno.ai/{PUBLIC_UUID}.mp4");
        assert_eq!(suno_audio_uuid(&url), Some(PUBLIC_UUID));
        for rejected in [
            format!("http://cdn1.suno.ai/{PUBLIC_UUID}.mp4"),
            format!("https://cdn2.suno.ai/{PUBLIC_UUID}.mp4"),
            format!("https://cdn1.suno.ai.evil.example/{PUBLIC_UUID}.mp4"),
            format!("https://cdn1.suno.ai@evil.example/{PUBLIC_UUID}.mp4"),
            format!("https://cdn1.suno.ai/{PUBLIC_UUID}.mp3"),
            format!("https://cdn1.suno.ai/x/{PUBLIC_UUID}.mp4"),
            format!("https://cdn1.suno.ai/{PUBLIC_UUID}.mp4?x=1"),
            "https://cdn1.suno.ai/0EE28DCC-929d-410b-a60c-6f5076b46f15.mp4".to_string(),
            "https://suno.com/song/0ee28dcc-929d-410b-a60c-6f5076b46f15".to_string(),
        ] {
            assert_eq!(suno_audio_uuid(&rejected), None, "{rejected}");
        }
    }

    #[test]
    fn short_link_redirect_gives_the_uuid() {
        assert_eq!(
            uuid_from_redirect_location(
                "/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c?sh=m2J4HaE56D8teUEw"
            )
            .as_deref(),
            Some("e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c")
        );
        assert_eq!(
            uuid_from_redirect_location(
                "https://suno.com/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c"
            )
            .as_deref(),
            Some("e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c")
        );
        assert_eq!(
            uuid_from_redirect_location(
                "https://evil.example/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c"
            ),
            None
        );
        assert_eq!(uuid_from_redirect_location("/login"), None);
    }

    #[test]
    fn parses_a_real_public_clip() {
        let track = parse_suno_clip_json(PUBLIC_CLIP_FIXTURE.as_bytes(), PUBLIC_UUID).unwrap();
        assert_eq!(track.id, PUBLIC_UUID);
        assert_eq!(track.title, ".sound 9.");
        assert_eq!(track.uploader.as_deref(), Some(".fujiro."));
        assert_eq!(track.duration_seconds, Some(116));
        assert_eq!(
            track.thumbnail_url.as_deref(),
            Some("https://cdn2.suno.ai/aa392845-06ce-40ad-a2df-3de4b328f1a8.jpeg")
        );
        assert_eq!(track.url, format!("https://cdn1.suno.ai/{PUBLIC_UUID}.mp4"));
    }

    #[test]
    fn songs_without_a_public_video_are_not_downloadable() {
        let private = PUBLIC_CLIP_FIXTURE.replace(
            "\"https://cdn1.suno.ai/0ee28dcc-929d-410b-a60c-6f5076b46f15.mp4\"",
            "\"\"",
        );
        assert_eq!(
            parse_suno_clip_json(private.as_bytes(), PUBLIC_UUID),
            Err(EngineError::SunoSongNotDownloadable)
        );
        let foreign = PUBLIC_CLIP_FIXTURE.replace(
            "\"https://cdn1.suno.ai/0ee28dcc-929d-410b-a60c-6f5076b46f15.mp4\"",
            "\"https://evil.example/x.mp4\"",
        );
        assert_eq!(
            parse_suno_clip_json(foreign.as_bytes(), PUBLIC_UUID),
            Err(EngineError::SunoSongNotDownloadable)
        );
    }

    #[test]
    fn answers_for_another_song_or_garbage_are_refused() {
        assert!(matches!(
            parse_suno_clip_json(
                PUBLIC_CLIP_FIXTURE.as_bytes(),
                "e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c"
            ),
            Err(EngineError::ProbeFailed(_))
        ));
        assert!(matches!(
            parse_suno_clip_json(b"<html>", PUBLIC_UUID),
            Err(EngineError::ProbeFailed(_))
        ));
    }

    #[test]
    fn info_json_carries_the_metadata_and_only_allowed_covers() {
        let mut track = parse_suno_clip_json(PUBLIC_CLIP_FIXTURE.as_bytes(), PUBLIC_UUID).unwrap();
        let info: serde_json::Value =
            serde_json::from_str(&yt_dlp_info_json(&track, &track.url, PUBLIC_UUID)).unwrap();
        assert_eq!(info["title"], ".sound 9.");
        assert_eq!(info["artist"], ".fujiro.");
        assert_eq!(info["url"], track.url.as_str());
        assert_eq!(info["thumbnails"].as_array().map(Vec::len), Some(1));
        track.thumbnail_url = Some("https://evil.example/x.jpg".to_string());
        let info: serde_json::Value =
            serde_json::from_str(&yt_dlp_info_json(&track, &track.url, PUBLIC_UUID)).unwrap();
        assert_eq!(info["thumbnails"].as_array().map(Vec::len), Some(0));
    }
}
