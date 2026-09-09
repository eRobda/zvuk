# Contributing

Thanks for looking. This is a small, opinionated tool, so it helps to know the
opinions before you write code.

## The four rules

1. **The tool never plays audio.** A module that needs a stimulus returns a
   `TestSignal` from `Measurement::signal`, `zvuk generate` writes it to a WAV,
   and the user plays it through the system being measured. This is not a
   limitation to work around: it is what makes the measurement cover the head
   unit, its EQ and the amplifier instead of bypassing them. There is no output
   device anywhere in the code, and adding one would defeat the point.
2. **Every measurement is relative.** Two situations, same microphone, same
   position. Nothing here assumes a calibrated microphone, and nothing should
   start to. If a feature needs absolute SPL, it belongs in a different tool.
3. **Every module has a sanity check.** A measurement that cannot tell you when
   it is wrong is worse than no measurement, because you will act on it. See
   [`left_right_balance.rs`](src/modules/left_right_balance.rs) for the pattern:
   measure one side twice and compare.
4. **Adding a module must stay cheap.** One new file, one variant, one line in
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
2. Implement `signal()` if the module needs the user to play something. Return
   a `TestSignal` with the samples, a layout description and the instructions
   that get written next to the WAV.
3. Add a result struct and give it a `Box`ed variant in `MeasurementResult`
   (`src/measurement.rs`) so it serialises to JSON. Every variant is boxed:
   an enum is as big as its largest arm, and these structs carry whole
   response curves.
4. Add `pub mod your_module;` to `src/modules/mod.rs`.
5. Add one line to `registry::all()` in `src/registry.rs`.

Your module receives an `AudioContext` with these primitives:

| Primitive | What it does |
| --- | --- |
| `ctx.record(seconds)` | Record N seconds from the microphone (mono, channel 0) |
| `ctx.record_while(work)` | Record for as long as `work` takes - that is how you wait for the user |

There is no `play`. See rule 1.

For the analysis side, `dsp::segment::find_bursts` turns a capture into the
loud sections it contains, and `Burst::trimmed` drops the fade ramps from each
end. `signal::burst_track` builds a stereo track of bursts separated by
silence, which is the shape `find_bursts` is designed to recover.

`dsp::octave::fractional_octave_bands` does the band analysis at whatever
resolution you ask for. Use whole octaves when you are comparing broad levels
and thirds when a crossover frequency matters - but remember that thirds at the
bottom of the range need both a long transform and a long burst, or the lowest
bands end up with almost no FFT bins in them.

A module that needs the user to reconfigure the system between passes should
say so in its `TestSignal` instructions and record each pass separately with
`record_while`. See `crossover_check.rs`, where the third pass repeats the
first precisely so a moved volume knob cannot pass unnoticed.

## Testing DSP code

Audio hardware cannot be in CI, so the signal processing carries the test
burden. Write tests against signals with a known answer - a sine of known
amplitude must land in the right band at `A/sqrt(2)`, a 3 dB attenuation must
read as 3.00 dB in every band. See `src/dsp/octave.rs` for examples.

Better still, test the whole chain. Because the tool generates its own
stimulus, a module can build its track, turn it into a synthetic recording of a
system with a known fault, and assert that the fault comes back out. See
`a_known_imbalance_comes_back_out_of_the_whole_chain` in
`src/modules/left_right_balance.rs`; a new module should have its equivalent.

If a change cannot be tested without hardware, say so in the pull request and
describe what you checked by hand and on which devices.

## Reporting a measurement that looks wrong

Attach the JSON from your `measurements/` directory and the output of
`zvuk devices`, and say what you played the track from. Numbers beat
descriptions.
