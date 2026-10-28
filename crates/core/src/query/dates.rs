//! Date resolution ([Search § 4]): every date filter becomes a UTC instant in the tenant time zone, so
//! the mailbox only ever sees Unix milliseconds (F9). `now` is an input: the request time, or the
//! cursor's `as_of` on later pages.
//!
//! [Search § 4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use chrono::{DateTime, Days, LocalResult, Months, NaiveDateTime, TimeZone as _};
use chrono_tz::Tz;

use super::ast::{CivilDate, DateBound, DurUnit, Expr, Filter, Query, RelDuration};

/// A query whose date filters are all `After(Instant)` or `Before(Instant)`: `newer_than:` became
/// `After`, `older_than:` became `Before`, and each day became its local midnight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedQuery {
    pub query: Query,
}

pub fn resolve(q: &Query, tz: Tz, now_ms: i64) -> ResolvedQuery {
    let mut query = q.clone();
    if let Some(root) = &mut query.root {
        walk(root, tz, now_ms);
    }
    ResolvedQuery { query }
}

fn walk(e: &mut Expr, tz: Tz, now_ms: i64) {
    match e {
        Expr::And(xs) | Expr::Or(xs) => xs.iter_mut().for_each(|x| walk(x, tz, now_ms)),
        Expr::Not(x) => walk(x, tz, now_ms),
        Expr::Text(_) => {}
        Expr::Filter(f) => {
            let instant = |b: &DateBound| DateBound::Instant(bound_ms(b, tz));
            *f = match f {
                Filter::After(b) => Filter::After(instant(b)),
                Filter::Before(b) => Filter::Before(instant(b)),
                Filter::NewerThan(d) => Filter::After(DateBound::Instant(ago(*d, tz, now_ms))),
                Filter::OlderThan(d) => Filter::Before(DateBound::Instant(ago(*d, tz, now_ms))),
                _ => return,
            };
        }
    }
}

/// A bound in Unix milliseconds: an instant as is, a day at its local midnight.
pub fn bound_ms(b: &DateBound, tz: Tz) -> i64 {
    match b {
        DateBound::Instant(ms) => *ms,
        DateBound::Day(d) => local_midnight_ms(*d, tz),
    }
}

/// The start of `day` in `tz`. In a DST gap, the first valid instant after midnight; when midnight
/// occurs twice, the earlier.
pub fn local_midnight_ms(day: CivilDate, tz: Tz) -> i64 {
    local_to_ms(day.and_time(chrono::NaiveTime::MIN), tz)
}

/// `now − n·unit`. Hours are exact; days, weeks, months and years are calendar arithmetic on the local
/// date and time in `tz` (months clamp to the end of the month), then back to UTC by the same gap and
/// overlap rule as midnight.
pub fn ago(d: RelDuration, tz: Tz, now_ms: i64) -> i64 {
    let n = u64::from(d.n);
    if d.unit == DurUnit::Hour {
        return now_ms.saturating_sub(d.n as i64 * 3_600_000);
    }
    let Some(now) = DateTime::from_timestamp_millis(now_ms) else {
        return now_ms;
    };
    let local = now.with_timezone(&tz).naive_local();
    let shifted = match d.unit {
        DurUnit::Hour | DurUnit::Day => local.checked_sub_days(Days::new(n)),
        DurUnit::Week => local.checked_sub_days(Days::new(7 * n)),
        DurUnit::Month => local.checked_sub_months(Months::new(d.n)),
        DurUnit::Year => local.checked_sub_months(Months::new(12 * d.n)),
    };
    shifted.map_or(i64::MIN, |t| local_to_ms(t, tz))
}

fn local_to_ms(local: NaiveDateTime, tz: Tz) -> i64 {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(t) => t.timestamp_millis(),
        LocalResult::Ambiguous(a, b) => a.timestamp_millis().min(b.timestamp_millis()),
        LocalResult::None => first_after_gap(local, tz),
    }
}

/// The first instant whose local time is at or after `local`, which falls in a gap: binary search over
/// UTC seconds within two days either side (a gap is never longer than a day).
fn first_after_gap(local: NaiveDateTime, tz: Tz) -> i64 {
    let at =
        |secs: i64| DateTime::from_timestamp(secs, 0).map(|t| t.with_timezone(&tz).naive_local());
    let base = local.and_utc().timestamp();
    let (mut lo, mut hi) = (base - 2 * 86_400, base + 2 * 86_400);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        match at(mid) {
            Some(t) if t >= local => hi = mid,
            Some(_) => lo = mid,
            None => return base * 1000,
        }
    }
    hi * 1000
}
