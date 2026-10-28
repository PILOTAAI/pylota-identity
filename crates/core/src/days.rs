//! Day boundaries (Outbound › Day boundaries): daily caps count in the tenant's IANA time zone, usage in
//! UTC. Pure: the time is an argument.

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// Whether `name` is an IANA zone this build knows.
pub fn valid_timezone(name: &str) -> bool {
    name.parse::<Tz>().is_ok()
}

fn utc(now_ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(now_ms).unwrap_or_default()
}

/// `YYYY-MM-DD` in UTC.
pub fn utc_day(now_ms: i64) -> String {
    utc(now_ms).format("%Y-%m-%d").to_string()
}

/// `YYYY-MM-DDTHH` in UTC.
pub fn utc_hour(now_ms: i64) -> String {
    utc(now_ms).format("%Y-%m-%dT%H").to_string()
}

/// `YYYY-MM-DD` in `tz` (UTC for an unknown zone).
pub fn local_day(now_ms: i64, tz: &str) -> String {
    match tz.parse::<Tz>() {
        Ok(z) => utc(now_ms).with_timezone(&z).format("%Y-%m-%d").to_string(),
        Err(_) => utc_day(now_ms),
    }
}

/// The next local midnight in `tz`, as Unix milliseconds.
pub fn next_local_midnight(now_ms: i64, tz: &str) -> i64 {
    let z: Tz = tz.parse().unwrap_or(Tz::UTC);
    let local = utc(now_ms).with_timezone(&z);
    let tomorrow = local.date_naive() + Duration::days(1);
    let midnight = tomorrow.and_time(NaiveTime::MIN);
    z.from_local_datetime(&midnight)
        .earliest()
        .map_or(now_ms + 86_400_000, |d| d.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_utc_days() {
        // 2026-10-28T23:30:00Z is already the 29th in Europe/Berlin (UTC+1 after the October change).
        let t = 1_793_230_200_000;
        assert_eq!(utc_day(t), "2026-10-28");
        assert_eq!(local_day(t, "Europe/Berlin"), "2026-10-29");
        assert_eq!(local_day(t, "Not/AZone"), "2026-10-28");
        assert_eq!(utc_hour(t), "2026-10-28T23");
        assert!(valid_timezone("Europe/London") && !valid_timezone("Mars/Base"));
        let m = next_local_midnight(t, "UTC");
        assert_eq!(utc_day(m), "2026-10-29");
        assert_eq!(m % 86_400_000, 0);
    }
}

/// RFC 3339 UTC with seconds (`2026-10-09T10:00:00Z`), as the API writes times.
pub fn rfc3339(ms: i64) -> String {
    utc(ms).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Parses an RFC 3339 time into Unix milliseconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod rfc_tests {
    #[test]
    fn rfc3339_round_trip() {
        assert_eq!(super::rfc3339(1_791_540_000_000), "2026-10-09T10:00:00Z");
        assert_eq!(
            super::parse_rfc3339("2026-10-09T10:00:00Z"),
            Some(1_791_540_000_000)
        );
        assert_eq!(super::parse_rfc3339("yesterday"), None);
    }
}
