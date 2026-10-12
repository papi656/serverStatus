//! Minimal timestamped logger. Writes to stderr; under systemd this is appended to a file.
//!
//! Intentionally tiny: only `info` and `error`, one `[timestamp] LEVEL message` line each.

use std::time::{SystemTime, UNIX_EPOCH};

/// Log an informational message (startup, recovery).
pub fn info(message: &str) {
    emit("INFO", message);
}

/// Log an error (config failure, push failure).
pub fn error(message: &str) {
    emit("ERROR", message);
}

fn emit(level: &str, message: &str) {
    eprintln!("[{}] {} {}", timestamp(), level, message);
}

/// Current time as an ISO-8601 UTC string, e.g. `2026-10-12T13:42:01Z`.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days_since_epoch = (secs / 86_400) as i64;
    let seconds_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days_since_epoch);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60
    )
}

/// Convert days since the Unix epoch to `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days` algorithm (public domain).
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // [0, 365]
    let month_prime = (5 * day_of_year + 2) / 153; // [0, 11]
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32; // [1, 31]
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_known_day_numbers() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(18_993), (2022, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
    }
}
