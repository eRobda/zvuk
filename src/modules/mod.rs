//! Measurement modules. Each module lives in its own file and is registered
//! with a single line in [`crate::registry`].

pub mod crossover_check;
pub mod left_right_balance;

// Planned modules (not implemented yet):
// pub mod arrival_time;      // per-channel arrival time for setting delays
// pub mod subwoofer_phase;   // level in the crossover region at 0 deg vs 180 deg
// pub mod clipping_sweep;    // clipping detection on a rising volume ramp
// pub mod compare;           // diff curve between two stored measurements
