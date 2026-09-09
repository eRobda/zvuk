//! Audio layer: device selection, stream configuration and the primitives
//! that measurement modules build on.

pub mod context;
pub mod devices;
pub mod signal;

// Public surface for measurement modules. `Recording` and `StreamSpec` have no
// user yet, but they are part of the primitives every module receives.
#[allow(unused_imports)]
pub use context::{AudioContext, Recording, RunParams, StreamSpec};
