# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **The tool no longer plays audio.** `zvuk generate` writes a 16-bit WAV test
  track, the user plays it through the system being measured, and `zvuk` only
  records. Measuring through the head unit means the result covers its DAC, EQ
  and amplifier instead of bypassing them via an aux input - and it works in
  cars that have no analogue input at all.
- The output device is gone from device selection, from `AudioContext` and from
  `zvuk devices`. With it went the Windows "same device for input and output"
  problem.
- `AudioContext` now offers `record` and `record_while` instead of `play`,
  `record` and `play_and_record`.
- Storage schema bumped to 2: `ContextInfo` no longer has output device fields,
  and a channel measurement records where its burst sat in the recording.

### Added

- `zvuk generate`, with flags for burst length, level, lead-in, gap, file
  sample rate and whether to include the repeat burst. It writes the WAV plus a
  text file of instructions that travels with it onto the USB stick.
- `Measurement::signal`, so a module describes the track it needs rather than
  playing anything.
- `dsp::segment`: burst detection by short-term RMS, since the tool no longer
  knows when playback started. It refuses to guess when it finds the wrong
  number of bursts or bursts of unequal length.
- An end-to-end test that generates a track, synthesises a recording of a
  system 2 dB louder on the left, and asserts the analysis says 2 dB.


## [0.1.0] - 2026-09-09

First tagged version: the measurement core plus one module. The signal
processing is covered by tests; the audio path has not yet been exercised
against a car.

### Added

- `Measurement` trait, module registry, and JSON storage with timestamp,
  user label and the device context a measurement was taken with.
- `AudioContext` with three primitives for modules: play a buffer, record N
  seconds, play and record simultaneously. Recording is anchored to the sample
  at which playback started, so the steady-state segment can be extracted
  without output latency or room decay.
- Device enumeration that picks the closest supported sample rate to 48 kHz
  instead of asking the user, independently for input and output.
- `left-right-balance` module: pink noise per channel, broadband RMS plus RMS
  in the 125 Hz to 8 kHz octave bands via FFT, a difference table, and a
  balance recommendation in dB.
- Repeatability sanity check: the left side is optionally measured twice, and a
  disagreement of more than 0.5 dB broadband marks the result untrustworthy.
- `zvuk devices`, `zvuk modules` and `zvuk show` subcommands.
- Test suite covering the DSP: Parseval normalisation, band placement of a
  known sine, exactness of a known attenuation per band, and determinism of the
  noise generator.

[Unreleased]: https://github.com/eRobda/zvuk/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/eRobda/zvuk/releases/tag/v0.1.0
