//! HTTP `Last-Modified` date parsing and filesystem timestamp application.

use std::fs::{FileTimes, OpenOptions};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::EngineError;

/// Parses an HTTP-date (`IMF-fixdate`, `RFC 850`, or ANSI C `asctime()`) into a UTC [`SystemTime`].
///
/// Returns `None` if the header string cannot be parsed or precedes the Unix epoch (`1970-01-01`).
pub(crate) fn parse_http_datetime(raw: &str) -> Option<SystemTime> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (year, month, day, hour, minute, second) = parse_imf_fixdate(trimmed)
        .or_else(|| parse_rfc850(trimmed))
        .or_else(|| parse_asctime(trimmed))?;

    let days = days_from_civil(year, month, day)?;
    if days < 0 {
        return None;
    }
    let secs = (days as u64)
        .checked_mul(86_400)?
        .checked_add(u64::from(hour) * 3_600 + u64::from(minute) * 60 + u64::from(second))?;
    UNIX_EPOCH.checked_add(Duration::from_secs(secs))
}

/// Applies the server's `Last-Modified` header timestamp to `path` (modified time, plus creation
/// time on Windows).
pub(crate) fn apply_server_file_time(path: &Path, last_modified: &str) -> Result<(), EngineError> {
    let Some(ts) = parse_http_datetime(last_modified) else {
        return Ok(());
    };

    let mut opts = OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        opts.access_mode(FILE_WRITE_ATTRIBUTES);
    }
    #[cfg(not(windows))]
    {
        opts.write(true);
    }

    let file = opts.open(path)?;
    let mut times = FileTimes::new().set_modified(ts);
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTimesExt as _;
        times = times.set_created(ts);
    }
    file.set_times(times)?;
    Ok(())
}

/// Example: `Sun, 06 Nov 1994 08:49:37 GMT`
fn parse_imf_fixdate(s: &str) -> Option<(i32, u8, u8, u8, u8, u8)> {
    let (_, rest) = s.split_once(',')?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 5 || !parts[4].eq_ignore_ascii_case("GMT") {
        return None;
    }
    let day: u8 = parts[0].parse().ok()?;
    let month = parse_month_abbr(parts[1])?;
    let year: i32 = parts[2].parse().ok()?;
    let (hour, minute, second) = parse_hms(parts[3])?;
    validate_ymd(year, month, day)?;
    Some((year, month, day, hour, minute, second))
}

/// Example: `Sunday, 06-Nov-94 08:49:37 GMT`
fn parse_rfc850(s: &str) -> Option<(i32, u8, u8, u8, u8, u8)> {
    let (_, rest) = s.split_once(',')?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 3 || !parts[2].eq_ignore_ascii_case("GMT") {
        return None;
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let day: u8 = date_parts[0].parse().ok()?;
    let month = parse_month_abbr(date_parts[1])?;
    let yy: i32 = date_parts[2].parse().ok()?;
    let year = if date_parts[2].len() == 2 {
        if yy < 70 { 2000 + yy } else { 1900 + yy }
    } else {
        yy
    };
    let (hour, minute, second) = parse_hms(parts[1])?;
    validate_ymd(year, month, day)?;
    Some((year, month, day, hour, minute, second))
}

/// Example: `Sun Nov  6 08:49:37 1994`
fn parse_asctime(s: &str) -> Option<(i32, u8, u8, u8, u8, u8)> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 5 {
        return None;
    }
    let month = parse_month_abbr(parts[1])?;
    let day: u8 = parts[2].parse().ok()?;
    let (hour, minute, second) = parse_hms(parts[3])?;
    let year: i32 = parts[4].parse().ok()?;
    validate_ymd(year, month, day)?;
    Some((year, month, day, hour, minute, second))
}

fn parse_hms(s: &str) -> Option<(u8, u8, u8)> {
    let mut it = s.split(':');
    let h: u8 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    let sec: u8 = it.next()?.parse().ok()?;
    if it.next().is_some() || h > 23 || m > 59 || sec > 60 {
        return None;
    }
    Some((h, m, sec.min(59)))
}

fn parse_month_abbr(s: &str) -> Option<u8> {
    match s.to_ascii_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn validate_ymd(year: i32, month: u8, day: u8) -> Option<()> {
    if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    (1..=max_day).contains(&day).then_some(())
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Howard Hinnant's civil-to-days algorithm (days since 1970-01-01).
fn days_from_civil(year: i32, month: u8, day: u8) -> Option<i64> {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_three_http_date_formats() {
        // Sun, 06 Nov 1994 08:49:37 GMT -> 784111777 seconds since epoch.
        let expected = UNIX_EPOCH + Duration::from_secs(784_111_777);
        assert_eq!(
            parse_http_datetime("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(expected)
        );
        assert_eq!(
            parse_http_datetime("Sunday, 06-Nov-94 08:49:37 GMT"),
            Some(expected)
        );
        assert_eq!(
            parse_http_datetime("Sun Nov  6 08:49:37 1994"),
            Some(expected)
        );
        assert_eq!(parse_http_datetime("invalid-date"), None);
    }

    #[test]
    fn apply_server_file_time_updates_file_timestamps() {
        let dir = std::env::temp_dir().join(format!("posthaze_ts_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("sample.bin");
        std::fs::write(&file_path, b"hello").unwrap();

        let expected = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        apply_server_file_time(&file_path, "Tue, 14 Nov 2023 22:13:20 GMT").unwrap();

        let meta = std::fs::metadata(&file_path).unwrap();
        assert_eq!(meta.modified().unwrap(), expected);
        #[cfg(windows)]
        assert_eq!(meta.created().unwrap(), expected);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
