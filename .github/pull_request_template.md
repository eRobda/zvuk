## What this changes

<!-- One or two sentences. Link the issue if there is one. -->

## Why

<!-- What was wrong, or what became possible. -->

## Checklist

- [ ] `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` are clean
- [ ] `cargo test` passes
- [ ] New behaviour is covered by a test, or I explain below why it cannot be

## For a new measurement module

- [ ] It lives in its own file under `src/modules/`
- [ ] It is registered with one line in `registry::all()`
- [ ] Its result type has a variant in `MeasurementResult` and serialises to JSON
- [ ] **It has a sanity check** that tells the user when the result is untrustworthy
- [ ] The measurement is relative - it does not assume a calibrated microphone

## Verification

<!--
How did you check this on real hardware? Which OS, which devices?
"Not tested on hardware" is an acceptable answer - just say so.
-->
