//! Curves that are not frequency responses: what a compressor or a gate does
//! to level, and the shape of an envelope. Like [`crate::eq`], these are
//! textbook shapes drawn from the settings, a guide and not a measurement.

/// Lowest input level drawn on a dynamics curve, in decibels.
pub const FLOOR_DB: f64 = -80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dynamics {
    /// Levels above the threshold rise more slowly.
    Compressor,
    /// Levels below the threshold fall away faster.
    Expander,
}

/// Output level for an input level, both in decibels. `knee_db` is the
/// width of the rounded corner around the threshold.
pub fn level_out(kind: Dynamics, input_db: f64, threshold_db: f64, ratio: f64, knee_db: f64) -> f64 {
    let ratio = ratio.max(1.0);
    // Slope of the part beyond the threshold, relative to a straight line.
    let bend = match kind {
        Dynamics::Compressor => 1.0 / ratio - 1.0,
        Dynamics::Expander => ratio - 1.0,
    };
    // How far the input is past the threshold, on the side that is processed.
    let over = match kind {
        Dynamics::Compressor => input_db - threshold_db,
        Dynamics::Expander => threshold_db - input_db,
    };
    let past = if knee_db > 0.0 && over.abs() <= knee_db / 2.0 {
        (over + knee_db / 2.0).powi(2) / (2.0 * knee_db)
    } else {
        over.max(0.0)
    };
    match kind {
        Dynamics::Compressor => input_db + bend * past,
        Dynamics::Expander => input_db - bend * past,
    }
}

/// The transfer curve from [`FLOOR_DB`] to 0 dB as `(input, output)`.
pub fn transfer(kind: Dynamics, threshold_db: f64, ratio: f64, knee_db: f64, points: usize) -> Vec<(f64, f64)> {
    (0..points)
        .map(|index| {
            let input = FLOOR_DB * (1.0 - index as f64 / (points.max(2) - 1) as f64);
            (input, level_out(kind, input, threshold_db, ratio, knee_db).max(FLOOR_DB * 1.5))
        })
        .collect()
}

/// An envelope as `(milliseconds, level)` corner points, level from 0 to 1:
/// a rise to full level over `attack_ms`, a fall to `level` over `decay_ms`,
/// `sustain_ms` held there, and a fall to nothing over `release_ms`.
pub fn adsr(attack_ms: f64, decay_ms: f64, sustain_ms: f64, level: f64, release_ms: f64) -> Vec<(f64, f64)> {
    let level = level.clamp(0.0, 1.0);
    let mut at = 0.0;
    let mut corner = |length: f64, level: f64| {
        at += length.max(0.0);
        (at, level)
    };
    vec![
        (0.0, 0.0),
        corner(attack_ms, 1.0),
        corner(decay_ms, level),
        corner(sustain_ms, level),
        corner(release_ms, 0.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_compressor_flattens_what_is_above_the_threshold() {
        let out = |input| level_out(Dynamics::Compressor, input, -20.0, 4.0, 0.0);
        assert_eq!(out(-40.0), -40.0);
        assert_eq!(out(-20.0), -20.0);
        assert!((out(0.0) + 15.0).abs() < 1e-9);
        // Ratio 1 changes nothing; a ratio below 1 is treated as 1.
        assert_eq!(level_out(Dynamics::Compressor, -5.0, -20.0, 0.5, 0.0), -5.0);
    }

    #[test]
    fn an_expander_pushes_down_what_is_below() {
        let out = |input| level_out(Dynamics::Expander, input, -50.0, 2.0, 0.0);
        assert_eq!(out(-30.0), -30.0);
        assert!((out(-60.0) + 70.0).abs() < 1e-9);
    }

    #[test]
    fn a_knee_rounds_the_corner_without_a_step() {
        let hard = |input| level_out(Dynamics::Compressor, input, -20.0, 4.0, 0.0);
        let soft = |input| level_out(Dynamics::Compressor, input, -20.0, 4.0, 12.0);
        assert!(soft(-20.0) < hard(-20.0));
        // Outside the knee the two agree, and the curve joins up at its ends.
        assert_eq!(soft(-40.0), hard(-40.0));
        assert!((soft(-14.0) - hard(-14.0)).abs() < 1e-9 && (soft(-26.0) - hard(-26.0)).abs() < 1e-9);
        let curve = transfer(Dynamics::Compressor, -20.0, 4.0, 12.0, 81);
        assert_eq!((curve[0].0, curve[80].0), (FLOOR_DB, 0.0));
        assert!(curve.windows(2).all(|pair| pair[1].1 >= pair[0].1));
    }

    #[test]
    fn an_envelope_has_five_corners() {
        let shape = adsr(10.0, 20.0, 20.0, 0.5, 30.0);
        assert_eq!(shape, [(0.0, 0.0), (10.0, 1.0), (30.0, 0.5), (50.0, 0.5), (80.0, 0.0)]);
        // Out-of-range settings still give a shape that does not run backwards.
        let odd = adsr(0.0, 0.0, 5.0, 2.0, -5.0);
        assert_eq!((odd[2].1, odd.last()), (1.0, Some(&(5.0, 0.0))));
    }
}
