//! Frequency response of a chain of equaliser bands, for drawing a curve
//! next to the controls. The filters are the textbook second-order designs;
//! the device's own may differ in detail, so the curve is a guide to what
//! the settings do, not a measurement.

use std::f64::consts::PI;

/// Sample rate the device runs at.
const SAMPLE_RATE: f64 = 48_000.0;
/// Lowest and highest frequency drawn.
pub const SPAN_HZ: (f64, f64) = (20.0, 20_000.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Peaking,
    LowShelf,
    HighShelf,
    /// Removes everything below the frequency.
    HighPass,
    /// Removes everything above the frequency.
    LowPass,
    /// Passes a band around the frequency.
    BandPass,
    /// Removes a narrow band at the frequency.
    Notch,
    /// A cut below the frequency with a slope of this many times 6 dB per octave.
    LowCut(u8),
    /// The same above the frequency.
    HighCut(u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub shape: Shape,
    pub hz: f64,
    pub q: f64,
    pub gain_db: f64,
}

/// Normalised biquad coefficients `(b0, b1, b2, a1, a2)`.
fn coefficients(band: &Band) -> [f64; 5] {
    let w = 2.0 * PI * (band.hz.clamp(1.0, SAMPLE_RATE * 0.49) / SAMPLE_RATE);
    let (sin, cos) = w.sin_cos();
    let alpha = sin / (2.0 * band.q.max(0.05));
    let a = 10f64.powf(band.gain_db / 40.0);
    let (b, a0, a1, a2) = match band.shape {
        Shape::Peaking => {
            ([1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a], 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
        }
        Shape::HighPass => ([(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0], 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        Shape::LowPass => ([(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0], 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        Shape::BandPass => ([alpha, 0.0, -alpha], 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        Shape::Notch => ([1.0, -2.0 * cos, 1.0], 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        // Drawn from their slope alone, in `band_db`.
        Shape::LowCut(_) | Shape::HighCut(_) => ([1.0, 0.0, 0.0], 1.0, 0.0, 0.0),
        Shape::LowShelf => {
            let k = 2.0 * a.sqrt() * alpha;
            (
                [
                    a * ((a + 1.0) - (a - 1.0) * cos + k),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - k),
                ],
                (a + 1.0) + (a - 1.0) * cos + k,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - k,
            )
        }
        Shape::HighShelf => {
            let k = 2.0 * a.sqrt() * alpha;
            (
                [
                    a * ((a + 1.0) + (a - 1.0) * cos + k),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - k),
                ],
                (a + 1.0) - (a - 1.0) * cos + k,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - k,
            )
        }
    };
    [b[0] / a0, b[1] / a0, b[2] / a0, a1 / a0, a2 / a0]
}

/// Gain of one band at `hz`, in decibels.
fn band_db(band: &Band, hz: f64) -> f64 {
    // A cut of a given slope: the maximally flat response of that order.
    let slope = |order: u8, ratio: f64| -10.0 * (1.0 + ratio.powi(2 * i32::from(order.max(1)))).log10();
    match band.shape {
        Shape::LowCut(order) => return slope(order, band.hz / hz),
        Shape::HighCut(order) => return slope(order, hz / band.hz),
        _ => {}
    }
    let [b0, b1, b2, a1, a2] = coefficients(band);
    let w = 2.0 * PI * hz / SAMPLE_RATE;
    let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let numerator = (b0 + b1 * c1 + b2 * c2).powi(2) + (b1 * s1 + b2 * s2).powi(2);
    let denominator = (1.0 + a1 * c1 + a2 * c2).powi(2) + (a1 * s1 + a2 * s2).powi(2);
    10.0 * (numerator / denominator).max(1e-12).log10()
}

/// The combined response at `points` frequencies spaced evenly on a
/// logarithmic axis across [`SPAN_HZ`], as `(hz, decibels)`.
pub fn response(bands: &[Band], points: usize) -> Vec<(f64, f64)> {
    let (low, high) = SPAN_HZ;
    (0..points)
        .map(|index| {
            let hz = low * (high / low).powf(index as f64 / (points.max(2) - 1) as f64);
            (hz, bands.iter().map(|band| band_db(band, hz)).sum())
        })
        .collect()
}

/// A display text as a plain quantity: `"1.25 kHz"` is 1250, `"80.0 Hz"`
/// is 80, `"250 ms"` is 250 and `"1.2 s"` is 1200 (times come out in
/// milliseconds).
pub fn quantity(text: &str) -> Option<f64> {
    let number = leading_number(text)?;
    let digits = text.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(text.len());
    let unit = text[digits..].trim().to_ascii_lowercase();
    Some(match unit.as_str() {
        "khz" | "k" | "s" | "sec" => number * 1000.0,
        _ => number,
    })
}

/// The number at the start of a display text such as `"1599 Hz"` or `"-3.5 dB"`.
pub fn leading_number(text: &str) -> Option<f64> {
    let end = text.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(text.len());
    text[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(curve: &[(f64, f64)], hz: f64) -> f64 {
        let nearest = curve.iter().min_by(|a, b| (a.0 - hz).abs().total_cmp(&(b.0 - hz).abs()));
        nearest.map(|point| point.1).unwrap()
    }

    #[test]
    fn flat_settings_draw_a_flat_line() {
        let band = Band { shape: Shape::Peaking, hz: 800.0, q: 0.707, gain_db: 0.0 };
        let shelf = Band { shape: Shape::LowShelf, hz: 200.0, q: 0.707, gain_db: 0.0 };
        assert!(response(&[band, shelf], 64).iter().all(|&(_, db)| db.abs() < 1e-6));
        assert!(response(&[], 8).iter().all(|&(_, db)| db == 0.0));
    }

    #[test]
    fn peak_is_at_its_frequency() {
        let curve = response(&[Band { shape: Shape::Peaking, hz: 1000.0, q: 2.0, gain_db: 6.0 }], 400);
        assert!((at(&curve, 1000.0) - 6.0).abs() < 0.1);
        assert!(at(&curve, 100.0).abs() < 0.3);
        assert!(at(&curve, 10_000.0).abs() < 0.3);
    }

    #[test]
    fn shelves_and_cuts_act_on_their_side() {
        let low = response(&[Band { shape: Shape::LowShelf, hz: 200.0, q: 0.707, gain_db: 6.0 }], 200);
        assert!((at(&low, 20.0) - 6.0).abs() < 0.3 && at(&low, 10_000.0).abs() < 0.2);
        let high = response(&[Band { shape: Shape::HighShelf, hz: 3200.0, q: 0.707, gain_db: -6.0 }], 200);
        assert!((at(&high, 20_000.0) + 6.0).abs() < 0.5 && at(&high, 50.0).abs() < 0.2);
        let cut = response(&[Band { shape: Shape::HighPass, hz: 200.0, q: 0.707, gain_db: 0.0 }], 200);
        assert!(at(&cut, 20.0) < -30.0 && (at(&cut, 200.0) + 3.0).abs() < 0.3 && at(&cut, 5000.0).abs() < 0.1);
        let roll = response(&[Band { shape: Shape::LowPass, hz: 3200.0, q: 0.707, gain_db: 0.0 }], 200);
        assert!(at(&roll, 20_000.0) < -20.0 && at(&roll, 100.0).abs() < 0.1);
    }

    #[test]
    fn cuts_follow_their_slope() {
        let gentle = response(&[Band { shape: Shape::LowCut(1), hz: 100.0, q: 0.0, gain_db: 0.0 }], 400);
        let steep = response(&[Band { shape: Shape::LowCut(4), hz: 100.0, q: 0.0, gain_db: 0.0 }], 400);
        assert!((at(&gentle, 100.0) + 3.0).abs() < 0.2 && (at(&steep, 100.0) + 3.0).abs() < 0.3);
        // One octave down: about 6 dB per order.
        assert!((at(&gentle, 50.0) + 7.0).abs() < 0.5 && (at(&steep, 50.0) + 24.1).abs() < 0.8);
        assert!(at(&steep, 5000.0).abs() < 0.01);
        let top = response(&[Band { shape: Shape::HighCut(2), hz: 5000.0, q: 0.0, gain_db: 0.0 }], 400);
        assert!(at(&top, 20_000.0) < -20.0 && at(&top, 200.0).abs() < 0.01);
    }

    #[test]
    fn band_pass_and_notch_centre_on_their_frequency() {
        let pass = response(&[Band { shape: Shape::BandPass, hz: 1000.0, q: 2.0, gain_db: 0.0 }], 400);
        assert!(at(&pass, 1000.0).abs() < 0.2 && at(&pass, 100.0) < -12.0 && at(&pass, 10_000.0) < -12.0);
        let notch = response(&[Band { shape: Shape::Notch, hz: 1000.0, q: 2.0, gain_db: 0.0 }], 400);
        assert!(at(&notch, 1000.0) < -20.0 && at(&notch, 100.0).abs() < 0.5);
    }

    #[test]
    fn quantities_carry_their_unit() {
        assert_eq!(quantity("1.25 kHz"), Some(1250.0));
        assert_eq!(quantity("80.0 Hz"), Some(80.0));
        assert_eq!(quantity("250 ms"), Some(250.0));
        assert_eq!(quantity("1.20 s"), Some(1200.0));
        assert_eq!(quantity("-3.5 dB"), Some(-3.5));
        assert_eq!(quantity("OFF"), None);
    }

    #[test]
    fn numbers_come_out_of_display_text() {
        assert_eq!(leading_number("200.0 Hz"), Some(200.0));
        assert_eq!(leading_number("1599 Hz"), Some(1599.0));
        assert_eq!(leading_number("-3.50 dB"), Some(-3.5));
        assert_eq!(leading_number("0.707"), Some(0.707));
        assert_eq!(leading_number("OFF"), None);
    }
}
