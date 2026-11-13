//! French display strings for numbers, durations, dates and preset parts.

use crate::engine::{AudioFormat, AudioQuality, VideoFormat, VideoQuality};

const BYTES_PER_MEGABYTE: f64 = 1_000_000.0;
const SECONDS_PER_DAY: u64 = 86_400;
const FRENCH_MONTH_ABBREVIATIONS: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

/// `3:07`, or `1:02:03` past an hour.
pub fn track_duration(total_seconds: u32) -> String {
    let hours = total_seconds / 3600;
    let minutes = total_seconds % 3600 / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `2,4 Mo/s` (French decimal comma).
pub fn speed_megabytes_per_second(bytes_per_second: u64) -> String {
    let megabytes = bytes_per_second as f64 / BYTES_PER_MEGABYTE;
    format!("{megabytes:.1} Mo/s").replace('.', ",")
}

/// `reste 42 s`, `reste 3 min 05 s`, `reste 1 h 02 min`.
pub fn remaining_time(eta_seconds: u32) -> String {
    let hours = eta_seconds / 3600;
    let minutes = eta_seconds % 3600 / 60;
    let seconds = eta_seconds % 60;
    if hours > 0 {
        format!("reste {hours} h {minutes:02} min")
    } else if minutes > 0 {
        format!("reste {minutes} min {seconds:02} s")
    } else {
        format!("reste {seconds} s")
    }
}

pub fn percent(fraction: f32) -> String {
    format!("{} %", (fraction.clamp(0.0, 1.0) * 100.0).floor() as u32)
}

/// `13 nov. 2026` (UTC date).
pub fn calendar_date(unix_seconds: u64) -> String {
    // shortcut: UTC date, no local time zone; good enough for a day-level history label.
    let (year, month, day) = civil_date_from_days((unix_seconds / SECONDS_PER_DAY) as i64);
    let month_name = FRENCH_MONTH_ABBREVIATIONS[(month - 1) as usize];
    format!("{day} {month_name} {year}")
}

/// Howard Hinnant's days-from-epoch to proleptic Gregorian (year, month 1-12, day 1-31).
fn civil_date_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let shifted = days_since_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

pub fn audio_format_name(format: AudioFormat) -> &'static str {
    match format {
        AudioFormat::Mp3 => "MP3",
        AudioFormat::M4a => "M4A",
        AudioFormat::Opus => "OPUS",
        AudioFormat::Flac => "FLAC",
        AudioFormat::Wav => "WAV",
    }
}

pub fn video_format_name(format: VideoFormat) -> &'static str {
    match format {
        VideoFormat::Mp4 => "MP4",
        VideoFormat::Webm => "WEBM",
    }
}

pub fn audio_quality_name(quality: AudioQuality) -> &'static str {
    match quality {
        AudioQuality::Best => "Max",
        AudioQuality::Kbps320 => "320 kb/s",
        AudioQuality::Kbps256 => "256 kb/s",
        AudioQuality::Kbps192 => "192 kb/s",
        AudioQuality::Kbps128 => "128 kb/s",
    }
}

pub fn video_quality_name(quality: VideoQuality) -> &'static str {
    match quality {
        VideoQuality::Best => "Max",
        VideoQuality::P2160 => "4K",
        VideoQuality::P1440 => "1440p",
        VideoQuality::P1080 => "1080p",
        VideoQuality::P720 => "720p",
        VideoQuality::P480 => "480p",
    }
}

pub fn track_count(count: usize) -> String {
    if count == 1 {
        "1 piste".to_owned()
    } else {
        format!("{count} pistes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(track_duration(19), "0:19");
        assert_eq!(track_duration(187), "3:07");
        assert_eq!(track_duration(3723), "1:02:03");
    }

    #[test]
    fn formats_speed_with_french_comma() {
        assert_eq!(speed_megabytes_per_second(2_440_000), "2,4 Mo/s");
    }

    #[test]
    fn formats_remaining_time() {
        assert_eq!(remaining_time(42), "reste 42 s");
        assert_eq!(remaining_time(185), "reste 3 min 05 s");
        assert_eq!(remaining_time(3720), "reste 1 h 02 min");
    }

    #[test]
    fn formats_percent_and_counts() {
        assert_eq!(percent(0.456), "45 %");
        assert_eq!(percent(1.7), "100 %");
        assert_eq!(track_count(1), "1 piste");
        assert_eq!(track_count(12), "12 pistes");
    }

    #[test]
    fn formats_calendar_dates() {
        assert_eq!(calendar_date(0), "1 janv. 1970");
        // 2026-11-13T12:00:00Z
        assert_eq!(calendar_date(1_794_571_200), "13 nov. 2026");
        // 2024-02-29 (leap day)
        assert_eq!(calendar_date(1_709_164_800), "29 févr. 2024");
    }
}
