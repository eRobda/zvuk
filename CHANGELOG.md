# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
