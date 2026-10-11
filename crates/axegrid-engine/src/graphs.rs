//! The curve a page of the block editor draws beside its controls. Every
//! curve is computed here from the block's current values with textbook
//! formulas; none is read from the device, so each is a guide to what the
//! settings do and not a measurement.

use axegrid_model::curves::{self, Dynamics};
use axegrid_model::eq::{self, Band, Shape};
use axegrid_model::layouts::{Kind, Page, Values};
use axegrid_model::params;
use serde_json::{json, Value};

/// Points along the axis of a curve.
const CURVE_POINTS: usize = 96;
/// Q of a band that has no control for it: about an octave wide.
const DEFAULT_Q: f64 = 1.41;
/// Slope of a cut that has no control for it: 12 dB per octave.
const DEFAULT_ORDER: u8 = 2;
/// Slope of each side of a crossover: 24 dB per octave.
const CROSSOVER_ORDER: u8 = 4;

const COMPRESSOR: u8 = 2;
const GRAPHIC_EQ: u8 = 3;
const PARAMETRIC_EQ: u8 = 4;
const AMP: u8 = 5;
const CAB: u8 = 6;
const REVERB: u8 = 7;
const DELAY: u8 = 8;
const FILTER: u8 = 18;
const CROSSOVER: u8 = 28;
const GATE: u8 = 29;
const MULTIBAND: u8 = 31;
const INPUT_GATE: u8 = 35;
const CONTROLLERS: u8 = 37;

/// Reads a block's values by parameter name.
struct Block<'a> {
    effect: u8,
    values: &'a Values,
}

impl Block<'_> {
    fn text(&self, name: &str) -> Option<&str> {
        self.values.get(&params::id(self.effect, name)?).map(|(_, text)| text.as_str())
    }

    /// The value as a plain number: hertz, decibels, milliseconds or a ratio.
    fn number(&self, name: &str) -> Option<f64> {
        self.text(name).and_then(eq::quantity)
    }

    /// A cut at the frequency of `name`; none while it reads as a word such as OFF.
    fn cut(&self, name: &str, low: bool, order: u8) -> Option<Band> {
        let hz = self.number(name)?;
        let shape = if low { Shape::LowCut(order) } else { Shape::HighCut(order) };
        Some(Band { shape, hz, q: 0.0, gain_db: 0.0 })
    }

    /// A band from three parameters; a missing Q means an octave-wide band.
    fn band(&self, shape: Shape, freq: &str, q: Option<&str>, gain: Option<&str>) -> Option<Band> {
        Some(Band {
            shape,
            hz: self.number(freq)?,
            q: q.and_then(|name| self.number(name)).unwrap_or(DEFAULT_Q),
            gain_db: gain.and_then(|name| self.number(name)).unwrap_or(0.0),
        })
    }
}

/// How steep a cut is, from a choice such as `"12 dB/OCT"` or `"4TH"`.
fn order_of(text: Option<&str>) -> u8 {
    let Some(number) = text.and_then(eq::leading_number) else { return DEFAULT_ORDER };
    let per_octave = text.is_some_and(|text| text.to_ascii_uppercase().contains("DB"));
    let order = if per_octave { number / 6.0 } else { number };
    order.clamp(1.0, 8.0) as u8
}

fn rounded(points: Vec<(f64, f64)>) -> Vec<Value> {
    points.into_iter().map(|(x, y)| json!([(x * 10.0).round() / 10.0, (y * 100.0).round() / 100.0])).collect()
}

/// A frequency-response graph of `bands`, with a numbered handle on each of `marked`.
fn response(bands: &[Band], marked: &[Band]) -> Value {
    let marks: Vec<Value> = marked.iter().map(|band| json!([band.hz, band.gain_db])).collect();
    json!({ "kind": "eq", "points": rounded(eq::response(bands, CURVE_POINTS)), "bands": marks })
}

