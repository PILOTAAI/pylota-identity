//! The repeat policy of Testing § 9.3: a suite runs three times and scores the median; when the spread
//! (highest minus lowest) exceeds 0.02, three more runs follow and the median of six counts; a spread
//! still above 0.02 fails as `unstable`. A gate fails when the median is under its floor, or more than
//! 0.01 below the baseline.

/// The widest spread a score may have.
pub const SPREAD: f64 = 0.02;
/// The largest drop below the baseline that still passes.
pub const DROP: f64 = 0.01;

/// What to do after the runs so far.
#[derive(Clone, Debug, PartialEq)]
pub enum Next {
    /// Run this many more times.
    More(usize),
    /// Scored: the median.
    Scored(f64),
    /// Six runs still spread more than [`SPREAD`].
    Unstable { spread: f64 },
}

pub fn median(scores: &[f64]) -> f64 {
    let mut s = scores.to_vec();
    s.sort_by(f64::total_cmp);
    match s.len() {
        0 => 0.0,
        n if n % 2 == 1 => s[n / 2],
        n => (s[n / 2 - 1] + s[n / 2]) / 2.0,
    }
}

pub fn spread(scores: &[f64]) -> f64 {
    let max = scores.iter().copied().fold(f64::MIN, f64::max);
    let min = scores.iter().copied().fold(f64::MAX, f64::min);
    if scores.is_empty() { 0.0 } else { max - min }
}

/// The step after `scores` (the per-run scores so far, in run order).
pub fn next(scores: &[f64]) -> Next {
    let n = scores.len();
    if n < 3 {
        return Next::More(3 - n);
    }
    let s = spread(scores);
    // A small float error must not turn 0.02 into a rerun.
    if s <= SPREAD + 1e-9 {
        return Next::Scored(median(scores));
    }
    if n < 6 {
        Next::More(6 - n)
    } else {
        Next::Unstable { spread: s }
    }
}

/// Whether a scored median passes: at or above `floor`, and no more than [`DROP`] under the baseline.
pub fn gate(median: f64, floor: Option<f64>, baseline: Option<f64>) -> Result<(), String> {
    if let Some(f) = floor
        && median + 1e-9 < f
    {
        return Err(format!("{median:.4} is under the gate of {f:.2}"));
    }
    if let Some(b) = baseline
        && b - median > DROP + 1e-9
    {
        return Err(format!(
            "{median:.4} dropped more than {DROP} below the baseline {b:.4}"
        ));
    }
    Ok(())
}
