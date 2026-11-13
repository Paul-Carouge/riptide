use serde::Deserialize;

use super::EngineError;
use super::error::last_yt_dlp_error_line;
use super::suno::{ALLOWED_SUNO_PAGE_HOSTS, SunoLink, parse_suno_path, probe_suno_link};
use super::tools::yt_dlp_command;

const ALLOWED_YOUTUBE_HOSTS: [&str; 7] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "www.youtube-nocookie.com",
];
const YOUTUBE_VIDEO_ID_LENGTH: usize = 11;
/// Titles YouTube gives to playlist slots that can no longer be downloaded.
const UNAVAILABLE_ENTRY_TITLES: [&str; 2] = ["[Private video]", "[Deleted video]"];

/// Where a validated link points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MediaSource {
    YouTube,
    Suno(SunoLink),
}

/// Checks the input is an http(s) YouTube or Suno link and returns it normalized (https).
pub fn validate_media_url(input: &str) -> Result<String, EngineError> {
    classify_media_url(input).map(|(_, url)| url)
}

pub(super) fn classify_media_url(input: &str) -> Result<(MediaSource, String), EngineError> {
    let trimmed = input.trim();
    let invalid = |reason: &str| EngineError::InvalidUrl(format!("{reason}: {trimmed}"));
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid("contains spaces or control characters"));
    }
    let (scheme, after_scheme) = trimmed
        .split_once("://")
        .ok_or_else(|| invalid("missing http(s)://"))?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(invalid("only http and https are allowed"));
    }
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let (authority, path_and_query) = after_scheme.split_at(authority_end);
    // `https://youtube.com@evil.example/` would really go to evil.example.
    if authority.contains('@') {
        return Err(invalid("credentials are not allowed"));
    }
    let host = match authority.split_once(':') {
        Some((host, port)) => {
            if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
                return Err(invalid("invalid port"));
            }
            host
        }
        None => authority,
    };
    let host = host.to_ascii_lowercase();
    if ALLOWED_SUNO_PAGE_HOSTS.contains(&host.as_str()) {
        let link = parse_suno_path(path_and_query)
            .ok_or_else(|| invalid("not a Suno song link (/song/… or /s/…)"))?;
        let url = link.normalized_url();
        return Ok((MediaSource::Suno(link), url));
    }
    if !ALLOWED_YOUTUBE_HOSTS.contains(&host.as_str()) {
        return Err(invalid("not a YouTube or Suno address"));
    }
    if path_and_query.trim_start_matches('/').is_empty() {
        return Err(invalid("no video or playlist in the link"));
    }
    Ok((
        MediaSource::YouTube,
        format!("https://{host}{path_and_query}"),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Single,
    Playlist,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub uploader: Option<String>,
    pub duration_seconds: Option<u32>,
    pub thumbnail_url: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbedLink {
    pub kind: MediaKind,
    pub title: String,
    pub uploader: Option<String>,
    pub thumbnail_url: Option<String>,
    pub tracks: Vec<Track>,
}

/// YouTube: runs `yt-dlp -J --flat-playlist` on the validated url. Suno: reads the song's
/// public clip JSON (yt-dlp does not support Suno).
pub fn probe_link(url: &str) -> Result<ProbedLink, EngineError> {
    let validated_url = match classify_media_url(url)? {
        (MediaSource::Suno(link), _) => return probe_suno_link(&link),
        (MediaSource::YouTube, url) => url,
    };
    let output = yt_dlp_command()?
        .args(["-J", "--flat-playlist", "--no-warnings", "--use-extractors"])
        .arg(super::download::YOUTUBE_EXTRACTORS)
        .arg("--")
        .arg(&validated_url)
        .output()
        .map_err(|error| EngineError::Io(format!("cannot start yt-dlp: {error}")))?;
    if !output.status.success() {
        return Err(EngineError::ProbeFailed(last_yt_dlp_error_line(
            &String::from_utf8_lossy(&output.stderr),
        )));
    }
    parse_probe_json(&output.stdout)
}

#[derive(Debug, Deserialize)]
struct YtDlpInfo {
    #[serde(rename = "_type")]
    kind: Option<String>,
    id: Option<String>,
    title: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    #[serde(default)]
    thumbnails: Vec<YtDlpThumbnail>,
    entries: Option<Vec<Option<YtDlpInfo>>>,
}

#[derive(Debug, Deserialize)]
struct YtDlpThumbnail {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
    preference: Option<i64>,
}

impl YtDlpInfo {
    fn uploader_or_channel(&self) -> Option<String> {
        self.uploader.clone().or_else(|| self.channel.clone())
    }

    /// yt-dlp's own `thumbnail` pick when present, else the largest listed one.
    fn best_thumbnail_url(&self) -> Option<String> {
        self.thumbnail.clone().or_else(|| {
            self.thumbnails
                .iter()
                .max_by_key(|thumbnail| {
                    let area = u64::from(thumbnail.width.unwrap_or(0))
                        * u64::from(thumbnail.height.unwrap_or(0));
                    (area, thumbnail.preference.unwrap_or(i64::MIN))
                })
                .map(|thumbnail| thumbnail.url.clone())
        })
    }

    /// `None` for entries that are not downloadable videos (channel tabs, private or deleted slots).
    fn to_track(&self) -> Option<Track> {
        let id = self.id.as_deref().filter(|id| is_youtube_video_id(id))?;
        if self
            .title
            .as_deref()
            .is_some_and(|title| UNAVAILABLE_ENTRY_TITLES.contains(&title))
        {
            return None;
        }
        Some(Track {
            id: id.to_string(),
            title: self.title.clone().unwrap_or_else(|| id.to_string()),
            uploader: self.uploader_or_channel(),
            duration_seconds: self
                .duration
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                // Saturating cast: durations never come close to u32::MAX seconds.
                .map(|seconds| seconds.round() as u32),
            thumbnail_url: self
                .best_thumbnail_url()
                .or_else(|| Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"))),
            url: format!("https://www.youtube.com/watch?v={id}"),
        })
    }
}

pub(super) fn is_youtube_video_id(candidate: &str) -> bool {
    candidate.len() == YOUTUBE_VIDEO_ID_LENGTH
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn parse_probe_json(json_bytes: &[u8]) -> Result<ProbedLink, EngineError> {
    let info: YtDlpInfo = serde_json::from_slice(json_bytes)
        .map_err(|error| EngineError::ProbeFailed(format!("unreadable yt-dlp output: {error}")))?;
    let is_playlist = info.kind.as_deref() == Some("playlist") && info.entries.is_some();
    if !is_playlist {
        let track = info.to_track().ok_or_else(|| {
            EngineError::ProbeFailed("the link does not point to a YouTube video".to_string())
        })?;
        return Ok(ProbedLink {
            kind: MediaKind::Single,
            title: track.title.clone(),
            uploader: track.uploader.clone(),
            thumbnail_url: track.thumbnail_url.clone(),
            tracks: vec![track],
        });
    }
    let tracks: Vec<Track> = info
        .entries
        .iter()
        .flatten()
        .flatten()
        .filter_map(YtDlpInfo::to_track)
        .collect();
    let first_track = tracks.first().ok_or_else(|| {
        EngineError::ProbeFailed("the playlist has no downloadable video".to_string())
    })?;
    Ok(ProbedLink {
        kind: MediaKind::Playlist,
        title: info
            .title
            .clone()
            .or_else(|| info.id.clone())
            .unwrap_or_else(|| "Playlist".to_string()),
        uploader: info.uploader_or_channel(),
        thumbnail_url: info
            .best_thumbnail_url()
            .or_else(|| first_track.thumbnail_url.clone()),
        tracks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE_VIDEO_FIXTURE: &str = include_str!("fixtures/probe_single_video.json");
    const PLAYLIST_FIXTURE: &str = include_str!("fixtures/probe_playlist.json");

    fn assert_valid(input: &str, expected: &str) {
        assert_eq!(
            validate_media_url(input).as_deref(),
            Ok(expected),
            "{input}"
        );
    }

    fn assert_invalid(input: &str) {
        assert!(
            matches!(validate_media_url(input), Err(EngineError::InvalidUrl(_))),
            "{input} should be rejected"
        );
    }

    #[test]
    fn accepts_every_youtube_host() {
        assert_valid(
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        );
        assert_valid(
            "https://youtube.com/watch?v=jNQXAC9IVRw",
            "https://youtube.com/watch?v=jNQXAC9IVRw",
        );
        assert_valid(
            "https://m.youtube.com/watch?v=jNQXAC9IVRw",
            "https://m.youtube.com/watch?v=jNQXAC9IVRw",
        );
        assert_valid(
            "https://music.youtube.com/watch?v=jNQXAC9IVRw&si=x",
            "https://music.youtube.com/watch?v=jNQXAC9IVRw&si=x",
        );
        assert_valid(
            "https://youtu.be/jNQXAC9IVRw?t=3",
            "https://youtu.be/jNQXAC9IVRw?t=3",
        );
        assert_valid(
            "https://www.youtube-nocookie.com/embed/jNQXAC9IVRw",
            "https://www.youtube-nocookie.com/embed/jNQXAC9IVRw",
        );
        assert_valid(
            "https://youtube-nocookie.com/embed/jNQXAC9IVRw",
            "https://youtube-nocookie.com/embed/jNQXAC9IVRw",
        );
    }

    #[test]
    fn accepts_shorts_playlists_and_channels() {
        assert_valid(
            "https://www.youtube.com/shorts/abcdefghijk",
            "https://www.youtube.com/shorts/abcdefghijk",
        );
        assert_valid(
            "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
            "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
        );
        assert_valid(
            "https://www.youtube.com/@jawed/videos",
            "https://www.youtube.com/@jawed/videos",
        );
    }

    #[test]
    fn trims_and_normalizes_scheme_and_host() {
        assert_valid(
            "  \thttps://www.youtube.com/watch?v=jNQXAC9IVRw\n",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        );
        assert_valid(
            "http://www.youtube.com/watch?v=jNQXAC9IVRw",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        );
        assert_valid(
            "HTTPS://WWW.YouTube.COM/watch?v=jNQXAC9IVRw",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        );
        assert_valid(
            "https://www.youtube.com:443/watch?v=jNQXAC9IVRw",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        );
    }

    #[test]
    fn keeps_the_path_case_because_ids_are_case_sensitive() {
        assert_valid(
            "https://youtu.be/AbCdEfGhIjK",
            "https://youtu.be/AbCdEfGhIjK",
        );
    }

    #[test]
    fn rejects_other_schemes() {
        assert_invalid("ftp://www.youtube.com/watch?v=jNQXAC9IVRw");
        assert_invalid("file:///etc/passwd");
        assert_invalid("javascript://www.youtube.com/watch?v=x");
        assert_invalid("www.youtube.com/watch?v=jNQXAC9IVRw");
        assert_invalid("//www.youtube.com/watch?v=jNQXAC9IVRw");
    }

    #[test]
    fn rejects_other_hosts_and_lookalikes() {
        assert_invalid("https://vimeo.com/12345");
        assert_invalid("https://youtube.com.evil.example/watch?v=jNQXAC9IVRw");
        assert_invalid("https://evilyoutube.com/watch?v=jNQXAC9IVRw");
        assert_invalid("https://www.youtube.co/watch?v=jNQXAC9IVRw");
        assert_invalid("https://gaming.youtube.com/watch?v=jNQXAC9IVRw");
        assert_invalid("https://youtube.com@evil.example/watch?v=jNQXAC9IVRw");
        assert_invalid("https://user:pass@www.youtube.com/watch?v=jNQXAC9IVRw");
    }

    #[test]
    fn accepts_suno_song_and_short_links() {
        let song = "https://suno.com/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c";
        assert_valid(song, song);
        assert_valid(
            "http://WWW.Suno.com/song/E0EE9438-9ccc-4c4d-ac5b-d38814c55b6c?sh=abc",
            song,
        );
        assert_valid(
            "https://app.suno.ai/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c/",
            song,
        );
        assert_valid(
            "https://suno.com/s/m2J4HaE56D8teUEw",
            "https://suno.com/s/m2J4HaE56D8teUEw",
        );
        assert!(matches!(
            classify_media_url(song),
            Ok((MediaSource::Suno(SunoLink::Song(_)), _))
        ));
        assert!(matches!(
            classify_media_url("https://youtu.be/jNQXAC9IVRw"),
            Ok((MediaSource::YouTube, _))
        ));
    }

    #[test]
    fn rejects_other_suno_pages_and_lookalikes() {
        assert_invalid("https://suno.com/");
        assert_invalid("https://suno.com/explore");
        assert_invalid("https://suno.com/song/not-a-uuid");
        assert_invalid("https://suno.com/@hepaul");
        assert_invalid("https://suno.com/s/a%2Fb");
        assert_invalid("https://suno.ai/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c");
        assert_invalid("https://cdn1.suno.ai/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c.mp4");
        assert_invalid("https://suno.com.evil.example/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c");
        assert_invalid("https://suno.com@evil.example/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c");
        assert_invalid("ftp://suno.com/song/e0ee9438-9ccc-4c4d-ac5b-d38814c55b6c");
    }

    #[test]
    fn rejects_malformed_input() {
        assert_invalid("");
        assert_invalid("   ");
        assert_invalid("not a url");
        assert_invalid("-o /tmp/x https://www.youtube.com/watch?v=jNQXAC9IVRw");
        assert_invalid("https://www.youtube.com/watch?v=a b");
        assert_invalid("https://www.youtube.com:abc/watch?v=jNQXAC9IVRw");
        assert_invalid("https://www.youtube.com:/watch?v=jNQXAC9IVRw");
        assert_invalid("https://www.youtube.com");
        assert_invalid("https://www.youtube.com/");
        assert_invalid("https:///watch?v=jNQXAC9IVRw");
    }

    #[test]
    fn parses_a_single_video_fixture() {
        let probed = parse_probe_json(SINGLE_VIDEO_FIXTURE.as_bytes()).unwrap();
        assert_eq!(probed.kind, MediaKind::Single);
        assert_eq!(probed.title, "Me at the zoo");
        assert_eq!(probed.uploader.as_deref(), Some("jawed"));
        assert_eq!(probed.tracks.len(), 1);
        let track = &probed.tracks[0];
        assert_eq!(track.id, "jNQXAC9IVRw");
        assert_eq!(track.duration_seconds, Some(19));
        assert_eq!(track.url, "https://www.youtube.com/watch?v=jNQXAC9IVRw");
        let thumbnail = track.thumbnail_url.as_deref().unwrap();
        assert!(thumbnail.starts_with("https://i.ytimg.com/vi/jNQXAC9IVRw/"));
        assert_eq!(probed.thumbnail_url, track.thumbnail_url);
    }

    #[test]
    fn parses_a_flat_playlist_fixture() {
        let probed = parse_probe_json(PLAYLIST_FIXTURE.as_bytes()).unwrap();
        assert_eq!(probed.kind, MediaKind::Playlist);
        assert_eq!(probed.title, "Popular Music Videos");
        assert_eq!(probed.tracks.len(), 3);
        let first = &probed.tracks[0];
        assert_eq!(first.id, "fOT0BUpITw8");
        assert_eq!(first.title, "BELLAKEO (Video Oficial) - Peso Pluma, Anitta");
        assert_eq!(first.duration_seconds, Some(235));
        assert_eq!(first.url, "https://www.youtube.com/watch?v=fOT0BUpITw8");
        // The largest of the listed sizes (336x188) wins.
        let thumbnail = first.thumbnail_url.as_deref().unwrap();
        assert!(thumbnail.starts_with("https://i.ytimg.com/vi/fOT0BUpITw8/hqdefault.jpg"));
        assert!(thumbnail.contains("CNACELwB"), "{thumbnail}");
        assert!(probed.thumbnail_url.is_some());
    }

    #[test]
    fn flat_entries_without_thumbnails_get_the_ytimg_fallback() {
        let json = r#"{"_type":"playlist","id":"PLx","title":"Mix","entries":[
            {"_type":"url","id":"jNQXAC9IVRw","title":"Me at the zoo","duration":19.4}
        ]}"#;
        let probed = parse_probe_json(json.as_bytes()).unwrap();
        let track = &probed.tracks[0];
        assert_eq!(
            track.thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg")
        );
        assert_eq!(track.duration_seconds, Some(19));
        assert_eq!(track.uploader, None);
        assert_eq!(probed.thumbnail_url, track.thumbnail_url);
    }

    #[test]
    fn playlists_skip_unavailable_and_non_video_entries() {
        let json = r#"{"_type":"playlist","id":"PLx","title":"Mix","channel":"Someone","entries":[
            {"id":"aaaaaaaaaaa","title":"[Private video]"},
            {"id":"bbbbbbbbbbb","title":"[Deleted video]"},
            null,
            {"id":"UC4QobU6STFB0P71PMvOGN5A","title":"Channel tab"},
            {"id":"ccccccccccc","title":"Kept"}
        ]}"#;
        let probed = parse_probe_json(json.as_bytes()).unwrap();
        assert_eq!(probed.uploader.as_deref(), Some("Someone"));
        let ids: Vec<&str> = probed
            .tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect();
        assert_eq!(ids, ["ccccccccccc"]);
    }

    #[test]
    fn empty_playlists_and_garbage_are_probe_failures() {
        let empty = r#"{"_type":"playlist","id":"PLx","title":"Empty","entries":[]}"#;
        assert!(matches!(
            parse_probe_json(empty.as_bytes()),
            Err(EngineError::ProbeFailed(_))
        ));
        assert!(matches!(
            parse_probe_json(b"not json"),
            Err(EngineError::ProbeFailed(_))
        ));
    }
}
