# Prisme

DJ software built for DVS: timecode vinyl drives the tracks, with a real-time audio
engine in Rust and a Tauri + Angular interface. For Windows, Linux and macOS: the last
two are new, and tried on fewer sound cards.

## Features

- **Vinyl control**: Serato CV02 (sides A and B) and Serato CD timecode, absolute and
  relative modes, needle drops, backspins and scratches; a decoder written from
  scratch. The deck plays on by itself when the needle reaches the end of the record.
- **Mixing**: each deck on its own outputs for a hardware mixer, or a software mixer
  (trim, 3-band EQ with kills, filter, crossfader curves, master limiter, headphone
  cue); auto gain plays every track at the same loudness. Two decks or four: on an
  interface for two, the third and fourth are layers over the first two.
- **Performance**: tempo and beat sync, quantize, 8 hot cues, beat loops, loop in/out,
  loop rolls, beat jumps, slip, censor, key lock and key shift, beat-synced effects,
  8-slot sampler per deck, instant doubles, mix recording with its tracklist.
- **Stems**: vocals, drums, bass and melody, separated on the GPU (on its tensor cores
  where it has some) or the processor, from where the deck plays as soon as a track is
  loaded; a knob per part, cut and brought back on the bar with quantize, and lanes
  showing where each part plays.
- **Library**: MP3, AAC, ALAC, FLAC, Ogg Vorbis, WAV and AIFF; tempo, beat grid and key
  analysis in the background, instant search,
  crates and smart crates, prepare list, set history, duplicates, Serato and rekordbox
  import, beat grid and key editing. A file renamed, moved, tagged again or on a drive
  under another letter is known again, by its audio if need be, and keeps its cues.
- **Control**: keyboard shortcuts, MIDI controllers with MIDI learn and lights, MIDI
  clock out.
- **Reliability**: the audio restarts after a device loss or an engine failure, decks
  come back after a crash, daily logs and a diagnostic report.
- English and French interface.

## Building

Requirements: Rust (stable), Node.js 24 and pnpm, Python (it fetches the libclang the
C++ bindings are generated with), and:

- Windows 10 or 11;
- macOS 13.3 or later, with the Xcode command line tools;
- Linux: a C++ compiler and the libraries of the web view, ALSA and JACK. On Debian and
  Ubuntu: `sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev
  libssl-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev libjack-jackd2-dev
  libudev-dev`.

```
scripts/setup-dev.sh        (scripts\setup-dev.ps1 on Windows)
cargo test --workspace
cd apps/prisme-desktop
cargo tauri dev
```

`cargo tauri build` produces the packages in `target/release/bundle`: the installer on
Windows, a `.deb` and an AppImage on Linux, a `.dmg` on macOS.

On Linux, Prisme plays through ALSA, or through JACK (PipeWire's included) for the
lowest latency. The macOS build is not notarized by Apple: the first time, open it from
System Settings > Privacy & Security.

## Releasing

1. Set the new version in the workspace `Cargo.toml` (`[workspace.package] version`).
2. Push a tag with that version, such as `0.2.0`: CI builds the packages of the three
   systems and their signed update packages and attaches them to a draft release.
3. Publish the draft: the installed copies then offer the update.

## Architecture

```
apps/
  prisme-desktop/    the application (Tauri v2), frontend in frontend/ (Angular)
  prisme-cli/        the whole chain from the command line, without the interface
crates/
  prisme-timecode/   timecode vinyl decoder: speed, direction, absolute position
  prisme-engine/     real-time engine: decks, vinyl control, mixer, sampler
  prisme-dsp/        real-time processing: filters, EQ, effects, limiter
  prisme-audio-io/   sound cards (WASAPI, ASIO, CoreAudio, ALSA, JACK) behind a small interface
  prisme-media/      audio file decoding, tags and cover art
  prisme-analysis/   waveforms, tempo and beat grid, musical key
  prisme-library/    track database, folder scanning, background analysis
  prisme-stems/      source separation (Mel-Band RoFormer) on the GPU (wgpu) or the CPU
  prisme-import/     Serato and rekordbox import
  prisme-control/    MIDI controller messages and mappings
  prisme-session/    settings files with atomic writes
design/              the design system: DESIGN.md and the reference screens
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

The separation models, downloaded separately, are Kimberley Jensen's
[Mel-Band RoFormer](https://huggingface.co/KimberleyJSN/melbandroformer) (MIT licence) for
the vocals, and SYH99999's
[four-stem Mel-Band RoFormer](https://huggingface.co/SYH99999/MelBandRoformer4StemFTLarge)
(Apache 2.0 licence) for the drums and bass.
