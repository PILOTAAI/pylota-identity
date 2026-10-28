//! Which mail an agent sees ([Screening hold §9], Inbound › Read path): a message is visible unless
//! its status is one of [`NOT_VISIBLE`]. Every list, thread counter and read predicate is built from
//! this one list.
//!
//! [Screening hold §9]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/screening.md

/// The statuses no agent sees without `quarantine:review` (and `hidden`/`throttled` never).
pub const NOT_VISIBLE: [&str; 4] = ["quarantined", "screening", "hidden", "throttled"];

/// The SQL list written into every visibility predicate.
pub const fn not_visible_sql() -> &'static str {
    "('quarantined','screening','hidden','throttled')"
}

pub fn visible(status: &str) -> bool {
    !NOT_VISIBLE.contains(&status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_and_sql_agree() {
        for s in NOT_VISIBLE {
            assert!(not_visible_sql().contains(&format!("'{s}'")));
            assert!(!visible(s));
        }
        assert!(visible("received") && visible("submitted"));
    }
}
