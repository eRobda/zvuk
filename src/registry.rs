//! Measurement module registry.
//!
//! Adding a module = one new file in `src/modules/`, one `pub mod` line in
//! `src/modules/mod.rs`, and one line down here.

use crate::measurement::Measurement;
use crate::modules::left_right_balance::LeftRightBalance;

pub fn all() -> Vec<Box<dyn Measurement>> {
    vec![
        Box::new(LeftRightBalance),
        // <-- register the next module here
    ]
}

pub fn find(name: &str) -> Option<Box<dyn Measurement>> {
    let needle = name.trim();
    all()
        .into_iter()
        .find(|m| m.name().eq_ignore_ascii_case(needle))
}
