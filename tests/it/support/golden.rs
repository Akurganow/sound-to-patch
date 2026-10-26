//! Golden vectors from `tests/fixtures/golden/` and the one comparison helper.

use std::fs;
use std::path::Path;

use serde_json::Value;

/// An absolute and a relative bound: `|actual - expected| <= abs + rel |expected|`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tolerance {
    pub(crate) abs: f64,
    pub(crate) rel: f64,
}

/// A golden vector: its recipe, values and stored tolerance.
pub(crate) struct Golden {
    json: Value,
}

impl Golden {
    /// Reads `tests/fixtures/golden/<name>.json`.
    pub(crate) fn load(name: &str) -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden")
            .join(format!("{name}.json"));
        let text = fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!(
                "cannot read {}: {err}. Run `cargo xtask golden`",
                path.display()
            )
        });
        Self {
            json: serde_json::from_str(&text).unwrap(),
        }
    }

    pub(crate) fn recipe(&self) -> &Value {
        &self.json["recipe"]
    }

    pub(crate) fn values(&self) -> &Value {
        &self.json["values"]
    }

    pub(crate) fn tolerance(&self) -> Tolerance {
        let tolerance = &self.json["tolerance"];
        Tolerance {
            abs: tolerance["abs"].as_f64().unwrap(),
            rel: tolerance["rel"].as_f64().unwrap(),
        }
    }
}

/// Reads a JSON array of numbers.
pub(crate) fn floats(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {value}"))
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

/// Reads a JSON number.
pub(crate) fn float(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

/// Compares two slices element by element, naming the first value out of
/// bounds.
pub(crate) fn check_close(
    actual: &[f64],
    expected: &[f64],
    tolerance: Tolerance,
) -> Result<(), String> {
    if actual.len() != expected.len() {
        return Err(format!(
            "{} values, expected {}",
            actual.len(),
            expected.len()
        ));
    }
    for (index, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        let bound = tolerance.abs + tolerance.rel * e.abs();
        if (a - e).abs() > bound || !a.is_finite() {
            return Err(format!(
                "value {index}: got {a:e}, expected {e:e} within {bound:e} ({tolerance:?})"
            ));
        }
    }
    Ok(())
}

/// Panics with the first value out of bounds.
pub(crate) fn assert_close(actual: &[f64], expected: &[f64], tolerance: Tolerance) {
    if let Err(message) = check_close(actual, expected, tolerance) {
        panic!("{message}");
    }
}
