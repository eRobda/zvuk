# Contributing

Thanks for looking. This is a small, opinionated tool, so it helps to know the
opinions before you write code.

## The three rules

1. **Every measurement is relative.** Two situations, same microphone, same
   position. Nothing here assumes a calibrated microphone, and nothing should
   start to. If a feature needs absolute SPL, it belongs in a different tool.
2. **Every module has a sanity check.** A measurement that cannot tell you when
   it is wrong is worse than no measurement, because you will act on it. See
   [`left_right_balance.rs`](src/modules/left_right_balance.rs) for the pattern:
   measure one side twice and compare.
3. **Adding a module must stay cheap.** One new file, one variant, one line in
   the registry. If your change makes that harder, rethink it.

## Getting set up

```bash
git clone https://github.com/eRobda/zvuk
cd zvuk
cargo build
cargo test
```

On Linux you also need ALSA headers: `sudo apt-get install libasound2-dev`.

On Windows use the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`)
together with the Visual Studio Build Tools C++ workload.

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs exactly these on Linux, Windows and macOS, plus a build against the
minimum supported Rust version from `rust-version` in `Cargo.toml`.

## Branches and commits

- Branch off `main`: `feat/arrival-time`, `fix/wasapi-device-name`, `docs/readme`.
- Commit subjects in the imperative mood, under ~70 characters:
  `Add arrival-time module`, not `added stuff`.
- Small commits that each build are better than one that does everything.
- `main` is protected; everything lands through a pull request.

## Adding a measurement module

1. Create `src/modules/your_module.rs` and implement the `Measurement` trait.
2. Add a result struct and give it a variant in `MeasurementResult`
   (`src/measurement.rs`) so it serialises to JSON.
3. Add `pub mod your_module;` to `src/modules/mod.rs`.
4. Add one line to `registry::all()` in `src/registry.rs`.

Your module receives an `AudioContext` with these primitives:

| Primitive | What it does |
| --- | --- |
| `ctx.play(&interleaved)` | Play a buffer and wait until it finishes |
| `ctx.record(seconds)` | Record N seconds from the microphone (mono, channel 0) |
| `ctx.play_and_record(&interleaved, tail)` | Play and record at once, returns a `Recording` |
| `ctx.mono_to_channel(&mono, ch)` | Spread mono into one output channel |
| `ctx.mono_to_all(&mono)` | Spread mono into every output channel |

`Recording::steady_state(guard)` gives you the part of the capture without the
output latency at the start and the room decay at the end. That is what you want
to analyse.

## Testing DSP code

Audio hardware cannot be in CI, so the signal processing carries the test
burden. Write tests against signals with a known answer - a sine of known
amplitude must land in the right band at `A/sqrt(2)`, a 3 dB attenuation must
read as 3.00 dB in every band. See `src/dsp/octave.rs` for examples.

If a change cannot be tested without hardware, say so in the pull request and
describe what you checked by hand and on which devices.

## Reporting a measurement that looks wrong

Attach the JSON from your `measurements/` directory and the output of
`zvuk devices`. Numbers beat descriptions.