fn parametric(block: &Block<'_>) -> Value {
    let bands: Vec<Band> = (1..=5)
        .filter_map(|band| {
            // The outer and the inner pairs choose their shape; the middle band is a peak.
            let (chooser, low_side) = match band {
                1 => (Some("PEQ_LFTYPE"), true),
                2 => (Some("PEQ_LMTYPE"), true),
                4 => (Some("PEQ_HMTYPE"), false),
                5 => (Some("PEQ_HFTYPE"), false),
                _ => (None, true),
            };
            let chosen = chooser.and_then(|name| block.text(name)).unwrap_or("PEAKING").to_ascii_uppercase();
            let shape = match (chosen.as_str(), low_side) {
                ("BLOCKING", true) => Shape::HighPass,
                ("BLOCKING", false) => Shape::LowPass,
                (shelf, true) if shelf.starts_with("SHELVING") => Shape::LowShelf,
                (shelf, false) if shelf.starts_with("SHELVING") => Shape::HighShelf,
                _ => Shape::Peaking,
            };
            Some(Band {
                shape,
                hz: block.number(&format!("PEQ_FREQ{band}"))?,
                q: block.number(&format!("PEQ_Q{band}"))?,
                gain_db: block.number(&format!("PEQ_GAIN{band}"))?,
            })
        })
        .collect();
    response(&bands, &bands)
}

/// Two cuts and two peaking bands, as the delay and the reverb have them.
fn cuts_and_peaks(block: &Block<'_>, prefix: &str, low_cut: &str, order: u8) -> Value {
    let name = |rest: &str| format!("{prefix}_{rest}");
    let peaks: Vec<Band> = (1..=2)
        .filter_map(|n| {
            let (freq, q, gain) = (name(&format!("FREQ{n}")), name(&format!("Q{n}")), name(&format!("GAIN{n}")));
            block.band(Shape::Peaking, &freq, Some(&q), Some(&gain))
        })
        .collect();
    let cuts = [block.cut(&name(low_cut), true, order), block.cut(&name("HICUT"), false, order)];
    let bands: Vec<Band> = cuts.into_iter().flatten().chain(peaks.iter().copied()).collect();
    response(&bands, &peaks)
}

fn filter(block: &Block<'_>) -> Value {
    let kind = block.text("FILTER_TYPE").unwrap_or_default().to_ascii_uppercase().replace([' ', '-', '_'], "");
    let shape = match kind.as_str() {
        "LOWPASS" => Some(Shape::LowPass),
        "HIGHPASS" => Some(Shape::HighPass),
        "BANDPASS" => Some(Shape::BandPass),
        "NOTCH" => Some(Shape::Notch),
        "LOWSHELF" | "LOWSHELF2" => Some(Shape::LowShelf),
        "HIGHSHELF" | "HIGHSHELF2" | "HISHELF" | "HISHELF2" => Some(Shape::HighShelf),
        "PEAKING" | "PEAKING2" => Some(Shape::Peaking),
        // NULL, tilt and anything this build does not know: only the cuts are drawn.
        _ => None,
    };
    let main = shape.and_then(|shape| block.band(shape, "FILTER_FREQ", Some("FILTER_Q"), Some("FILTER_GAIN")));
    // A fourth-order filter is the second-order one applied twice.
    let twice = block.text("FILTER_ORDER").and_then(eq::leading_number).is_some_and(|order| order >= 4.0);
    let cuts = [block.cut("FILTER_LOWCUT", true, DEFAULT_ORDER), block.cut("FILTER_HICUT", false, DEFAULT_ORDER)];
    let mut bands: Vec<Band> = main.into_iter().collect();
    if twice {
        bands.extend(main);
    }
    bands.extend(cuts.into_iter().flatten());
    let marked: Vec<Band> = main.into_iter().collect();
    response(&bands, &marked)
}

