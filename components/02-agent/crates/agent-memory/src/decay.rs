use chrono::{DateTime, Utc};

/// Days of age that cut a fact's confidence in half.
pub const DEFAULT_HALF_LIFE_DAYS: f64 = 30.0;

const CONFIDENCE_FLOOR: f64 = 0.05;

/// Exponential decay. A fact at `half_life_days` keeps half of `confidence`.
///
/// The result never rises and never falls below [`CONFIDENCE_FLOOR`] unless the
/// stored confidence was already lower.
pub fn decayed_confidence(
    confidence: f64,
    updated_at: DateTime<Utc>,
    now: DateTime<Utc>,
    half_life_days: f64,
) -> f64 {
    let confidence = confidence.clamp(0.0, 1.0);
    if half_life_days <= 0.0 || confidence <= CONFIDENCE_FLOOR {
        return confidence;
    }
    let elapsed_days = (now - updated_at).num_seconds() as f64 / 86_400.0;
    if elapsed_days <= 0.0 {
        return confidence;
    }
    let decayed = confidence * 0.5_f64.powf(elapsed_days / half_life_days);
    decayed.max(CONFIDENCE_FLOOR).min(confidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn one_half_life_cuts_confidence_in_half() {
        let updated = Utc::now();
        let later = updated + Duration::days(10);
        let decayed = decayed_confidence(0.8, updated, later, 10.0);
        assert!((decayed - 0.4).abs() < 0.000_1, "{decayed}");
        assert_eq!(decayed_confidence(0.8, updated, updated, 10.0), 0.8);
    }

    #[test]
    fn decay_does_not_raise_confidence_or_pass_the_floor() {
        let updated = Utc::now();
        let earlier = updated - Duration::days(1);
        assert_eq!(decayed_confidence(0.4, updated, earlier, 30.0), 0.4);
        let ancient = updated + Duration::days(10_000);
        assert_eq!(decayed_confidence(0.9, updated, ancient, 30.0), 0.05);
    }
}
