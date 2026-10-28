//! The deterministic judge between planning steps ([Search § 11.2] step 6).
//!
//! [Search § 11.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

/// What follows a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Plan,
    Answer,
    /// Nothing found, and two different searches came back empty: no answer call.
    Insufficient,
    /// Less than a second left: the evidence, no answer.
    Exhausted,
}

/// After planning step `step` of `max_steps`, with `remaining_ms` left. `searches` are the queries run
/// so far with their hit counts, oldest first.
pub fn judge(
    step: u8,
    max_steps: u8,
    remaining_ms: i64,
    evidence_empty: bool,
    searches: &[(String, usize)],
) -> Next {
    let last_two_empty = searches.len() >= 2 && {
        let (a, b) = (&searches[searches.len() - 2], &searches[searches.len() - 1]);
        a.1 == 0 && b.1 == 0 && a.0 != b.0
    };
    if last_two_empty && evidence_empty {
        return Next::Insufficient;
    }
    if remaining_ms < 1_000 {
        return Next::Exhausted;
    }
    if u16::from(step) + 1 >= u16::from(max_steps).saturating_sub(1) || remaining_ms < 2_500 {
        return Next::Answer;
    }
    Next::Plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(q: &str, n: usize) -> (String, usize) {
        (q.to_owned(), n)
    }

    #[test]
    fn judge_rules() {
        assert_eq!(judge(1, 6, 6_000, false, &[s("a", 3)]), Next::Plan);
        // Only the answer step is left.
        assert_eq!(judge(4, 6, 6_000, false, &[]), Next::Answer);
        assert_eq!(judge(1, 6, 2_000, false, &[]), Next::Answer);
        assert_eq!(judge(1, 6, 900, false, &[]), Next::Exhausted);
        // Two different empty searches and nothing found.
        assert_eq!(
            judge(2, 6, 6_000, true, &[s("a", 0), s("b", 0)]),
            Next::Insufficient
        );
        // The same query twice, or evidence from elsewhere, keeps going.
        assert_eq!(
            judge(2, 6, 6_000, true, &[s("a", 0), s("a", 0)]),
            Next::Plan
        );
        assert_eq!(
            judge(2, 6, 6_000, false, &[s("a", 0), s("b", 0)]),
            Next::Plan
        );
    }
}
