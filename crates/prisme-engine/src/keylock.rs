//! Key lock: the tempo changes, the pitch does not. The track is read at the deck's
//! speed and goes through a time-stretcher (Signalsmith Stretch) that plays it back at
//! the original pitch, or transposed by whole semitones (key shift).
//!
//! The stretcher delays what it is given; it is fed ahead of the playhead by that delay
//! so that what is heard is where the playhead is. After a jump the feed starts again
//! from the new place; in a loop it wraps with the playhead. Out of
//! [`KeyLock::applies`] (scratching, backwards, very slow or fast) the deck plays
//! without it.
//!
//! Parts of the track read apart from the rest (for an effect on them alone) go through a
//! stretcher of four channels, the rest and the parts, which stretches both alike so that
//! they still add up. Going from one stretcher to the other crossfades, once the one coming
//! in, started silent for its latency, plays.

use crate::stems::Reader;
use signalsmith_stretch::Stretch;

const MIN_SPEED: f64 = 0.5;
const MAX_SPEED: f64 = 2.0;
/// A feed further than this from where it should be means the playhead jumped.
const JUMP_FRAMES: f64 = 64.0;
/// The highest sample rate of a track the input is sized for (studio masters reach 384 kHz):
/// a block reads that many more track frames than it plays at the output rate.
const MAX_TRACK_RATE: f64 = 384_000.0;
/// How long going from the whole track to parts apart, or back, crossfades (output frames).
const SWITCH_FRAMES: usize = 1024;
/// Channels of the whole track, and of the rest with the parts apart.
const WHOLE: usize = 2;
const APART: usize = 4;

/// A time-stretcher following the playhead, or not.
struct Stretcher {
    stretch: Stretch,
    channels: usize,
    engaged: bool,
    /// Track sample rate and key shift the transposition was set for.
    transposed_for: Option<(u32, i8)>,
}

impl Stretcher {
    fn new(channels: usize, out_rate: u32) -> Self {
        Self {
            stretch: Stretch::preset_cheaper(channels as u32, out_rate),
            channels,
            engaged: false,
            transposed_for: None,
        }
    }

    /// Plays a track at `track_rate` at its pitch moved by `key_shift` semitones, its samples
    /// being played at the output rate (`ratio`: track rate / output rate).
    fn transpose(&mut self, track_rate: u32, key_shift: i8, ratio: f64) {
        if self.transposed_for != Some((track_rate, key_shift)) {
            let factor = ratio * 2f64.powf(f64::from(key_shift) / 12.0);
            self.stretch.set_transpose_factor(factor as f32, None);
            self.transposed_for = Some((track_rate, key_shift));
        }
    }

    /// Restarts with `preroll`, what comes before the feed.
    fn start(&mut self, preroll: &[f32], speed: f64) {
        self.stretch.reset();
        self.stretch.seek(preroll, speed);
        self.engaged = true;
    }
}

pub(crate) struct KeyLock {
    whole: Stretcher,
    apart: Stretcher,
    /// Parts are read apart from the rest now.
    setting_apart: bool,
    /// Output frames left until the other stretcher is no longer heard: the one coming in
    /// gets going, then the two crossfade over the last [`SWITCH_FRAMES`].
    switching: usize,
    /// Interleaved input, four channels wide, sized for the longest block at the highest
    /// speed and track rate.
    input: Vec<f32>,
    /// The rest and the parts added up, for the whole track's stretcher while both run.
    added: Vec<f32>,
    /// What the stretcher of the parts apart gives, four channels wide.
    stretched: Vec<f32>,
    /// Source frame (fractional) of the next input.
    feed: f64,
    key_shift: i8,
    out_rate: f64,
}

impl KeyLock {
    pub(crate) fn new(out_rate: u32, max_block_frames: usize) -> Self {
        let whole = Stretcher::new(WHOLE, out_rate);
        let rates = (MAX_TRACK_RATE / f64::from(out_rate)).max(1.0);
        let longest = (max_block_frames as f64 * MAX_SPEED * rates).ceil() as usize + 2;
        let frames = longest.max(whole.stretch.input_latency() * 2);
        Self {
            whole,
            apart: Stretcher::new(APART, out_rate),
            setting_apart: false,
            switching: 0,
            input: vec![0.0; frames * APART],
            added: vec![0.0; frames * WHOLE],
            stretched: vec![0.0; max_block_frames * APART],
            feed: 0.0,
            key_shift: 0,
            out_rate: f64::from(out_rate),
        }
    }

    /// Whether the key lock can play at `speed`.
    pub(crate) fn applies(speed: f64) -> bool {
        (MIN_SPEED..=MAX_SPEED).contains(&speed)
    }

    pub(crate) fn key_shift(&self) -> i8 {
        self.key_shift
    }

