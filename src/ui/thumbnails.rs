//! Thumbnail fetching. GPUI's default HTTP client is a no-op, so the app downloads the
//! bytes itself (background thread, ureq) and hands `gpui::Image` to `img()`.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gpui::{Image, ImageFormat};

const MAXIMUM_THUMBNAIL_BYTES: u64 = 4 * 1024 * 1024;
const THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(15);
/// Simultaneous thumbnail requests; the rest wait (newest first, i.e. the rows on screen).
pub const MAXIMUM_CONCURRENT_THUMBNAIL_REQUESTS: usize = 6;
/// YouTube's image CDNs. Any other host is refused (the URL comes from yt-dlp's JSON).
const ALLOWED_THUMBNAIL_HOST_SUFFIXES: [&str; 3] =
    ["ytimg.com", "ggpht.com", "googleusercontent.com"];

#[derive(Clone)]
pub enum ThumbnailSlot {
    /// Requested, waiting for a free request slot.
    Pending,
    Loading,
    Ready(Arc<Image>),
    Failed,
}

/// Thumbnail images by URL, plus the scheduling of their downloads (at most
/// [`MAXIMUM_CONCURRENT_THUMBNAIL_REQUESTS`] at once).
#[derive(Default)]
pub struct ThumbnailCache {
    slots: HashMap<String, ThumbnailSlot>,
    /// Requested but not started, oldest first; served from the end (most recent request).
    waiting_urls: Vec<String>,
    loading_count: usize,
}

impl ThumbnailCache {
    pub fn image(&self, url: &str) -> Option<Arc<Image>> {
        match self.slots.get(url) {
            Some(ThumbnailSlot::Ready(image)) => Some(image.clone()),
            _ => None,
        }
    }

    /// Records a request for `url` (ignored when already known). Returns the URLs that may
    /// start downloading now.
    pub fn request(&mut self, url: &str) -> Vec<String> {
        match self.slots.get(url) {
            None => {
                self.slots.insert(url.to_owned(), ThumbnailSlot::Pending);
                self.waiting_urls.push(url.to_owned());
            }
            Some(ThumbnailSlot::Pending) => {
                // Seen again (row back on screen): serve it next.
                if let Some(position) = self.waiting_urls.iter().position(|waiting| waiting == url)
                {
                    let url = self.waiting_urls.remove(position);
                    self.waiting_urls.push(url);
                }
            }
            Some(_) => {}
        }
        self.urls_ready_to_start()
    }

    /// Stores a finished download and returns the URLs that may start next.
    pub fn finish_loading(&mut self, url: String, result: Result<Image, String>) -> Vec<String> {
        let slot = match result {
            Ok(image) => ThumbnailSlot::Ready(Arc::new(image)),
            Err(error) => {
                eprintln!("riptide: thumbnail unavailable: {error}");
                ThumbnailSlot::Failed
            }
        };
        self.slots.insert(url, slot);
        self.loading_count = self.loading_count.saturating_sub(1);
        self.urls_ready_to_start()
    }

    fn urls_ready_to_start(&mut self) -> Vec<String> {
        let mut starting = Vec::new();
        while self.loading_count < MAXIMUM_CONCURRENT_THUMBNAIL_REQUESTS
            && let Some(url) = self.waiting_urls.pop()
        {
            self.slots.insert(url.clone(), ThumbnailSlot::Loading);
            self.loading_count += 1;
            starting.push(url);
        }
        starting
    }
}

/// One HTTP agent for every thumbnail (keeps TLS connections alive between requests).
fn shared_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(THUMBNAIL_TIMEOUT))
            .https_only(true)
            // A redirect could leave the allowed hosts: refuse it.
            .max_redirects(0)
            .build()
            .into()
    })
}

/// True for `https://<host>/…` where host is one of YouTube's image CDNs (or a subdomain).
pub fn is_allowed_thumbnail_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // No credentials, no explicit port: a plain host name only.
    if authority.contains(['@', ':']) {
        return false;
    }
    let host = authority.to_ascii_lowercase();
    ALLOWED_THUMBNAIL_HOST_SUFFIXES.iter().any(|suffix| {
        host == *suffix
            || host
                .strip_suffix(suffix)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

/// Blocking: call from a background task. Only https URLs on YouTube image hosts are fetched.
pub fn fetch_thumbnail(url: &str) -> Result<Image, String> {
    if !is_allowed_thumbnail_url(url) {
        return Err("thumbnail url is not https on a YouTube image host".to_owned());
    }
    let mut response = shared_agent()
        .get(url)
        .call()
        .map_err(|error| error.to_string())?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAXIMUM_THUMBNAIL_BYTES)
        .read_to_vec()
        .map_err(|error| error.to_string())?;
    let format = sniff_image_format(&bytes).ok_or("unsupported thumbnail format")?;
    Ok(Image::from_bytes(format, bytes))
}

/// Detects the format from magic bytes (servers sometimes lie about content types).
pub fn sniff_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some(ImageFormat::Jpeg),
        [0x89, b'P', b'N', b'G', ..] => Some(ImageFormat::Png),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageFormat::Webp),
        [b'G', b'I', b'F', b'8', ..] => Some(ImageFormat::Gif),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_common_formats() {
        assert_eq!(
            sniff_image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(sniff_image_format(b"\x89PNG\r\n"), Some(ImageFormat::Png));
        assert_eq!(
            sniff_image_format(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::Webp)
        );
        assert_eq!(sniff_image_format(b"<html>"), None);
        assert_eq!(sniff_image_format(&[]), None);
    }

    #[test]
    fn refuses_plain_http() {
        assert!(fetch_thumbnail("http://i.ytimg.com/vi/x/hq.jpg").is_err());
    }

    #[test]
    fn only_youtube_image_hosts_are_allowed() {
        assert!(is_allowed_thumbnail_url(
            "https://i.ytimg.com/vi/x/hqdefault.jpg"
        ));
        assert!(is_allowed_thumbnail_url("https://yt3.ggpht.com/a/b=s88"));
        assert!(is_allowed_thumbnail_url(
            "https://lh3.googleusercontent.com/x"
        ));
        assert!(is_allowed_thumbnail_url("https://I.YTIMG.COM/vi/x.jpg"));
        assert!(!is_allowed_thumbnail_url("https://evilytimg.com/x.jpg"));
        assert!(!is_allowed_thumbnail_url(
            "https://i.ytimg.com.evil.net/x.jpg"
        ));
        assert!(!is_allowed_thumbnail_url(
            "https://i.ytimg.com@evil.net/x.jpg"
        ));
        assert!(!is_allowed_thumbnail_url("https://i.ytimg.com:8443/x.jpg"));
        assert!(!is_allowed_thumbnail_url("https://example.com/i.ytimg.com"));
        assert!(!is_allowed_thumbnail_url("ftp://i.ytimg.com/x"));
    }

    #[test]
    fn at_most_six_requests_run_and_newest_waits_first() {
        let mut cache = ThumbnailCache::default();
        let mut started = Vec::new();
        for index in 0..10 {
            started.extend(cache.request(&format!("https://i.ytimg.com/{index}")));
        }
        assert_eq!(started.len(), MAXIMUM_CONCURRENT_THUMBNAIL_REQUESTS);
        assert!(cache.request("https://i.ytimg.com/0").is_empty());
        let next = cache.finish_loading(started[0].clone(), Err("404".to_owned()));
        assert_eq!(next, vec!["https://i.ytimg.com/9".to_owned()]);
    }
}
