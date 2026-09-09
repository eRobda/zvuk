//! Reading and writing measurements as JSON files.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::measurement::MeasurementRecord;

/// Writes a record to `dir` as `<timestamp>_<module>[_<label>].json`.
pub fn save(dir: &Path, record: &MeasurementRecord) -> Result<PathBuf> {
    fs::create_dir_all(dir)
        .with_context(|| format!("cannot create directory {}", dir.display()))?;

    let stamp = record.timestamp.format("%Y%m%d-%H%M%S");
    let mut stem = format!("{}_{}", stamp, slug(&record.module));
    if let Some(label) = &record.label {
        let s = slug(label);
        if !s.is_empty() {
            stem.push('_');
            stem.push_str(&s);
        }
    }

    let path = dir.join(format!("{stem}.json"));
    let json = serde_json::to_string_pretty(record)?;
    fs::write(&path, json).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

pub fn load(path: &Path) -> Result<MeasurementRecord> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let record = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid measurement", path.display()))?;
    Ok(record)
}

/// Turns arbitrary text into a safe file name fragment: diacritics folded to
/// ASCII, everything else collapsed into single dashes.
fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in text.chars() {
        let mapped = match ch {
            'a'..='z' | '0'..='9' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => strip_diacritics(ch).unwrap_or('-'),
        };
        if mapped == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        out.push(mapped);
    }
    out.trim_matches('-').to_string()
}

/// Folds the accented letters common in Czech, Slovak, German and Polish onto
/// their ASCII base. Anything unknown becomes a dash in [`slug`].
fn strip_diacritics(ch: char) -> Option<char> {
    const MAP: [(char, char); 29] = [
        ('\u{e1}', 'a'), // a-acute
        ('\u{e0}', 'a'),
        ('\u{e4}', 'a'),
        ('\u{e2}', 'a'),
        ('\u{10d}', 'c'), // c-caron
        ('\u{e7}', 'c'),
        ('\u{10f}', 'd'), // d-caron
        ('\u{e9}', 'e'),  // e-acute
        ('\u{11b}', 'e'), // e-caron
        ('\u{e8}', 'e'),
        ('\u{eb}', 'e'),
        ('\u{ed}', 'i'), // i-acute
        ('\u{ec}', 'i'),
        ('\u{ef}', 'i'),
        ('\u{148}', 'n'), // n-caron
        ('\u{f3}', 'o'),  // o-acute
        ('\u{f6}', 'o'),
        ('\u{f4}', 'o'),
        ('\u{159}', 'r'), // r-caron
        ('\u{161}', 's'), // s-caron
        ('\u{165}', 't'), // t-caron
        ('\u{fa}', 'u'),  // u-acute
        ('\u{16f}', 'u'), // u-ring
        ('\u{fc}', 'u'),
        ('\u{fd}', 'y'),  // y-acute
        ('\u{17e}', 'z'), // z-caron
        ('\u{142}', 'l'), // l-stroke
        ('\u{f1}', 'n'),  // n-tilde
        ('\u{df}', 's'),  // sharp s
    ];
    let lower = ch.to_lowercase().next()?;
    MAP.iter()
        .find(|(from, _)| *from == lower)
        .map(|(_, to)| *to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_folds_diacritics_and_collapses_separators() {
        assert_eq!(slug("po ztlumeni dveri"), "po-ztlumeni-dveri");
        assert_eq!(
            slug("P\u{159}\u{ed}li\u{161} \u{17e}lu\u{165}ou\u{10d}k\u{fd}"),
            "prilis-zlutoucky"
        );
        assert_eq!(slug("  --  A/B  --  "), "a-b");
    }

    #[test]
    fn slug_of_unusable_input_is_empty() {
        assert_eq!(slug("///"), "");
        assert_eq!(slug(""), "");
    }
}
