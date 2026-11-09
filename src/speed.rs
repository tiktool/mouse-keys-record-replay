//! Playback speed: five fixed steps plus one the user types in.

pub const MIN: f64 = 0.1;
pub const MAX: f64 = 1000.0;
pub const DEFAULT_STEP: usize = 2;
pub const DEFAULT_CUSTOM: f64 = 100.0;
pub const CUSTOM: usize = 5;
pub const COUNT: usize = 6;

const FIXED: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];
pub const LABELS: [&str; COUNT] = ["1/4x", "1/2x", "1x", "2x", "4x", "Custom"];
pub const BAD_CUSTOM: &str = "Enter a speed from 0.1 to 1000.";

/// The playback factor for a step; the custom value is used only by the last step.
pub fn factor(step: usize, custom: f64) -> f64 {
    match FIXED.get(step) {
        Some(&f) => f,
        None => clamp(custom),
    }
}

fn clamp(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(MIN, MAX)
    } else {
        DEFAULT_CUSTOM
    }
}

/// Parses the custom speed box. A trailing "x" is accepted, so "2.5x" works.
pub fn parse_custom(s: &str) -> Result<f64, String> {
    let t = s.trim();
    let t = t.strip_suffix(['x', 'X']).unwrap_or(t).trim();
    match t.parse::<f64>() {
        Ok(v) if v.is_finite() && (MIN..=MAX).contains(&v) => Ok(v),
        _ => Err(BAD_CUSTOM.to_string()),
    }
}

/// "2x", "0.25x", "100x": the factor with no trailing zeros.
pub fn label(f: f64) -> String {
    let s = format!("{:.2}", f);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}x")
}

/// Short text for the step, as the slider and the HUD show it.
pub fn step_label(step: usize, custom: f64) -> String {
    if step == CUSTOM {
        label(factor(step, custom))
    } else {
        LABELS[step.min(CUSTOM)].to_string()
    }
}

/// The custom box text for a stored value.
pub fn custom_text(custom: f64) -> String {
    let l = label(clamp(custom));
    l.trim_end_matches('x').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_map_to_factors() {
        let want = [0.25, 0.5, 1.0, 2.0, 4.0, 100.0];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(factor(i, 100.0), *w);
        }
        assert_eq!(factor(CUSTOM, 7.5), 7.5);
        assert_eq!(factor(DEFAULT_STEP, 7.5), 1.0);
    }

    #[test]
    fn custom_factor_is_clamped() {
        assert_eq!(factor(CUSTOM, 0.0), MIN);
        assert_eq!(factor(CUSTOM, 5000.0), MAX);
        assert_eq!(factor(CUSTOM, f64::NAN), DEFAULT_CUSTOM);
        assert_eq!(factor(99, 3.0), 3.0);
    }

    #[test]
    fn parses_custom_values() {
        assert_eq!(parse_custom("100"), Ok(100.0));
        assert_eq!(parse_custom(" 2.5x "), Ok(2.5));
        assert_eq!(parse_custom("0.1"), Ok(0.1));
        assert_eq!(parse_custom("1000"), Ok(1000.0));
        assert_eq!(parse_custom("1e2"), Ok(100.0));
        for bad in [
            "", "x", "abc", "0", "0.09", "1000.5", "-3", "inf", "NaN", "1,5",
        ] {
            assert_eq!(parse_custom(bad), Err(BAD_CUSTOM.to_string()), "{bad}");
        }
    }

    #[test]
    fn labels_drop_trailing_zeros() {
        assert_eq!(label(100.0), "100x");
        assert_eq!(label(2.5), "2.5x");
        assert_eq!(label(0.25), "0.25x");
        assert_eq!(step_label(0, 100.0), "1/4x");
        assert_eq!(step_label(CUSTOM, 12.0), "12x");
        assert_eq!(custom_text(100.0), "100");
        assert_eq!(custom_text(0.1), "0.1");
    }
}
