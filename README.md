# Prisme

DJ software built for DVS: timecode vinyl drives the tracks, with a real-time audio
engine in Rust and a Tauri + Angular interface. Windows only for now.

## Features

- **Vinyl control**: Serato CV02 (sides A and B) and Serato CD timecode, absolute and
  relative modes, needle drops, backspins and scratches; a decoder written from
  scratch. The deck plays on by itself when the needle reaches the end of the record.
- **Mixing**: each deck on its own outputs for a hardware mixer, or a software mixer
  (trim, 3-band EQ with kills, filter, crossfader curves, master limiter, headphone
  cue).
- **Performance**: tempo and beat sync, quantize, 8 hot cues, beat loops, loop in/out,
  loop rolls, beat jumps, slip, censor, key lock and key shift, beat-synced effects,
  8-slot sampler per deck, mix recording with its tracklist.
- **Library**: tempo, beat grid and key analysis in the background, instant search,
  crates and smart crates, prepare list, set history, duplicates, Serato and rekordbox
  import, beat grid and key editing.
- **Control**: keyboard shortcuts, MIDI controllers with MIDI learn and lights, MIDI
  clock out.
- **Reliability**: the audio restarts after a device loss or an engine failure, decks
  come back after a crash, daily logs and a diagnostic report.
- English and French interface.

## Building

Requirements: Windows 10 or 11, Rust (stable), Node.js 24 and pnpm, Python (for the
ASIO bindings' libclang).

```
scripts\setup-dev.ps1
cargo test --workspace
cd apps\prisme-desktop
cargo tauri dev
```

`cargo tauri build` produces the installer in `target\release\bundle\nsis`.

## Architecture

```
apps/
  prisme-desktop/    the application (Tauri v2), frontend in frontend/ (Angular)
  prisme-cli/        the whole chain from the command line, without the interface
crates/
  prisme-timecode/   timecode vinyl decoder: speed, direction, absolute position
  prisme-engine/     real-time engine: decks, vinyl control, mixer, sampler
  prisme-dsp/        real-time processing: filters, EQ, effects, limiter
  prisme-audio-io/   sound cards (WASAPI, ASIO) behind a small interface
  prisme-media/      audio file decoding, tags and cover art
  prisme-analysis/   waveforms, tempo and beat grid, musical key
  prisme-library/    track database, folder scanning, background analysis
  prisme-import/     Serato and rekordbox import
  prisme-control/    MIDI controller messages and mappings
  prisme-session/    settings files with atomic writes
```

The audio thread never allocates, locks or frees: the interface sends commands through
a lock-free queue, the engine publishes its state in a triple buffer, and what it
replaces (tracks, decoders) goes back to the control side to be freed there.

## Command line

```
prisme devices
prisme play "track.mp3" --pitch 1.02
prisme simulate "track.mp3" --pattern scratch --gain 0.3
prisme vinyl "track.mp3" --input "interface name" --mode absolute
prisme analyze "Music folder" --csv results.csv
prisme latency --input "interface name"
```

`simulate` generates the signal of a turntable, to try vinyl control without hardware.
`latency` measures the round trip through a cable from an output to an input: it plays
clicks at full level, so keep speakers and headphones away from that output.

## License

Prisme is free software: you can redistribute it and/or modify it under the terms of
the GNU General Public License as published by the Free Software Foundation, either
version 3 of the License, or (at your option) any later version. See [LICENSE](LICENSE).