    pub(crate) fn set_key_shift(&mut self, semitones: i8) {
        self.key_shift = semitones;
    }

    /// Stops following the playhead (the deck plays without the key lock).
    pub(crate) fn disengage(&mut self) {
        self.whole.engaged = false;
        self.apart.engaged = false;
        self.switching = 0;
    }

    /// Renders `out` from `source` at `speed`, the playhead being at `position` (source
    /// frames) in the loop `loop_frames`; returns where the playhead is afterwards. With
    /// `apart`, the parts the source sets apart go there and the rest to `out`.
    pub(crate) fn render(
        &mut self,
        source: &mut Reader,
        position: f64,
        loop_frames: Option<(f64, f64)>,
        out: &mut [f32],
        apart: Option<&mut [f32]>,
        speed: f64,
    ) -> f64 {
        let ratio = source.sample_rate() as f64 / self.out_rate;
        for stretcher in [&mut self.whole, &mut self.apart] {
            stretcher.transpose(source.sample_rate(), self.key_shift, ratio);
        }
        let stretch = &self.whole.stretch;
        let lead = stretch.input_latency() as f64 + stretch.output_latency() as f64 * speed * ratio;
        let wanted = position + lead;
        let setting_apart = apart.is_some();
        let jumped = (self.feed - wanted).abs() > JUMP_FRAMES;
        if jumped || !(self.whole.engaged || self.apart.engaged) {
            self.disengage();
            self.feed = wanted.floor();
            self.setting_apart = setting_apart;
        }
        if setting_apart != self.setting_apart {
            // the other stretcher still plays while this one comes in
            self.setting_apart = setting_apart;
            self.switching = self.whole.stretch.output_latency() + SWITCH_FRAMES;
        }
        if !self.stretchers().0.engaged {
            self.start(source, loop_frames, speed);
        }
        if self.switching == 0 {
            self.stretchers().1.engaged = false;
        }

        let out_frames = out.len() / 2;
        let advance = out_frames as f64 * speed * ratio;
        let start = self.feed.floor() as isize;
        // a track above MAX_TRACK_RATE is read short rather than overflowing the input
        let frames = (((self.feed + advance).floor() - start as f64) as usize).min(self.input.len() / APART);
        let channels = if self.apart.engaged { APART } else { WHOLE };
        let input = &mut self.input[..frames * channels];
        read(source, start, loop_frames, input, channels, true);
        if self.apart.engaged {
            self.apart.stretch.process(&*input, &mut self.stretched[..out_frames * APART]);
        }
        if self.whole.engaged {
            let whole_input = if channels == APART {
                add_up(input, &mut self.added[..frames * WHOLE]);
                &self.added[..frames * WHOLE]
            } else {
                &*input
            };
            self.whole.stretch.process(whole_input, &mut *out);
        }
        self.mix(out, apart);
        self.feed += advance;

        let after = crate::playhead::wrap_in_loop(position, position + advance, loop_frames);
        // the playhead wrapped: the feed, ahead of it, wraps with it
        self.feed += after - (position + advance);
        after
    }

    /// The stretcher of the way the track is read now, and the other.
    fn stretchers(&mut self) -> (&mut Stretcher, &mut Stretcher) {
        if self.setting_apart {
            (&mut self.apart, &mut self.whole)
        } else {
            (&mut self.whole, &mut self.apart)
        }
    }

    /// Restarts the stretcher of the way the track is read now with what comes before the
    /// feed.
    fn start(&mut self, source: &mut Reader, loop_frames: Option<(f64, f64)>, speed: f64) {
        let feed = self.feed as isize;
        let Self { whole, apart, input, setting_apart, .. } = self;
        let now = if *setting_apart { apart } else { whole };
        let preroll = now.stretch.input_latency();
        let input = &mut input[..preroll * now.channels];
        read(source, feed - preroll as isize, loop_frames, input, now.channels, false);
        now.start(input, speed);
    }

    /// What the stretchers gave into `out` and `apart`: the rest and the parts apart, or the
    /// whole track, crossfading from one to the other while switching.
    fn mix(&mut self, out: &mut [f32], mut apart: Option<&mut [f32]>) {
        let stretched = &self.stretched[..out.len() * 2];
        if !self.apart.engaged {
            if let Some(apart) = apart {
                apart.fill(0.0);
            }
            return;
        }
        for (i, (frame, parts)) in out.chunks_exact_mut(2).zip(stretched.chunks_exact(APART)).enumerate() {
            // how much the parts apart are heard: 1 once switched to them, 0 once switched away
            let left = self.switching.saturating_sub(i).min(SWITCH_FRAMES) as f32 / SWITCH_FRAMES as f32;
            let heard = if self.setting_apart { 1.0 - left } else { left };
            let whole = if self.whole.engaged { 1.0 - heard } else { 0.0 };
            match apart.as_deref_mut() {
                Some(apart) => {
                    frame[0] = whole * frame[0] + heard * parts[0];
                    frame[1] = whole * frame[1] + heard * parts[1];
                    apart[2 * i] = heard * parts[2];
                    apart[2 * i + 1] = heard * parts[3];
                }
                None => {
                    frame[0] = whole * frame[0] + heard * (parts[0] + parts[2]);
                    frame[1] = whole * frame[1] + heard * (parts[1] + parts[3]);
                }
            }
        }
        self.switching = self.switching.saturating_sub(out.len() / 2);
    }
}

