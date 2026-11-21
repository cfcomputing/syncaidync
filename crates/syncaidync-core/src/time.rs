//! Civil dates from Unix epoch milliseconds (UTC). No clock access.

const MS_PER_DAY: i64 = 86_400_000;

/// Convert epoch milliseconds to a UTC civil date `(year, month, day)`.
pub fn civil_date(epoch_ms: u64) -> (i32, u32, u32) {
    // Algorithm from civil_from_days (Howard Hinnant), public domain.
    let z = (epoch_ms as i64).div_euclid(MS_PER_DAY) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Format epoch milliseconds as `YYYY-MM-DD` in UTC.
pub fn format_date(epoch_ms: u64) -> String {
    let (y, m, d) = civil_date(epoch_ms);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_start() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(format_date(0), "1970-01-01");
    }

    #[test]
    fn leap_day() {
        // 2024-02-29 00:00:00 UTC
        let ms = 1_709_164_800_000u64;
        assert_eq!(civil_date(ms), (2024, 2, 29));
    }

    #[test]
    fn known_day() {
        // 2026-11-17 00:00:00 UTC
        let ms = 1_794_873_600_000u64;
        assert_eq!(format_date(ms), "2026-11-17");
    }

    #[test]
    fn year_boundary() {
        // 2025-12-31 23:59:59.999 UTC → still 2025-12-31
        let ms = 1_767_225_599_999u64;
        assert_eq!(format_date(ms), "2025-12-31");
        // 2026-01-01 00:00:00 UTC
        assert_eq!(format_date(ms + 1), "2026-01-01");
    }
}
