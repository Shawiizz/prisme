//! Key lock: the tempo changes, the pitch does not. The track is read at the deck's
//! speed and goes through a time-stretcher (Signalsmith Stretch) that plays it back at
//! the original pitch, or transposed by whole semitones (key shift).
//!
//! The stretcher delays what it is given; it is fed ahead of the playhead by that delay
//! so that what is heard is where the playhead is. After a jump it starts again from the
//! new place, behind the feed by its delay, and catches up a little more each block: it is
//! silent for a few milliseconds instead of the whole delay, and no block does all the work
//! of the delay at once. In a loop the feed wraps with the playhead. Backwards, it plays the
//! track as read from the end, mirrored. Out of [`KeyLock::applies`] (scratching, very slow
//! or fast) the deck plays without it.
//!
//! Parts of the track read apart from the rest (for an effect on them alone) go through a
//! stretcher of four channels, the rest and the parts, which stretches both alike so that
//! they still add up. Going from one stretcher to the other crossfades, once the one coming
//! in has caught up.

use crate::stems::Reader;
use signalsmith_stretch::Stretch;

const MIN_SPEED: f64 = 0.5;
const MAX_SPEED: f64 = 2.0;
/// A feed further than this from where it should be means the playhead jumped.
const JUMP_FRAMES: f64 = 64.0;
/// The highest sample rate of a track the input is sized for (studio masters reach 384 kHz):
/// a block reads that many more track frames than it plays at the output rate.
const MAX_TRACK_RATE: f64 = 384_000.0;
/// A stretcher catching up runs through at most this many times a block's frames of its
/// delay a block: silent about 7 ms after a jump, each block's extra work small enough for
/// the slower processor of a phone.
const CATCH_UP_PER_FRAME: usize = 12;
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
    /// Output frames of its delay it has still to run through, silent.
    behind: usize,
    /// How far its input is behind the feed meanwhile (track frames).
    lag: f64,
    /// Track sample rate and key shift the transposition was set for.
    transposed_for: Option<(u32, i8)>,
}

