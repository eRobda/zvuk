# zvuk

**Measure and tune a car audio system from the command line.**

[![CI](https://github.com/eRobda/zvuk/actions/workflows/ci.yml/badge.svg)](https://github.com/eRobda/zvuk/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

`zvuk` writes a test track, you play it through the car, `zvuk` listens and
tells you what to change. Numbers instead of opinions.

Two design decisions shape everything else:

- **The tool never plays audio.** You play the generated track through the
  system you are measuring. [Why that matters](#why-you-play-the-track-yourself).
- **Every measurement is relative,** so no calibrated microphone is needed.
  [Why that works](#why-every-measurement-is-relative).

*(`zvuk` is Czech for "sound".)*

---

## What it looks like

### 1. Generate the track

```console
$ zvuk generate --module left-right-balance
Wrote signals/zvuk-left-right-balance.wav
  48000 Hz, 2 channels, 20.0 s, peak -6.0 dBFS

Track layout:
     0.0 s   5.0 s  silence (lead-in)
     5.0 s   3.0 s  pink noise, left channel only
     8.0 s   2.0 s  silence
    10.0 s   3.0 s  pink noise, right channel only
    13.0 s   2.0 s  silence
    15.0 s   3.0 s  pink noise, left channel only (repeat, for the sanity check)
    18.0 s   2.0 s  silence (tail)

What to do:
  1. Copy this file to whatever the car plays from: USB stick, phone, CD.
  ...
```

The same instructions are written to `zvuk-left-right-balance.txt` next to the
WAV, so they travel with the file onto the USB stick.

### 2. Record it

```console
$ zvuk --module left-right-balance --label "after door damping"

== Left / right balance ==
Recording. Press play on the track now, then sit still.
Press Enter once the track has finished. (Enter)

Captured 24.4 s. Looking for the bursts...
  burst 1 at    5.1 s, 3.0 s long
  burst 2 at   10.1 s, 3.0 s long
  burst 3 at   15.1 s, 3.0 s long
  left            -34.0 dBFS broadband, peak  -19.2 dBFS
  right           -36.0 dBFS broadband, peak  -20.9 dBFS
  left-recheck    -34.0 dBFS broadband, peak  -19.2 dBFS

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

Sanity check (left measured twice): 0.04 dB apart, broadband
  OK, the measurement is repeatable (limit 0.5 dB).

Recommendation: The left side is 2.0 dB louder. Cut the left channel by 2.0 dB
(or add 2.0 dB to the right).

Saved: measurements/20260909-183012_left-right-balance_after-door-damping.json
```

*The numbers above come from the bundled example measurement and are there to
show the shape of the output, not to describe any particular car.*

Every run is stored as JSON with a timestamp, your label and the exact device
and sample rate used, so you can compare before and after months later.
Try it without any hardware:

```bash
cargo run -- show examples/example-measurement.json
```

---

## Why you play the track yourself

It would be easy for the tool to open the laptop's sound card and play the
noise itself. It deliberately does not, for two reasons.

**You would be measuring the wrong system.** Plugging a laptop into an aux
input bypasses the head unit's DAC, its EQ curve, its loudness contour and
whatever DSP the manufacturer baked in. Those are part of what you hear, and in
a car they are often the largest part. Playing the track from a USB stick means
the measurement covers the whole chain, exactly as you listen to it.

**Most cars have nowhere to plug the laptop in.** Newer head units offer USB
and Bluetooth and no analogue input at all. A tool that requires a cable to the
car simply does not work in those cars.

The cost is that `zvuk` has no idea when you pressed play. That is what the
silences in the generated track are for: the analysis finds the bursts by their
energy and works out the timing from the recording itself. If it cannot find
them cleanly — too much background noise, a passing car, the recording started
too late — it says so instead of returning a number it made up.

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

The generated track plays the left side **twice** — once first and once last.
If the two measurements disagree by more than **0.5 dB** broadband, `zvuk` says
so loudly and tells you not to trust the result.

This is not ceremony. In a car it routinely happens that:

- the microphone shifts (a few centimetres matters in a cabin that small),
- you shift, and change the acoustics yourself,
- a car drives past, the ventilation kicks in, the heater starts.

Without the check you would measure a 1.2 dB imbalance, confidently set the
balance control, and have actually measured your own hand moving. The check
costs five seconds of track and tells you whether the result is worth reading.

The repeat sits at the **end** rather than back-to-back on purpose: that way it
covers the whole session, including the switch to the right channel — exactly
the window in which something can move.

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

On the first recording macOS asks for microphone access. If the dialog never
appears and the capture is dead silence, grant it by hand in *Settings →
Privacy & Security → Microphone*; the permission belongs to the terminal you
launch from.
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
# write the track to play through the car
zvuk generate --module left-right-balance

# what is available
zvuk devices
zvuk modules

# record and analyse: lists inputs, asks which module, asks for a label
zvuk

# non-interactive
zvuk --input 2 --module left-right-balance --label "after door damping"

# print a stored measurement
zvuk show measurements/20260909-183012_left-right-balance_after-door-damping.json
```

### `zvuk generate`

| Flag | Meaning | Default |
| --- | --- | --- |
| `-m`, `--module` | Module to generate the track for | interactive picker |
| `--out-dir` | Where the track is written | `signals/` |
| `--sample-rate` | Sample rate of the file. Try `44100` for an older head unit | `48000` |
| `--duration` | Length of one burst, in seconds | `3.0` |
| `--level` | Peak level of the burst in the file, in dBFS | `-6.0` |
| `--lead-in` | Silence before the first burst, so you can sit down | `5.0` |
| `--gap` | Silence between bursts — this is what separates them | `2.0` |
| `--no-recheck` | Leave out the repeat, and with it the sanity check | off |
| `--force` | Overwrite an existing file | off |

### `zvuk` (record and analyse)

| Flag | Meaning | Default |
| --- | --- | --- |
| `-m`, `--module` | Module to run | interactive picker |
| `-l`, `--label` | Label stored with the result | asked interactively |
| `-i`, `--input` | Input device: index or part of the name | interactive picker |
| `--sample-rate` | Target capture rate; the closest supported one is used | `48000` |
| `--buffer` | Fixed buffer size in samples | driver default |
| `--out-dir` | Where results are written | `measurements/` |
| `--no-save` | Do not write anything to disk | off |

**You are never asked for a sample rate.** The tool reads what the input device
actually supports and picks the value closest to 48 kHz. The generated file's
rate is independent of it and does not have to match.

---

## Modules

| Module | Status | What it answers |
| --- | --- | --- |
| `left-right-balance` | **available** | How many dB louder is one side, broadband and per octave band? |
| `crossover-check` | **available** | Where do two speaker groups hand over, is there a hole, and do their levels match? |
| `arrival-time` | planned | How much delay does each channel need? |
| `subwoofer-phase` | planned | Is the sub in phase — level in the crossover region at 0° vs 180°? |
| `clipping-sweep` | planned | At what volume does the system start to clip? |
| `compare` | planned | What changed between two stored measurements? |

Adding one is meant to be cheap: one new file, one variant in
`MeasurementResult`, one line in the registry. See
[CONTRIBUTING.md](CONTRIBUTING.md).

---

## Crossover and gain staging

`crossover-check` answers the question you actually have when a sub and a set
of door speakers are supposed to work together: **from where to where does each
of them work, where do they hand over, and is one of them running hot?**

You play the same track three times, changing only which speakers are allowed
to make sound:

| Pass | Setup |
| --- | --- |
| 1 | group A only - doors playing, sub muted |
| 2 | group B only - sub playing, doors muted |
| 3 | group A again, exactly as in pass 1 |

The third pass is not a formality. Comparing the level of the doors with the
level of the sub is only meaningful if **the volume knob did not move between
passes** - and neither did the microphone, or you. Pass 3 measures the same
thing as pass 1, so if the two disagree by more than 0.5 dB, something did move
and the comparison is thrown out rather than reported.

Results come in thirds of an octave from 20 Hz, because a subwoofer crosses
somewhere around 80 Hz and whole octaves put their nearest centre at 125 Hz.

```console
     Hz      doors        sub        sum   summed response
----------------------------------------------------------------------------
     50      -43.3      -33.0      -32.6   |======================  |
     63      -40.3      -32.8      -32.0   |======================  |
     80      -37.2      -33.4      -31.9   |======================= |
    100      -34.6      -35.0      -31.8   |======================= | <- crossover
    125      -32.4      -37.7      -31.2   |======================= |
    160      -30.7      -41.5      -30.4   |========================|
----------------------------------------------------------------------------

doors: peaks at 1600 Hz (-30.0 dBFS)
  -6 dB from 90 Hz to 4.2 kHz
  -10 dB from 65 Hz to 5.8 kHz

sub: peaks at 63 Hz (-33.0 dBFS)
  -6 dB from (off the measured range) to 134 Hz
  -10 dB from (off the measured range) to 173 Hz
```

*(Bundled as `examples/example-crossover-check.json`, with synthetic numbers.
`cargo run -- show examples/example-crossover-check.json` prints the whole
thing, recommendation included.)*

Three things to read out of it:

- **Bandwidth.** Where each group is 6 dB down from its own passband. That is
  the honest answer to "from what Hz to what Hz does the midbass work".
- **The crossover.** Where the two curves meet, and how far each has fallen by
  then. Both around -6 dB is the usual target; much less overlap leaves a hole,
  much more leaves a bump.
- **Gain staging.** The difference between the two passband levels, which is
  what you trim at the amplifier rather than at the volume knob.

**One honest limit:** the summed curve is a *magnitude* sum. If it already
shows a hole, no amount of phase alignment will fix it. If it looks flat, phase
can still ruin the real result - that is what the planned `subwoofer-phase`
module is for.

---

## How it works

**Pink noise.** Kellett's filter over a deterministic xorshift generator with a
fixed seed. Every burst in the track is **bit-for-bit the same signal** —
otherwise you are comparing two different things. Each burst has 30 ms
raised-cosine fades so the speaker does not click, and the file is 16-bit PCM
WAV, which is what head units read off a USB stick without complaining.

**Finding the bursts.** The tool does not know when you pressed play, so it
segments the recording itself: short-term RMS in 20 ms frames, a threshold
halfway in dB between the noise floor and the loudest frame, runs merged across
gaps under 0.5 s, anything shorter than a second discarded. It then insists on
finding two or three bursts of similar length, and refuses to guess if it
finds four, or one, or bursts that disagree about how long they are.

**Bands via FFT, not IIR filters.** Welch's method: Hann window, 50% overlap,
averaged power spectra, then the power of every bin inside a band is summed.
The normalisation is chosen so that the power summed over all bins equals the
mean square of the signal, which makes a band RMS directly comparable to the
broadband RMS — and makes it testable.

Resolution follows the question. Whole octaves (8192-point transform) are
enough to tell left from right. Crossover work uses thirds down to 20 Hz, and
the 20 Hz third spans only 17.8–22.4 Hz, so it needs a 32768-point transform to
get more than one bin into that band — and a burst of at least four seconds to
average over. Bands that end up with fewer than three bins are flagged rather
than quietly reported.

**Channel 0** of the input device is what gets recorded. On a stereo interface,
put the microphone in the first input.

### Tests

```bash
cargo test
```

Audio hardware cannot run in CI, so the parts around it carry the test burden:

- **End to end**: a generated track is turned into a synthetic recording of a
  system that is 2 dB louder on the left, then pushed through segmentation and
  analysis. The answer has to come back as 2 dB, broadband and in every band.
- Parseval normalisation holds to within 0.2 dB; a 1 kHz sine lands in the
  1 kHz band at `A/√2` with neighbours more than 40 dB down; a known 3 dB
  attenuation reads as 3.00 ± 0.01 dB in every band.
- **Crossover, end to end**: pink noise is split by a matched pair of 12 dB per
  octave filters at 80 Hz, and the analysis has to find the handover where the
  filters put it, report each side's bandwidth, and see the hole that appears
  when the two filters are pulled two octaves apart.
- Thirds sum back to whole octaves; a sine lands in its third with neighbours
  30 dB down; the bottom third has enough bins to mean something.
- Segmentation finds the right number of bursts, ignores short blips, and
  rejects a recording that is all background rather than returning nonsense.
- The generated track has the right length, puts each burst in the right
  channel only, and carries identical audio in every burst.
- The WAV writer round-trips through a real file.

---

## Project layout

```
src/
  main.rs                     CLI flow: generate, or devices -> module -> save
  cli.rs                      argument definitions (clap)
  measurement.rs              Measurement trait, TestSignal, MeasurementRecord
  registry.rs                 module list  <- one line per new module
  storage.rs                  JSON read/write
  prompt.rs                   stdin interaction
  audio/
    context.rs                AudioContext: capture only, no playback
    devices.rs                input device enumeration and config choice
    signal.rs                 pink noise and the burst track builder
    wav.rs                    16-bit WAV export
  dsp/
    mod.rs                    RMS, peak, dB
    octave.rs                 band analysis via FFT (Welch, Hann, 50% overlap)
    segment.rs                finding the bursts in a recording
  modules/
    left_right_balance.rs     left versus right
    crossover_check.rs        two groups, their handover and gain staging
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
