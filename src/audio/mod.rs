//! Audio layer: input device selection, capture, test signal generation and
//! WAV export.
//!
//! The tool has no output side on purpose - see [`context`].

pub mod context;
pub mod devices;
pub mod signal;
pub mod wav;

// Public surface for measurement modules. `Capture` and `StreamSpec` have no
// user outside the audio layer yet, but they are part of what a module sees.
#[allow(unused_imports)]
pub use context::{AudioContext, Capture, StreamSpec};