impl Stretcher {
    fn new(channels: usize, out_rate: u32) -> Self {
        Self {
            stretch: Stretch::preset_cheaper(channels as u32, out_rate),
            channels,
            engaged: false,
            behind: 0,
            lag: 0.0,
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

    /// Restarts with `preroll`, what comes before its input, its delay to run through while
    /// `step` track frames are read an output frame.
    fn start(&mut self, preroll: &[f32], speed: f64, step: f64) {
        self.stretch.reset();
        self.stretch.seek(preroll, speed);
        self.behind = self.stretch.output_latency();
        self.lag = self.behind as f64 * step;
        self.engaged = true;
    }

    /// The output frames of its delay it runs through in a block of `frames`.
    fn catching_up(&self, frames: usize) -> usize {
        self.behind.min(frames * CATCH_UP_PER_FRAME)
    }
}

pub(crate) struct KeyLock {
    whole: Stretcher,
    apart: Stretcher,
    /// Parts are read apart from the rest now.
    setting_apart: bool,
    /// Output frames left of the crossfade from the other stretcher.
    switching: usize,
    /// Interleaved input, four channels wide, sized for the longest block at the highest
    /// speed and track rate, a stretcher catching up.
    input: Vec<f32>,
    /// The rest and the parts added up, for the whole track's stretcher while both run.
    added: Vec<f32>,
    /// What a stretcher gives, its delay run through included, four channels wide.
    given: Vec<f32>,
    /// What the stretcher of the parts apart gives for the block, four channels wide.
    stretched: Vec<f32>,
    /// Source frame (fractional) of the next input, mirrored when backwards.
    feed: f64,
    /// The track is read backwards: its frames mirrored around its start.
    backwards: bool,
    key_shift: i8,
    out_rate: f64,
}

impl KeyLock {
    pub(crate) fn new(out_rate: u32, max_block_frames: usize) -> Self {
        let whole = Stretcher::new(WHOLE, out_rate);
        let delay = whole.stretch.output_latency();
        let given = max_block_frames + delay.min(max_block_frames * CATCH_UP_PER_FRAME);
        // a block reads from where a stretcher catching up is to where the feed goes
        let rates = (MAX_TRACK_RATE / f64::from(out_rate)).max(1.0);
        let longest = ((max_block_frames + delay) as f64 * MAX_SPEED * rates).ceil() as usize + 2;
        let frames = longest.max(whole.stretch.input_latency());
        Self {
            whole,
            apart: Stretcher::new(APART, out_rate),
            setting_apart: false,
            switching: 0,
            input: vec![0.0; frames * APART],
            added: vec![0.0; frames * WHOLE],
            given: vec![0.0; given * APART],
            stretched: vec![0.0; max_block_frames * APART],
            feed: 0.0,
            backwards: false,
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

    /// Renders `out` from `source` at `speed`, backwards if negative, the playhead being at
    /// `position` (source frames) in the loop `loop_frames`; returns where the playhead is
    /// afterwards. With `apart`, the parts the source sets apart go there and the rest to
    /// `out`.
    pub(crate) fn render(
        &mut self,
        source: &mut Reader,
        position: f64,
        loop_frames: Option<(f64, f64)>,
        out: &mut [f32],
        apart: Option<&mut [f32]>,
        speed: f64,
    ) -> f64 {
        let backwards = speed < 0.0;
        if backwards != self.backwards {
            self.disengage();
            self.backwards = backwards;
        }
        if !backwards {
            return self.render_forwards(source, position, loop_frames, out, apart, speed);
        }
        let mirrored = loop_frames.map(|(start, end)| (-end, -start));
        -self.render_forwards(source, -position, mirrored, out, apart, -speed)
    }

    /// [`KeyLock::render`] at a positive `speed`, through the track mirrored if backwards.
    fn render_forwards(
        &mut self,
        source: &mut Reader,
        position: f64,
        loop_frames: Option<(f64, f64)>,
        out: &mut [f32],
        apart: Option<&mut [f32]>,
        speed: f64,
    ) -> f64 {
        let ratio = source.sample_rate() as f64 / self.out_rate;
        let step = speed * ratio;
        for stretcher in [&mut self.whole, &mut self.apart] {
            stretcher.transpose(source.sample_rate(), self.key_shift, ratio);
        }
        let stretch = &self.whole.stretch;
        let wanted = position + stretch.input_latency() as f64 + stretch.output_latency() as f64 * step;
        let setting_apart = apart.is_some();
        if (self.feed - wanted).abs() > JUMP_FRAMES || !(self.whole.engaged || self.apart.engaged) {
            self.disengage();
            self.feed = wanted.floor();
            self.setting_apart = setting_apart;
        }
        if setting_apart != self.setting_apart {
            // the other stretcher still plays while this one comes in
            self.setting_apart = setting_apart;
            self.switching = SWITCH_FRAMES;
        }
        if !self.stretchers().0.engaged {
            self.start(source, loop_frames, speed, step);
        }
        if self.switching == 0 {
            self.stretchers().1.engaged = false;
        }

        let frames = out.len() / 2;
        let advance = frames as f64 * step;
        let lag = self.whole.lag.max(self.apart.lag);
        let start = (self.feed - lag).floor() as isize;
        // a track above MAX_TRACK_RATE is read short rather than overflowing the input
        let read_frames =
            (((self.feed + advance).floor() as isize - start) as usize).min(self.input.len() / APART);
        let channels = if self.apart.engaged { APART } else { WHOLE };
        let input = &mut self.input[..read_frames * channels];
        let span = Span { start, loop_frames, backwards: self.backwards };
        read(source, span, input, channels, self.feed.floor() as isize);
        let at = Input { start, feed: self.feed, advance, step };
        if self.apart.engaged {
            stretch_block(&mut self.apart, input, at, &mut self.given, &mut self.stretched[..frames * APART]);
        }
        if self.whole.engaged {
            let whole_input = if channels == APART {
                add_up(input, &mut self.added[..read_frames * WHOLE]);
                &self.added[..read_frames * WHOLE]
            } else {
                &*input
            };
            stretch_block(&mut self.whole, whole_input, at, &mut self.given, out);
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

    /// Restarts the stretcher of the way the track is read now, behind the feed by its delay,
    /// with what comes before.
    fn start(&mut self, source: &mut Reader, loop_frames: Option<(f64, f64)>, speed: f64, step: f64) {
        let Self { whole, apart, input, setting_apart, feed, backwards, .. } = self;
        let now = if *setting_apart { apart } else { whole };
        let channels = now.channels;
        let preroll = now.stretch.input_latency();
        let behind = now.stretch.output_latency() as f64 * step;
        let preroll_input = &mut input[..preroll * channels];
        let from = (*feed - behind).floor() as isize - preroll as isize;
        let span = Span { start: from, loop_frames, backwards: *backwards };
        read(source, span, preroll_input, channels, isize::MAX);
        now.start(preroll_input, speed, step);
    }

    /// What the stretchers gave into `out` and `apart`: the rest and the parts apart, or the
    /// whole track, crossfading from one to the other while switching.
    fn mix(&mut self, out: &mut [f32], mut apart: Option<&mut [f32]>) {
        if !self.apart.engaged {
            if let Some(apart) = apart {
                apart.fill(0.0);
            }
            return;
        }
        // the crossfade waits for the stretcher coming in to catch up
        let ready = self.stretchers().0.behind == 0;
        let stretched = &self.stretched[..out.len() * 2];
        for (i, (frame, parts)) in out.chunks_exact_mut(2).zip(stretched.chunks_exact(APART)).enumerate() {
            // how much the parts apart are heard: 1 once switched to them, 0 once switched away
            let left =
                if ready { self.switching.saturating_sub(i).min(SWITCH_FRAMES) } else { SWITCH_FRAMES };
            let left = left as f32 / SWITCH_FRAMES as f32;
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
        if ready {
            self.switching = self.switching.saturating_sub(out.len() / 2);
        }
    }
}

/// The input read for a block: from track frame `start`, up to the feed moved by `advance`,
/// `step` track frames an output frame.
#[derive(Clone, Copy)]
struct Input {
    start: isize,
    feed: f64,
    advance: f64,
    step: f64,
}

/// Runs `stretcher` over its part of `input` for a block of `out`, catching up a little of
/// its delay if it is behind: what it gives goes to `given`, the block's end to `out`.
fn stretch_block(stretcher: &mut Stretcher, input: &[f32], at: Input, given: &mut [f32], out: &mut [f32]) {
    let channels = stretcher.channels;
    let frames = out.len() / channels;
    let caught_up = stretcher.catching_up(frames);
    stretcher.behind -= caught_up;
    let lag_before = stretcher.lag;
    stretcher.lag = if stretcher.behind == 0 { 0.0 } else { lag_before - caught_up as f64 * at.step };
    let input_frames = input.len() / channels;
    let first = (((at.feed - lag_before).floor() as isize - at.start).max(0) as usize).min(input_frames);
    let end = (at.feed + at.advance - stretcher.lag).floor() as isize - at.start;
    let end = (end.max(first as isize) as usize).min(input_frames);
    let given = &mut given[..(frames + caught_up) * channels];
    stretcher.stretch.process(&input[first * channels..end * channels], &mut *given);
    out.copy_from_slice(&given[caught_up * channels..]);
}

/// Where frames are read from: the first, in the active loop, through the track mirrored or
/// not.
#[derive(Clone, Copy)]
struct Span {
    start: isize,
    loop_frames: Option<(f64, f64)>,
    backwards: bool,
}

/// Reads interleaved frames from the start of `span`, two channels (the whole track) or four
/// (the rest, then the parts apart); frames past the end of an active loop come from its
/// start, as the playhead will play them. The stem gains move with the frames read from
/// `heard_from` (those before were heard already, or will never be).
fn read(source: &mut Reader, span: Span, out: &mut [f32], channels: usize, heard_from: isize) {
    for (i, frame) in out.chunks_exact_mut(channels).enumerate() {
        let at = span.start + i as isize;
        let mut index = at;
        if let Some((loop_start, loop_end)) = span.loop_frames {
            let (s, e) = (loop_start.floor() as isize, loop_end.floor() as isize);
            if e > s && index >= e {
                index = s + (index - s) % (e - s);
            }
        }
        if span.backwards {
            index = -index;
        }
        if channels == APART {
            frame.copy_from_slice(&source.frame_apart(index));
        } else {
            (frame[0], frame[1]) = source.frame(index);
        }
        if at >= heard_from {
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
    fn a_jump_in_large_blocks_sounds_at_once() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        play(&mut lock, &track, 1.04, 0.5);
        let mut block = vec![0.0f32; 512 * 2];
        let mut position = 4.0 * RATE as f64;
        for _ in 0..8 {
            position = lock.render(&mut Stems::new().reader(&track), position, None, &mut block, None, 1.04);
            let level = block.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(level > 0.8, "{level}");
        }
    }

    #[test]
    fn a_jump_in_small_blocks_is_silent_some_milliseconds() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        play(&mut lock, &track, 1.04, 0.5);
        let mut block = vec![0.0f32; 64 * 2];
        let mut position = 4.0 * RATE as f64;
        let levels: Vec<f32> = (0..40)
            .map(|_| {
                position =
                    lock.render(&mut Stems::new().reader(&track), position, None, &mut block, None, 1.04);
                block.iter().fold(0.0f32, |m, s| m.max(s.abs()))
            })
            .collect();
        let sounding = levels.iter().position(|&l| l > 0.8).expect("it sounds");
        assert!(sounding <= 6, "{sounding} blocks of 64 frames silent");
        assert!(levels[sounding..].iter().all(|&l| l > 0.8), "{levels:?}");
        assert!((frequency(&play(&mut lock, &track, 1.04, 0.5).0) - 440.0).abs() < 3.0);
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
    fn a_loop_shorter_than_a_block_keeps_the_playhead() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        let loop_frames = Some((RATE as f64, RATE as f64 + 100.0));
        let mut position = RATE as f64;
        let mut block = vec![0.0f32; 512];
        for _ in 0..100 {
            position =
                lock.render(&mut Stems::new().reader(&track), position, loop_frames, &mut block, None, 1.0);
            assert!((RATE as f64..RATE as f64 + 100.0).contains(&position), "{position}");
        }
    }

    #[test]
    fn backwards_at_the_same_pitch_through_the_track_read_from_its_end() {
        // 440 Hz up to 4.3 s, 660 Hz after: played backwards from 5 s, the end is in the first
        let split = (4.3 * RATE as f32) as usize;
        let samples = (0..6 * RATE as usize)
            .flat_map(|i| {
                let freq = if i < split { 440.0 } else { 660.0 };
                [(TAU * freq * i as f32 / RATE as f32).sin(); 2]
            })
            .collect();
        let track = TrackAudio::from_interleaved(RATE, samples);
        let mut lock = KeyLock::new(RATE, 1024);
        let mut position = 5.0 * RATE as f64;
        let mut heard = Vec::new();
        let mut block = vec![0.0f32; 256 * 2];
        for _ in 0..2 * RATE as usize / 256 {
            position = lock.render(&mut Stems::new().reader(&track), position, None, &mut block, None, -1.08);
            heard.extend(block.chunks_exact(2).map(|f| f[0]));
        }
        let f = frequency(&heard);
        assert!((f - 440.0).abs() < 3.0, "{f} Hz");
        let travelled = position - 5.0 * RATE as f64;
        assert!((travelled / (2.0 * RATE as f64) + 1.08).abs() < 0.01, "{travelled}");
    }

    #[test]
    fn backwards_the_feed_wraps_with_the_loop() {
        let track = sine(440.0, 6.0);
        let mut lock = KeyLock::new(RATE, 1024);
        let loop_frames = Some((RATE as f64, RATE as f64 * 1.25));
        let mut position = RATE as f64 * 1.2;
        let mut block = vec![0.0f32; 512];
        for _ in 0..2_000 {
            position =
                lock.render(&mut Stems::new().reader(&track), position, loop_frames, &mut block, None, -1.0);
            assert!((RATE as f64..=RATE as f64 * 1.25).contains(&position), "{position}");
        }
        let lead = lock.whole.stretch.input_latency() as f64 + lock.whole.stretch.output_latency() as f64;
        assert!((lock.feed - (-position + lead)).abs() < 2.0, "no restart at each wrap");
        let level = block.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(level > 0.8, "{level}");
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
