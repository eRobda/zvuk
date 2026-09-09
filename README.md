# zvuk

**Measure and tune a car audio system from the command line.**

[![CI](https://github.com/eRobda/zvuk/actions/workflows/ci.yml/badge.svg)](https://github.com/eRobda/zvuk/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

Point a microphone at the driver's seat, plug your laptop into the head unit,
and get numbers instead of opinions. `zvuk` plays test signals, records what the
car does to them, and tells you what to change.

No calibrated microphone required — and that is a design decision, not a
shortcut. [Here is why](#why-every-measurement-is-relative).

*(`zvuk` is Czech for "sound".)*

---

## What it looks like

```console
$ zvuk --module left-right-balance --label "after door damping"

== Left / right balance ==
Before you start:
  1) Put the microphone at the listening position and DO NOT MOVE IT.
  2) Engine off, ventilation off, windows closed.
  3) Stay quiet and still while it runs.
  Pink noise will play for 3.0 s per channel at -6.0 dBFS peak.

Measure the left side twice to verify the result is repeatable (recommended)? [Y/n]: y
Press Enter once it is quiet and the microphone is in place. (Enter)

-> Measuring the left channel, hold still...
   done: -34.0 dBFS broadband, peak -19.2 dBFS, 105600 samples analysed

-> Measuring the right channel, hold still...
   done: -36.0 dBFS broadband, peak -20.9 dBFS, 105600 samples analysed

-> Measuring the left-recheck channel, hold still...
   done: -34.0 dBFS broadband, peak -19.2 dBFS, 105600 samples analysed

       Band         Left        Right         Diff   louder
------------------------------------------------------------------
     125 Hz     -40.9 dB     -43.0 dB      +2.0 dB   left
     250 Hz     -40.0 dB     -42.0 dB      +2.0 dB   left
     500 Hz     -39.2 dB     -41.2 dB      +2.0 dB   left
    1000 Hz     -40.5 dB     -42.5 dB      +2.0 dB   left
    2000 Hz     -41.9 dB     -43.9 dB      +1.9 dB   left
    4000 Hz     -44.4 dB     -46.4 dB      +1.9 dB   left
    8000 Hz     -48.0 dB     -49.9 dB      +1.9 dB   left
------------------------------------------------------------------
  broadband     -34.0 dB     -36.0 dB      +2.0 dB   left

Values are relative (dBFS at the input); do not read absolute SPL from them.

Sanity check (left channel measured twice): 0.04 dB apart, broadband
  OK, the measurement is repeatable (limit 0.5 dB).

Recommendation: The left side is 2.0 dB louder. Cut the left channel by 2.0 dB
(or add 2.0 dB to the right).

Saved: measurements/20260909-183012_left-right-balance_after-door-damping.json
```

*The numbers above come from the bundled example measurement and are there to
show the shape of the output, not to describe any particular car.*

Every run is stored as JSON with a timestamp, your label and the exact devices
and sample rates used, so you can compare before and after months later.
Try it without any hardware:

```bash
cargo run -- show examples/example-measurement.json
```

---

## Why every measurement is relative

**The microphone is never calibrated, and it never needs to be.**

Every microphone has its own response curve — a few dB up here, a few dB down
there. Measuring the *absolute* frequency response of a car means knowing that
curve and subtracting it, which takes a calibrated microphone and a calibration
file.

But every measurement in this tool compares **two situations captured with the
same microphone in the same position**:

- left channel against right channel,
- subwoofer in phase against out of phase,
- before a change against after it.

The microphone's error is identical in both captures, so it **cancels in the
difference**. A cheap electret capsule gives you the same left/right delta as a
measurement rig costing a thousand times more.

### What the numbers do and do not tell you

| The numbers tell you | The numbers do not tell you |
| --- | --- |
| How many dB louder one side is, overall and per band | Absolute SPL — values are dBFS at the sound card input |
| Whether that difference changes with frequency, which points at placement or reflections rather than gain | Whether the car has a "flat" response — most of the curve you see is the microphone |
| Whether the change you just made actually did something | Anything about sound quality. Balanced is not the same as good |

The practical consequence: **do not compare measurements taken with different
microphones or from different positions.** Every stored JSON records the device
it came from, precisely so you can check.

---

## The sanity check, and why it is not optional

The `left-right-balance` module offers to measure the left side **twice** — once
at the start and once at the very end of the run. If the two disagree by more
than **0.5 dB** broadband, it says so loudly and tells you not to trust the
result.

This is not ceremony. In a car it routinely happens that:

- the microphone shifts (a few centimetres matters in a cabin that small),
- you shift, and change the acoustics yourself,
- a car drives past, the ventilation kicks in, the heater starts.

Without the check you would measure a 1.2 dB imbalance, confidently set the
balance control, and have actually measured your own hand moving. The check
costs three seconds and tells you whether the result is worth reading at all.

It sits at the **end** rather than back-to-back on purpose: that way it covers
the whole session, including the switch to the right channel — exactly the
window in which something can move.

---

## Install

You need a [Rust toolchain](https://rustup.rs) (1.85 or newer).

```bash
git clone https://github.com/eRobda/zvuk
cd zvuk
cargo build --release
```

The binary lands in `target/release/zvuk` (`zvuk.exe` on Windows).

<details>
<summary><b>macOS</b></summary>

Nothing else to install — `cpal` talks to CoreAudio directly.

On the first run macOS asks for microphone access. If the dialog never appears
and the capture is dead silence, grant it by hand in *Settings → Privacy &
Security → Microphone*; the permission belongs to the terminal you launch from.
</details>

<details>
<summary><b>Windows</b></summary>

`cpal` uses WASAPI, so no extra SDK is needed — but **use the MSVC toolchain**:

```bash
rustup default stable-x86_64-pc-windows-msvc
```

together with the Visual Studio Build Tools "Desktop development with C++"
workload. The GNU toolchain also works, but only with a full MinGW-w64 on
`PATH`; the self-contained one rustup ships lacks binutils (`dlltool`, `as`),
and crates such as `chrono` and `windows-sys` then link into a binary that
crashes on startup.

ASIO is not enabled. It needs the ASIO SDK and the `asio` feature of `cpal`, and
these measurements are deliberately insensitive to latency, so it buys nothing.
</details>

<details>
<summary><b>Linux</b></summary>

Not a target platform (this is a tool for a laptop in a car), but it builds and
the test suite runs in CI. You need ALSA headers:

```bash
sudo apt-get install libasound2-dev
```
</details>

---

## Usage

```bash
# what is available
zvuk devices
zvuk modules

# interactive run: lists devices, asks which module, asks for a label
zvuk

# non-interactive, with explicit parameters
zvuk --input 2 --output "Head unit" --module left-right-balance --label "after door damping"

# print a stored measurement
zvuk show measurements/20260909-183012_left-right-balance_after-door-damping.json
```

| Flag | Meaning | Default |
| --- | --- | --- |
| `-m`, `--module` | Module to run | interactive picker |
| `-l`, `--label` | Label stored with the result | asked interactively |
| `-i`, `--input` | Input device: index or part of the name | interactive picker |
| `-o`, `--output` | Output device: index or part of the name | interactive picker |
| `--sample-rate` | Target rate; the closest supported one is used | `48000` |
| `--duration` | Test signal length in seconds | `3.0` |
| `--level` | Test signal level in dBFS (peak) | `-6.0` |
| `--buffer` | Fixed buffer size in samples | driver default |
| `--out-dir` | Where results are written | `measurements/` |
| `--no-save` | Do not write anything to disk | off |

**You are never asked for a sample rate.** The tool reads what each device
actually supports and picks the value closest to 48 kHz. Input and output choose
independently — for noise measurements that is harmless, and the analysis runs
at the input rate.

### One device for both input and output

On Windows a single device cannot always be opened for input and output at the
same time, particularly under ASIO or in exclusive mode. `zvuk` warns you when
you pick the same device for both, and if the stream then fails it says so in
plain language instead of dumping a driver error. The fix is to pick a different
output device.

---

## Modules

| Module | Status | What it answers |
| --- | --- | --- |
| `left-right-balance` | **available** | How many dB louder is one side, broadband and per octave band? |
| `arrival-time` | planned | How much delay does each channel need? |
| `subwoofer-phase` | planned | Is the sub in phase — level in the crossover region at 0° vs 180°? |
| `clipping-sweep` | planned | At what volume does the system start to clip? |
| `compare` | planned | What changed between two stored measurements? |

Adding one is meant to be cheap: one new file, one variant in
`MeasurementResult`, one line in the registry. See
[CONTRIBUTING.md](CONTRIBUTING.md).

---

## How it works

**Pink noise.** Kellett's filter over a deterministic xorshift generator with a
fixed seed. Both sides get **bit-for-bit the same signal** — otherwise you are
comparing two different things. The burst has 30 ms raised-cosine fades so the
speaker does not click.

**Octave bands via FFT, not IIR filters.** Welch's method: 8192-sample Hann
window, 50% overlap, averaged power spectra, then the power of every bin inside
`[fc/√2, fc·√2)` is summed. The normalisation is chosen so that the power summed
over all bins equals the mean square of the signal, which makes a band RMS
directly comparable to the broadband RMS — and makes it testable.

**Timing.** Recording starts 400 ms before playback and continues 400 ms after,
and the `Recording` remembers the sample index at which playback began. The
analysis uses only the steady-state middle, so neither output latency nor room
decay leaks into the result.

**Channel 0** of the input device is what gets recorded. On a stereo interface,
put the microphone in the first input.

### Tests

```bash
cargo test
```

Audio hardware cannot run in CI, so the DSP carries the test burden:

- Parseval normalisation holds to within 0.2 dB,
- a 1 kHz sine lands in the 1 kHz band at `A/√2`, with neighbouring bands more
  than 40 dB down,
- a known 3 dB attenuation reads as 3.00 ± 0.01 dB **in every band** — which is
  exactly the quantity the balance module reports,
- the pink noise generator is deterministic, correctly scaled and fades to
  silence at both ends.

---

## Project layout

```
src/
  main.rs                     CLI flow: devices -> context -> module -> save
  cli.rs                      argument definitions (clap)
  measurement.rs              Measurement trait, MeasurementResult, MeasurementRecord
  registry.rs                 module list  <- one line per new module
  storage.rs                  JSON read/write
  prompt.rs                   stdin interaction
  audio/
    context.rs                AudioContext + the three primitives
    devices.rs                device enumeration and closest supported config
    signal.rs                 pink noise generator
  dsp/
    mod.rs                    RMS, peak, dB
    octave.rs                 octave analysis via FFT (Welch, Hann, 50% overlap)
  modules/
    left_right_balance.rs     the one implemented module
```

---

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
