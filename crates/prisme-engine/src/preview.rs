//! The preview: a track of the library heard in the headphones alone, before it goes on a
//! deck. It never reaches the master, so never a recording either. Where its audio is not
//! decoded yet (a start a third into a track being read) it waits for it.

use crate::status::PreviewStatus;
use prisme_media::TrackAudio;
use std::sync::Arc;

pub(crate) struct Preview {
    track: Option<Arc<TrackAudio>>,
    id: u64,
    /// Source frame being played.
    position: f64,
    /// The last block waited for its audio.
    waiting: bool,
    /// The block as played, interleaved stereo.
    audio: Vec<f32>,
}

impl Preview {
    pub(crate) fn new(max_block_frames: usize) -> Self {
        Self { track: None, id: 0, position: 0.0, waiting: false, audio: vec![0.0; max_block_frames * 2] }
    }

    /// Plays `track` from `start_s`; returns the track it replaces, to be freed elsewhere.
    pub(crate) fn start(&mut self, id: u64, track: Arc<TrackAudio>, start_s: f64) -> Option<Arc<TrackAudio>> {
        let old = self.track.replace(track);
        self.id = id;
        self.waiting = false;
        self.seek(start_s);
        old
    }

    /// Stops it; returns its track, to be freed elsewhere.
    pub(crate) fn stop(&mut self) -> Option<Arc<TrackAudio>> {
        self.track.take()
    }

    /// Moves it to `seconds`, within the track as far as it is known.
    pub(crate) fn seek(&mut self, seconds: f64) {
        let Some(track) = self.track.as_ref() else { return };
        let last = track.frames().saturating_sub(1) as f64;
        self.position = (seconds * track.sample_rate() as f64).clamp(0.0, last);
    }

    /// Moves it by `seconds`, forwards or back.
    pub(crate) fn skip(&mut self, seconds: f64) {
        if let Some(track) = self.track.as_ref() {
            let at = self.position / track.sample_rate() as f64;
            self.seek(at + seconds);
        }
    }

    /// Plays the next `frames` at `out_rate`: the block, silent while its audio is not
    /// decoded yet, and the track once it has played to its end (to be freed elsewhere).
    pub(crate) fn render(&mut self, frames: usize, out_rate: f64) -> (&[f32], Option<Arc<TrackAudio>>) {
        let out = &mut self.audio[..frames * 2];
        out.fill(0.0);
        let Some(track) = self.track.as_ref() else { return (out, None) };
        let complete = track.is_complete();
        let length = track.frames() as f64;
        if complete && self.position >= length {
            return (out, self.track.take());
        }
        let step = track.sample_rate() as f64 / out_rate;
        let view = track.view();
        let first = self.position.floor() as isize;
        let last = (self.position + step * frames as f64).floor() as isize + 1;
        self.waiting = !complete && (view.get(first).is_none() || view.get(last).is_none());
        if self.waiting {
            return (out, None);
        }
        let mut position = self.position;
        for frame in out.chunks_exact_mut(2) {
            if position >= length {
                break;
            }
            // linear interpolation, as the sampler plays
            let i = position.floor();
            let t = (position - i) as f32;
            let (a, b) = (view.frame(i as isize), view.frame(i as isize + 1));
            frame[0] = a.0 + (b.0 - a.0) * t;
            frame[1] = a.1 + (b.1 - a.1) * t;
            position += step;
        }
        self.position = position;
        (out, None)
    }

    pub(crate) fn status(&self) -> Option<PreviewStatus> {
        let track = self.track.as_ref()?;
        let rate = track.sample_rate() as f64;
        Some(PreviewStatus {
            id: self.id,
            position_s: self.position / rate,
            duration_s: track.duration_s(),
            waiting: self.waiting,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_media::PENDING_BLOCK;

    fn ramp(frames: usize) -> Arc<TrackAudio> {
        Arc::new(TrackAudio::from_interleaved(
            1_000,
            (0..frames).flat_map(|i| [i as f32, i as f32]).collect(),
        ))
    }

    #[test]
    fn it_plays_from_where_it_starts_and_seeks() {
        let mut p = Preview::new(8);
        assert!(p.start(1, ramp(10_000), 2.0).is_none());
        let (out, _) = p.render(4, 1_000.0);
        assert_eq!(out, [2000.0, 2000.0, 2001.0, 2001.0, 2002.0, 2002.0, 2003.0, 2003.0]);
        p.seek(5.0);
        assert_eq!(p.render(1, 1_000.0).0, [5000.0, 5000.0]);
        p.skip(-1.0);
        assert_eq!(p.render(1, 1_000.0).0, [4001.0, 4001.0]);
        p.seek(99.0);
        assert_eq!(p.status().unwrap().position_s, 9.999, "within the track");
    }

    #[test]
    fn it_ends_with_its_track() {
        let mut p = Preview::new(8);
        p.start(3, ramp(10), 0.0);
        assert_eq!(p.status().map(|s| s.id), Some(3));
        let (out, ended) = p.render(8, 1_000.0);
        assert_eq!(out[14], 7.0);
        assert!(ended.is_none());
        let (out, ended) = p.render(8, 1_000.0);
        assert_eq!(&out[..4], [8.0, 8.0, 9.0, 9.0]);
        assert_eq!(&out[4..], [0.0; 12], "then silence");
        assert!(ended.is_none());
        assert!(p.render(8, 1_000.0).1.is_some(), "handed back");
        assert!(p.status().is_none());
    }

    #[test]
    fn it_waits_for_its_audio_to_be_decoded() {
        let track = Arc::new(TrackAudio::pending(1_000, 4 * PENDING_BLOCK));
        let mut p = Preview::new(8);
        p.start(1, track.clone(), 2.5);
        let (out, _) = p.render(8, 1_000.0);
        assert_eq!(out, [0.0; 16]);
        assert!(p.status().unwrap().waiting);
        assert_eq!(p.status().unwrap().position_s, 2.5, "it holds where it starts");
        track.publish(2 * PENDING_BLOCK, &vec![0.5; 2 * PENDING_BLOCK]);
        assert_eq!(p.render(2, 1_000.0).0, [0.5; 4]);
        assert!(!p.status().unwrap().waiting);
    }

    #[test]
    fn another_track_takes_its_place_and_the_old_one_is_handed_back() {
        let mut p = Preview::new(8);
        let first = ramp(10);
        p.start(1, first.clone(), 0.0);
        let back = p.start(2, ramp(10), 0.0).unwrap();
        assert!(Arc::ptr_eq(&back, &first));
        assert_eq!(p.status().unwrap().id, 2);
        assert!(p.stop().is_some());
        assert_eq!(p.render(2, 1_000.0).0, [0.0; 4]);
    }
}