/// Reads interleaved frames from `start`, two channels (the whole track) or four (the rest,
/// then the parts apart); frames past the end of an active loop come from its start, as the
/// playhead will play them. The stem gains move with the frames read, except for a pre-roll
/// (already heard, or never).
fn read(
    source: &mut Reader,
    start: isize,
    loop_frames: Option<(f64, f64)>,
    out: &mut [f32],
    channels: usize,
    advance: bool,
) {
    for (i, frame) in out.chunks_exact_mut(channels).enumerate() {
        let mut index = start + i as isize;
        if let Some((loop_start, loop_end)) = loop_frames {
            let (s, e) = (loop_start as isize, loop_end as isize);
            if e > s && index >= e {
                index = s + (index - s) % (e - s);
            }
        }
        if channels == APART {
            frame.copy_from_slice(&source.frame_apart(index));
        } else {
            (frame[0], frame[1]) = source.frame(index);
        }
        if advance {
            source.advance();
        }
    }
}

/// The rest and the parts apart added up into the whole track.
fn add_up(apart: &[f32], whole: &mut [f32]) {
    for (frame, parts) in whole.chunks_exact_mut(WHOLE).zip(apart.chunks_exact(APART)) {
        frame[0] = parts[0] + parts[2];
        frame[1] = parts[1] + parts[3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stems::{Parts, Stem, Stems};
    use prisme_media::TrackAudio;
    use std::f32::consts::TAU;

    const RATE: u32 = 48_000;

    fn sine(freq: f32, seconds: f32) -> TrackAudio {
        let frames = (seconds * RATE as f32) as usize;
        let samples = (0..frames).flat_map(|i| [(TAU * freq * i as f32 / RATE as f32).sin(); 2]).collect();
        TrackAudio::from_interleaved(RATE, samples)
    }

    /// Plays `seconds` at `speed`; returns the left channel and the final position.
    fn play(lock: &mut KeyLock, track: &TrackAudio, speed: f64, seconds: f64) -> (Vec<f32>, f64) {
        let mut position = RATE as f64;
        let mut heard = Vec::new();
        let mut block = vec![0.0f32; 256 * 2];
        for _ in 0..(seconds * RATE as f64 / 256.0) as usize {
            position = lock.render(&mut Stems::new().reader(track), position, None, &mut block, None, speed);
            heard.extend(block.chunks_exact(2).map(|f| f[0]));
        }
        (heard, position)
    }

    /// Frequency from the rising zero crossings of the second half.
    fn frequency(signal: &[f32]) -> f64 {
        let half = &signal[signal.len() / 2..];
        let crossings = half.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        crossings as f64 / (half.len() as f64 / RATE as f64)
    }

    #[test]
    fn a_high_resolution_track_plays_in_long_blocks() {
        // 192 kHz played at 44.1 kHz: more than four track frames per output frame
        let frames = 192_000 * 2;
        let samples = (0..frames).flat_map(|i| [(TAU * 440.0 * i as f32 / 192_000.0).sin(); 2]).collect();
        let track = TrackAudio::from_interleaved(192_000, samples);
        let mut lock = KeyLock::new(44_100, 4096);
        let mut block = vec![0.0f32; 4096 * 2];
        let mut position = 192_000.0 * 0.2;
        for _ in 0..4 {
            position =
                lock.render(&mut Stems::new().reader(&track), position, None, &mut block, None, MAX_SPEED);
        }
        assert!(block.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn faster_but_same_pitch() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        let (heard, position) = play(&mut lock, &track, 1.08, 2.0);
        let f = frequency(&heard);
        assert!((f - 440.0).abs() < 3.0, "{f} Hz");
        let travelled = position - RATE as f64;
        assert!((travelled / (2.0 * RATE as f64) - 1.08).abs() < 0.01, "{travelled}");
    }

    #[test]
    fn a_jump_restarts_at_the_new_place() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        play(&mut lock, &track, 1.0, 0.5);
        let mut block = vec![0.0f32; 512];
        let after =
            lock.render(&mut Stems::new().reader(&track), 4.0 * RATE as f64, None, &mut block, None, 1.0);
        assert!((after - (4.0 * RATE as f64 + 256.0)).abs() < 1e-6);
        assert!(
            (lock.feed
                - after
                - lock.whole.stretch.input_latency() as f64
                - lock.whole.stretch.output_latency() as f64)
                .abs()
                < 2.0
        );
    }

    #[test]
    fn the_feed_wraps_with_the_loop() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        let loop_frames = Some((RATE as f64, RATE as f64 * 1.25));
        let mut position = RATE as f64;
        let mut block = vec![0.0f32; 512];
        for _ in 0..2_000 {
            position =
                lock.render(&mut Stems::new().reader(&track), position, loop_frames, &mut block, None, 1.0);
            assert!((RATE as f64..RATE as f64 * 1.25).contains(&position));
        }
        let lead = lock.whole.stretch.input_latency() as f64 + lock.whole.stretch.output_latency() as f64;
        assert!((lock.feed - (position + lead)).abs() < 2.0, "no restart at each wrap");
    }

    #[test]
    fn a_key_shift_transposes_whatever_the_speed() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        lock.set_key_shift(12);
        let f = frequency(&play(&mut lock, &track, 1.0, 2.0).0);
        assert!((f - 880.0).abs() < 6.0, "{f} Hz");
        lock.set_key_shift(-2);
        let f = frequency(&play(&mut lock, &track, 1.08, 2.0).0);
        assert!((f - 392.0).abs() < 3.0, "{f} Hz");
    }

    /// A sine whose vocals are half of it, read with the vocals apart from the rest.
    fn sung() -> (TrackAudio, Stems) {
        let track = sine(440.0, 6.0);
        let half = track.view();
        let samples = (0..6 * RATE as isize).flat_map(|i| [0.5 * half.frame(i).0; 2]).collect();
        let mut stems = Stems::new();
        stems.set_tracks([
            Some(std::sync::Arc::new(TrackAudio::from_interleaved(RATE, samples))),
            None,
            None,
        ]);
        stems.set_apart(Parts::of(Stem::Vocals));
        (track, stems)
    }

    /// Plays blocks of 256 frames from `position`, the parts apart or not; returns the rest
    /// and the parts apart (left channel), and the final position.
    fn play_apart(
        lock: &mut KeyLock,
        track: &TrackAudio,
        stems: &mut Stems,
        position: f64,
        blocks: usize,
        apart: bool,
    ) -> (Vec<f32>, Vec<f32>, f64) {
        let (mut rest, mut parts, mut position) = (Vec::new(), Vec::new(), position);
        let mut block = vec![0.0f32; 256 * 2];
        let mut part = vec![0.0f32; 256 * 2];
        for _ in 0..blocks {
            let room = apart.then_some(&mut part[..]);
            position = lock.render(&mut stems.reader(track), position, None, &mut block, room, 1.06);
            rest.extend(block.chunks_exact(2).map(|f| f[0]));
            parts.extend(part.chunks_exact(2).map(|f| if apart { f[0] } else { 0.0 }));
        }
        (rest, parts, position)
    }

    #[test]
    fn parts_apart_are_stretched_as_the_rest() {
        let (track, mut stems) = sung();
        let mut lock = KeyLock::new(RATE, 1024);
        let (rest, parts, _) = play_apart(&mut lock, &track, &mut stems, RATE as f64, 400, true);
        let settled = rest.len() / 2..;
        let worst =
            rest[settled.clone()].iter().zip(&parts[settled]).map(|(r, p)| (r - p).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-3, "the rest and the vocals, both half the sine, differ by {worst}");
        assert!((frequency(&parts) - 440.0).abs() < 3.0);
    }

    #[test]
    fn setting_parts_apart_and_back_crossfades() {
        let (track, mut stems) = sung();
        let mut lock = KeyLock::new(RATE, 1024);
        let (mut heard, _, mut position) = play_apart(&mut lock, &track, &mut stems, RATE as f64, 200, false);
        for apart in [true, false] {
            let (rest, parts, after) = play_apart(&mut lock, &track, &mut stems, position, 200, apart);
            heard.extend(rest.iter().zip(&parts).map(|(r, p)| r + p));
            position = after;
        }
        // the sine's level, block by block, stays through both switches
        let levels: Vec<f32> = heard
            .chunks_exact(256)
            .skip(20)
            .map(|b| b.iter().fold(0.0, |m, s| f32::max(m, s.abs())))
            .collect();
        let (low, high) = levels.iter().fold((f32::MAX, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(low > 0.8 && high < 1.2, "level from {low} to {high}");
    }

    #[test]
    fn speeds_it_applies_to() {
        assert!(KeyLock::applies(1.0) && KeyLock::applies(0.92) && KeyLock::applies(1.5));
        assert!(!KeyLock::applies(-1.0) && !KeyLock::applies(0.1) && !KeyLock::applies(3.0));
    }
}