/// The amp's speaker page: the low resonance, the high-frequency rise and
/// the output transformer's two corners.
fn speaker(block: &Block<'_>) -> Value {
    let marked: Vec<Band> = [
        block.band(Shape::Peaking, "DISTORT_SPKRLFREQ", Some("DISTORT_SPKRLFQ"), Some("DISTORT_SPKRLFGAIN")),
        block.band(Shape::HighShelf, "DISTORT_SPKRHFREQ", None, Some("DISTORT_SPKRHFGAIN")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let corners = [block.cut("DISTORT_XFHPF", true, 1), block.cut("DISTORT_XFLPF", false, 1)];
    let bands: Vec<Band> = marked.iter().copied().chain(corners.into_iter().flatten()).collect();
    response(&bands, &marked)
}

/// A graphic equaliser: one band per fader on the page whose label is a frequency.
fn graphic(block: &Block<'_>, page: &Page) -> Option<Value> {
    let faders = page.rows.iter().flat_map(|row| &row.controls).filter(|control| control.kind == Kind::Slider);
    let shown = faders.filter(|control| control.visible(None, block.values));
    let bands: Vec<Band> = shown
        .filter_map(|control| {
            let hz = eq::quantity(&control.label)?;
            let (_, text) = block.values.get(&control.param)?;
            Some(Band { shape: Shape::Peaking, hz, q: DEFAULT_Q, gain_db: eq::quantity(text)? })
        })
        .collect();
    (!bands.is_empty()).then(|| response(&bands, &[]))
}

/// A level curve. A threshold that reads as a word (`"OFF"`) leaves every
/// level alone, which draws as a straight line.
fn dynamics(kind: Dynamics, block: &Block<'_>, threshold: &str, ratio: &str, knee_db: f64) -> Option<Value> {
    let off = match kind {
        Dynamics::Compressor => 0.0,
        Dynamics::Expander => curves::FLOOR_DB,
    };
    let threshold = block.text(threshold).map(|text| eq::quantity(text).unwrap_or(off))?;
    // A ratio that reads as a word (`"INF"`) limits completely.
    let ratio = block.text(ratio).map(|text| eq::leading_number(text).unwrap_or(1000.0))?;
    let points = curves::transfer(kind, threshold, ratio, knee_db, CURVE_POINTS);
    Some(json!({ "kind": "dynamics", "points": rounded(points), "threshold": threshold, "floor": curves::FLOOR_DB }))
}

/// Width of a compressor's knee from its choice (`"HARD"`, `"SOFT"`, ...).
fn knee_db(text: Option<&str>) -> f64 {
    match text.unwrap_or_default().to_ascii_uppercase().as_str() {
        "MEDIUM" => 6.0,
        "SOFT" => 12.0,
        "SOFTER" => 18.0,
        "SOFTEST" => 24.0,
        _ => 0.0,
    }
}

fn envelope(block: &Block<'_>, number: u8) -> Option<Value> {
    let read = |stage: &str| block.number(&format!("CONTROLLERS_ADSR{number}{stage}"));
    // The level is shown in percent; the four stages are times.
    let level = read("LEVEL")? / 100.0;
    let points = curves::adsr(read("ATTACK")?, read("DECAY")?, read("SUSTAIN")?, level, read("RELEASE")?);
    let length = points.last().map_or(1.0, |point| point.0);
    Some(json!({ "kind": "envelope", "points": rounded(points), "length_ms": length }))
}

/// The low and the high band of a crossover as two curves.
fn crossover(block: &Block<'_>) -> Option<Value> {
    // The multiplier reads "X 1" or "X 10".
    let digits = |text: &str| text.trim_start_matches(|c: char| !c.is_ascii_digit()).parse::<f64>().ok();
    let scale = block.text("CROSSOVER_FREQRANGE").and_then(digits).unwrap_or(1.0);
    let hz = block.number("CROSSOVER_FREQ")? * scale.max(1.0);
    let side = |shape| rounded(eq::response(&[Band { shape, hz, q: 0.0, gain_db: 0.0 }], CURVE_POINTS));
    Some(json!({ "kind": "eq", "points": side(Shape::HighCut(CROSSOVER_ORDER)),
                 "also": side(Shape::LowCut(CROSSOVER_ORDER)), "bands": [[hz, -6.0]] }))
}

/// Whether the page shows a control for the parameter called `name`.
fn shows(page: &Page, effect: u8, name: &str) -> bool {
    let Some(id) = params::id(effect, name) else { return false };
    page.rows.iter().flat_map(|row| &row.controls).any(|control| control.param == id && control.kind != Kind::Spacer)
}

/// The graph of one page, or `None` where there is nothing to draw.
pub(crate) fn for_page(effect: u8, page: &Page, values: &Values) -> Option<Value> {
    let block = Block { effect, values };
    // Pages the layout data marks as having a curve.
    match page.graph.as_deref() {
        Some("eq") if effect == PARAMETRIC_EQ => return Some(parametric(&block)),
        Some("speaker") if effect == AMP => return Some(speaker(&block)),
        Some("delay_eq") if effect == DELAY => {
            let order = order_of(block.text("DELAY_FILTORDER"));
            return Some(cuts_and_peaks(&block, "DELAY", "LOCUT", order));
        }
        Some("reverb_eq") if effect == REVERB => return Some(cuts_and_peaks(&block, "REVERB", "LOWCUT", 1)),
        Some("filter") if effect == FILTER => return Some(filter(&block)),
        Some("adsr1") if effect == CONTROLLERS => return envelope(&block, 1),
        Some("adsr2") if effect == CONTROLLERS => return envelope(&block, 2),
        // The tone-match display is a measurement held by the device.
        _ => {}
    }
    // Curves of this project's own, on the page that holds their controls.
    match effect {
        GRAPHIC_EQ => graphic(&block, page),
        COMPRESSOR if shows(page, effect, "COMP_RATIO") => {
            let knee = knee_db(block.text("COMP_KNEE"));
            dynamics(Dynamics::Compressor, &block, "COMP_THRESH", "COMP_RATIO", knee)
        }
        GATE if shows(page, effect, "GATE_RATIO") => {
            dynamics(Dynamics::Expander, &block, "GATE_THRESH", "GATE_RATIO", 0.0)
        }
        INPUT_GATE if shows(page, effect, "NOISEGATE_RATIO") => {
            dynamics(Dynamics::Expander, &block, "NOISEGATE_THRESH", "NOISEGATE_RATIO", 0.0)
        }
        // The middle band stands for all three.
        MULTIBAND if shows(page, effect, "MULTICOMP_RATIO2") => {
            dynamics(Dynamics::Compressor, &block, "MULTICOMP_THRESH2", "MULTICOMP_RATIO2", 0.0)
        }
        CAB if shows(page, effect, "CABINET_LOCUT") => {
            let order = order_of(block.text("CABINET_ORDER"));
            let cuts = [block.cut("CABINET_LOCUT", true, order), block.cut("CABINET_HICUT", false, order)];
            let bands: Vec<Band> = cuts.into_iter().flatten().collect();
            Some(response(&bands, &[]))
        }
        CROSSOVER if shows(page, effect, "CROSSOVER_FREQ") => crossover(&block),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values as a device reports them, by parameter name.
    fn values(effect: u8, pairs: &[(&str, &str)]) -> Values {
        pairs.iter().map(|(name, text)| (params::id(effect, name).unwrap(), (0, (*text).to_owned()))).collect()
    }

    fn page(effect: u8, name: &str) -> &'static Page {
        let layout = axegrid_model::layouts::for_effect(effect).unwrap();
        layout.variants.iter().flat_map(|v| &v.pages).find(|p| p.name == name).unwrap()
    }

    /// The highest point of a curve.
    fn peak(graph: &Value) -> (f64, f64) {
        let points = graph["points"].as_array().unwrap();
        let pair = |p: &Value| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
        points.iter().map(pair).fold((0.0, f64::MIN), |best, point| if point.1 > best.1 { point } else { best })
    }

    #[test]
    fn slopes_come_out_of_their_choice_texts() {
        assert_eq!(order_of(Some("6 dB/OCT")), 1);
        assert_eq!(order_of(Some("24 dB/Oct")), 4);
        assert_eq!(order_of(Some("4TH")), 4);
        assert_eq!(order_of(Some("?")), DEFAULT_ORDER);
        assert_eq!(order_of(None), DEFAULT_ORDER);
    }

    #[test]
    fn knees_widen_with_their_names() {
        assert_eq!(knee_db(Some("HARD")), 0.0);
        assert!(knee_db(Some("soft")) > knee_db(Some("Medium")));
        assert_eq!(knee_db(None), 0.0);
    }

    #[test]
    fn a_graphic_eq_draws_the_faders_of_its_type() {
        // Texts as an Axe-Fx II XL reports them.
        let mut set = values(GRAPHIC_EQ, &[("GEQ_TYPE", "10 BAND VAR Q"), ("GEQ_GAIN6", "9.00")]);
        for band in (1..=10).filter(|band| *band != 6) {
            set.extend(values(GRAPHIC_EQ, &[(&format!("GEQ_GAIN{band}"), "0.00")]));
        }
        let graph = for_page(GRAPHIC_EQ, page(GRAPHIC_EQ, "GEQ"), &set).unwrap();
        // The sixth fader of the ten-band type is the 1 kHz band.
        let (hz, db) = peak(&graph);
        assert!((800.0..1250.0).contains(&hz) && (8.0..9.5).contains(&db), "{hz} {db}");
        // A type this build has no faders for draws nothing.
        let unknown = values(GRAPHIC_EQ, &[("GEQ_TYPE", "31 BAND")]);
        assert_eq!(for_page(GRAPHIC_EQ, page(GRAPHIC_EQ, "GEQ"), &unknown), None);
    }

    #[test]
    fn a_filter_follows_its_type() {
        let set = |kind: &str| {
            values(
                FILTER,
                &[
                    ("FILTER_TYPE", kind),
                    ("FILTER_FREQ", "1000 Hz"),
                    ("FILTER_Q", "2.000"),
                    ("FILTER_GAIN", "6.00 dB"),
                    ("FILTER_ORDER", "2nd"),
                    ("FILTER_LOWCUT", "20.0 Hz"),
                    ("FILTER_HICUT", "20000 Hz"),
                ],
            )
        };
        let at = |graph: &Value, index: usize| graph["points"][index][1].as_f64().unwrap();
        let peaking = for_page(FILTER, page(FILTER, "Filter"), &set("PEAKING")).unwrap();
        let (hz, db) = peak(&peaking);
        assert!((900.0..1100.0).contains(&hz) && (5.0..6.2).contains(&db), "{hz} {db}");
        assert_eq!(peaking["bands"], json!([[1000.0, 6.0]]));
        let low = for_page(FILTER, page(FILTER, "Filter"), &set("LOWPASS")).unwrap();
        assert!(at(&low, 95) < -20.0 && at(&low, 30).abs() < 1.0);
        // The pass-through type shows only what its two cuts do.
        let null = for_page(FILTER, page(FILTER, "Filter"), &set("NULL")).unwrap();
        assert!(at(&null, 48).abs() < 0.1 && null["bands"] == json!([]));
    }

    #[test]
    fn a_threshold_that_is_off_leaves_levels_alone() {
        let set = values(INPUT_GATE, &[("NOISEGATE_THRESH", "OFF"), ("NOISEGATE_RATIO", "2.00")]);
        let graph = for_page(INPUT_GATE, page(INPUT_GATE, "Gate"), &set).unwrap();
        // Input and output agree, to the precision each is rounded to.
        let apart = |p: &Value| (p[0].as_f64().unwrap() - p[1].as_f64().unwrap()).abs();
        assert!(graph["points"].as_array().unwrap().iter().all(|p| apart(p) < 0.06));

        let set = values(COMPRESSOR, &[("COMP_THRESH", "-40.0 dB"), ("COMP_RATIO", "4.000"), ("COMP_KNEE", "HARD")]);
        let graph = for_page(COMPRESSOR, page(COMPRESSOR, "Studio"), &set).unwrap();
        assert_eq!(graph["threshold"], -40.0);
        assert_eq!(graph["points"][95], json!([0.0, -30.0]));
    }

    #[test]
    fn an_envelope_reads_its_four_times_and_its_level() {
        let stage = |name: &str, text: &'static str| (format!("CONTROLLERS_ADSR1{name}"), text);
        let texts = [
            stage("ATTACK", "10.00 ms"),
            stage("DECAY", "20.00 ms"),
            stage("SUSTAIN", "1.000 s"),
            stage("LEVEL", "50.0 %"),
            stage("RELEASE", "70.00 ms"),
        ];
        let pairs: Vec<(&str, &str)> = texts.iter().map(|(name, text)| (name.as_str(), *text)).collect();
        let graph = for_page(CONTROLLERS, page(CONTROLLERS, "ADSR 1"), &values(CONTROLLERS, &pairs)).unwrap();
        assert_eq!(graph["length_ms"], 1100.0);
        assert_eq!(graph["points"][3], json!([1030.0, 0.5]));
    }
}
