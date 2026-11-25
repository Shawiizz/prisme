//! Where the beats and the bars' first beats are, as a small neural network hears them:
//! "Beat This!" (Foscarin, Schlüter and Widmer, ISMIR 2024), its small model run by rten on a
//! few excerpts of the track (MIT, `models/LICENSE-beat-this`). It settles what the onsets
//! alone cannot: which of two pulses half a beat apart is the beat (a bass line on the
//! off-beats outweighs the kicks), and which beat starts the bars. The tempo, and each beat's
//! instant to the sample, stay the signal's ([`crate::tempo`], [`crate::grid`]).

use beat_this::{BeatThis, Model, Tensor};
use rten_tensor::Layout;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

static MEL_MODEL: &[u8] = include_bytes!("../models/mel_spectrogram.onnx");
static BEAT_MODEL: &[u8] = include_bytes!("../models/beat_this_small.onnx");
/// The network's frames a second.
const FRAME_RATE: f64 = 50.0;
/// Seconds of each excerpt heard: half the length the network was trained on, where it hears
/// the beats and the bars of real tracks as well, six times sooner (its work grows much faster
/// than the length heard).
pub const EXCERPT_S: f64 = 15.0;
/// Excerpts taken, spread over the track: every other one heard first, the others where the
/// bars are not clear in them.
const EXCERPTS: usize = 6;
/// Where the beats are looked for on a grid, in steps of this (seconds).
const PHASE_STEP_S: f64 = 0.002;
/// A grid on which the network hears beats less than this, on average, holds none it hears.
const HEARD: f32 = 0.2;
/// The bars start on the beat whose downbeats the network hears this much more than any other's,
/// surely: less, the network mistakes the third beat of a bar for its first (a clap on the
/// second and the fourth).
const BAR_MARGIN: f32 = 2.0;

/// A stretch of a track, mono.
pub struct Excerpt {
    pub start_s: f64,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// Where the excerpts are taken in a track of `duration_s` seconds, if known: evenly through
/// the track, past its intro and before its outro; one excerpt in a short track.
pub fn excerpt_starts(duration_s: Option<f64>) -> Vec<f64> {
    match duration_s {
        Some(d) if d <= 3.0 * EXCERPT_S => vec![0.0],
        Some(d) => (1..=EXCERPTS).map(|i| d * i as f64 / (EXCERPTS + 1) as f64 - EXCERPT_S / 2.0).collect(),
        // a stream that does not tell its length: its first minutes, past a short intro
        None => (0..EXCERPTS).map(|i| EXCERPT_S * (1.0 + i as f64)).collect(),
    }
}

/// Takes the excerpts of a track decoded chunk by chunk.
pub struct ExcerptTaker {
    sample_rate: u32,
    excerpts: Vec<Excerpt>,
    /// Frames pushed so far.
    frame: usize,
}

impl ExcerptTaker {
    pub fn new(sample_rate: u32, duration_s: Option<f64>) -> Self {
        let excerpts = excerpt_starts(duration_s)
            .into_iter()
            .map(|start_s| Excerpt { start_s, sample_rate, samples: Vec::new() })
            .collect();
        Self { sample_rate, excerpts, frame: 0 }
    }

    /// Adds interleaved stereo samples.
    pub fn push(&mut self, stereo: &[f32]) {
        let rate = f64::from(self.sample_rate);
        let frames = stereo.len() / 2;
        for excerpt in &mut self.excerpts {
            let start = (excerpt.start_s * rate) as usize;
            let end = start + (EXCERPT_S * rate) as usize;
            let (from, to) = (start.max(self.frame), end.min(self.frame + frames));
            if from < to {
                let taken = &stereo[2 * (from - self.frame)..2 * (to - self.frame)];
                excerpt.samples.extend(taken.chunks_exact(2).map(|f| (f[0] + f[1]) * 0.5));
            }
        }
        self.frame += frames;
    }

    pub fn finish(self) -> Vec<Excerpt> {
        let rate = f64::from(self.sample_rate);
        // at least ten seconds heard, else nothing to go by
        self.excerpts.into_iter().filter(|e| e.samples.len() as f64 >= 10.0 * rate).collect()
    }
}

/// How likely each frame of an excerpt holds a beat, and a bar's first beat (0 to 1).
pub struct Heard {
    start_s: f64,
    beat: Vec<f32>,
    downbeat: Vec<f32>,
}

impl Heard {
    fn end_s(&self) -> f64 {
        self.start_s + self.beat.len() as f64 / FRAME_RATE
    }

    fn at(&self, values: &[f32], time_s: f64) -> f32 {
        let position = (time_s - self.start_s) * FRAME_RATE;
        let i = position.floor();
        if i < 0.0 || i as usize + 1 >= values.len() {
            return 0.0;
        }
        let (i, t) = (i as usize, (position - i) as f32);
        values[i] * (1.0 - t) + values[i + 1] * t
    }

    /// The instants of a grid of `period_s` through `phase_s` within the excerpt.
    fn beats(&self, period_s: f64, phase_s: f64) -> impl Iterator<Item = (i64, f64)> + '_ {
        let first = ((self.start_s - phase_s) / period_s).ceil() as i64;
        (first..).map(move |n| (n, phase_s + n as f64 * period_s)).take_while(move |&(_, t)| t < self.end_s())
    }
}

thread_local! {
    /// The threads each analysing thread's network runs on, and how many [`hear`] asks for: one
    /// for the tracks of a library, analysed side by side, which uses the cores better; a few
    /// for a track the DJ waits for.
    static THREADS: RefCell<Option<(usize, Arc<rten::ThreadPool>)>> = const { RefCell::new(None) };
    static THREADS_ASKED: Cell<usize> = const { Cell::new(1) };
}

/// The analysing thread's own, at the priority the analysis runs at now: the background while a
/// deck plays, that of ordinary work otherwise (see [`crate::set_full_speed`]).
fn threads() -> Arc<rten::ThreadPool> {
    let asked = THREADS_ASKED.get();
    let pool = THREADS.with(|kept| {
        let mut kept = kept.borrow_mut();
        match kept.as_ref() {
            Some((count, pool)) if *count == asked => pool.clone(),
            _ => {
                let pool = Arc::new(rten::ThreadPool::with_num_threads(asked));
                *kept = Some((asked, pool.clone()));
                pool
            }
        }
    });
    pool.run(|| rayon::broadcast(|_| crate::analysis_priority()));
    pool
}

thread_local! {
    /// Each analysing thread's network, made once: it keeps state between its runs.
    static NETWORK: RefCell<Option<BeatThis<Graph>>> = const { RefCell::new(None) };
}

/// What the network hears of `excerpts`, run on `threads` threads: nothing if it cannot run.
pub fn hear(excerpts: &[&Excerpt], threads: usize) -> Vec<Heard> {
    THREADS_ASKED.set(threads.max(1));
    NETWORK.with(|network| {
        let mut network = network.borrow_mut();
        if network.is_none() {
            *network = Graph::network().inspect_err(|e| tracing::error!("no beat model: {e}")).ok();
        }
        let Some(network) = network.as_mut() else { return Vec::new() };
        excerpts
            .iter()
            .filter_map(|excerpt| {
                let heard = network.analyze_audio(&excerpt.samples, excerpt.sample_rate).ok()?;
                let sigmoid =
                    |logits: Vec<f32>| logits.into_iter().map(|x| 1.0 / (1.0 + (-x).exp())).collect();
                Some(Heard {
                    start_s: excerpt.start_s,
                    beat: sigmoid(heard.beat_logits),
                    downbeat: sigmoid(heard.downbeat_logits),
                })
            })
            .collect()
    })
}

/// How surely the network hears beats on a grid at `bpm` through `phase_s`: the mean, over
/// its beats, of the likelihood it gives them (0 to 1).
pub fn beat_score(heard: &[Heard], bpm: f64, phase_s: f64) -> f32 {
    let period_s = 60.0 / bpm;
    let (sum, count) = heard
        .iter()
        .flat_map(|h| h.beats(period_s, phase_s).map(move |(_, t)| h.at(&h.beat, t)))
        .fold((0.0f32, 0usize), |(s, n), v| (s + v, n + 1));
    sum / count.max(1) as f32
}

/// Where the network hears the beats of a grid at `bpm`: its first beat (seconds, in
/// `[0, period)`), if it hears beats on it.
pub fn beat_phase(heard: &[Heard], bpm: f64) -> Option<f64> {
    let steps = (60.0 / bpm / PHASE_STEP_S).ceil() as usize;
    let (best, mean) = (0..steps)
        .map(|i| (i as f64 * PHASE_STEP_S, beat_score(heard, bpm, i as f64 * PHASE_STEP_S)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    (mean >= HEARD).then_some(best)
}

/// Which beat of a grid at `bpm` through `first_beat_s` starts the bars (0 to 3 beats after
/// it), if the network hears it clearly.
pub fn bar_start(heard: &[Heard], bpm: f64, first_beat_s: f64) -> Option<u32> {
    let period_s = 60.0 / bpm;
    let mut sums = [0.0f32; 4];
    let mut counts = [0usize; 4];
    for h in heard {
        for (n, t) in h.beats(period_s, first_beat_s) {
            let k = n.rem_euclid(4) as usize;
            sums[k] += h.at(&h.downbeat, t);
            counts[k] += 1;
        }
    }
    let mut means: Vec<(u32, f32)> = (0..4).map(|k| (k as u32, sums[k] / counts[k].max(1) as f32)).collect();
    means.sort_by(|a, b| b.1.total_cmp(&a.1));
    (means[0].1 >= HEARD && means[0].1 >= BAR_MARGIN * means[1].1).then_some(means[0].0)
}

/// An ONNX graph of the network, run by rten.
struct Graph {
    model: rten::Model,
    inputs: HashMap<String, rten::NodeId>,
    outputs: Vec<(rten::NodeId, String)>,
}

impl Graph {
    fn network() -> anyhow::Result<BeatThis<Graph>> {
        Ok(BeatThis::from_models(Self::load(MEL_MODEL)?, Self::load(BEAT_MODEL)?))
    }

    fn load(bytes: &'static [u8]) -> anyhow::Result<Self> {
        let model = rten::Model::load_static_slice(bytes)?;
        let name = |id: rten::NodeId| model.node_info(id).and_then(|i| i.name().map(str::to_owned));
        let inputs = model.input_ids().iter().filter_map(|&id| Some((name(id)?, id))).collect();
        let outputs = model.output_ids().iter().filter_map(|&id| Some((id, name(id)?))).collect();
        Ok(Self { model, inputs, outputs })
    }
}

impl Model for Graph {
    fn run(&mut self, inputs: &[(&str, &Tensor)]) -> anyhow::Result<HashMap<String, Tensor>> {
        let inputs = inputs
            .iter()
            .map(|(name, tensor)| {
                let id = *self.inputs.get(*name).ok_or_else(|| anyhow::anyhow!("no input {name}"))?;
                let view = rten::ValueView::from_shape(tensor.shape.as_slice(), tensor.data.as_slice())?;
                Ok((id, view.into()))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let ids: Vec<rten::NodeId> = self.outputs.iter().map(|(id, _)| *id).collect();
        let options = rten::RunOptions::default().with_thread_pool(Some(threads()));
        let values = self.model.run(inputs, &ids, Some(options))?;
        self.outputs
            .iter()
            .zip(values)
            .map(|((_, name), value)| {
                let tensor =
                    value.into_tensor::<f32>().ok_or_else(|| anyhow::anyhow!("{name} is not f32"))?;
                let shape = tensor.shape().to_vec();
                Ok((name.clone(), Tensor { shape, data: tensor.into_data() }))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a network would hear of a grid at `bpm` through `phase_s`, bars starting on its
    /// beats `bar` (mod 4), with a weaker pulse half a beat later.
    fn heard(bpm: f64, phase_s: f64, bar: i64) -> Heard {
        let period = 60.0 / bpm;
        let frames = (EXCERPT_S * FRAME_RATE) as usize;
        let (mut beat, mut downbeat) = (vec![0.0f32; frames], vec![0.0f32; frames]);
        for n in 0.. {
            let t = phase_s + n as f64 * period;
            let i = (t * FRAME_RATE).round() as usize;
            if i >= frames {
                break;
            }
            beat[i] = 0.9;
            if n % 4 == bar {
                downbeat[i] = 0.8;
            }
            if let Some(off) = beat.get_mut(((t + period / 2.0) * FRAME_RATE).round() as usize) {
                *off = 0.3;
            }
        }
        Heard { start_s: 0.0, beat, downbeat }
    }

    #[test]
    fn the_beats_and_the_bars_are_where_the_network_hears_them() {
        let h = [heard(124.0, 0.3, 2)];
        let phase = beat_phase(&h, 124.0).unwrap();
        assert!((phase - 0.3).abs() < 0.011, "{phase}");
        assert_eq!(bar_start(&h, 124.0, phase), Some(2));
        let silent = [Heard { start_s: 0.0, beat: vec![0.0; 1500], downbeat: vec![0.0; 1500] }];
        assert_eq!(beat_phase(&silent, 124.0), None);
        assert_eq!(bar_start(&silent, 124.0, 0.0), None);
    }

    #[test]
    fn the_excerpts_are_taken_where_they_are_asked() {
        assert_eq!(excerpt_starts(Some(2.0 * EXCERPT_S)), vec![0.0]);
        let starts = excerpt_starts(Some(420.0));
        assert_eq!(starts.len(), 6);
        assert_eq!((starts[0], starts[5]), (60.0 - EXCERPT_S / 2.0, 360.0 - EXCERPT_S / 2.0));
        let mut taker = ExcerptTaker::new(100, Some(420.0));
        let second: Vec<f32> = (0..200).map(|i| (i / 2) as f32).collect();
        for _ in 0..420 {
            taker.push(&second);
        }
        let excerpts = taker.finish();
        assert_eq!(excerpts.len(), 6);
        assert_eq!(excerpts[0].samples.len(), (EXCERPT_S * 100.0) as usize);
        let frame = (starts[1] * 100.0) as usize % 100;
        assert_eq!(excerpts[1].samples[0], frame as f32, "that frame of its second");
    }

    #[test]
    fn the_network_hears_a_track() {
        // house: a kick on each beat, a clap on two and four, an open hat between
        let rate = 22_050u32;
        let period = 60.0 / 125.0;
        let noise = |i: usize| ((i.wrapping_mul(2_654_435_761) >> 8) % 2000) as f64 / 1000.0 - 1.0;
        let samples: Vec<f32> = (0..(EXCERPT_S * rate as f64) as usize)
            .map(|i| {
                let t = i as f64 / rate as f64;
                let k = t.rem_euclid(period);
                let c = (t - period).rem_euclid(2.0 * period);
                let h = (t + period / 2.0).rem_euclid(period);
                let pitch = 50.0 + 120.0 * (-k * 40.0).exp();
                let kick = (-k * 12.0).exp() * (std::f64::consts::TAU * pitch * k).sin();
                let clap = (-c * 40.0).exp() * noise(i);
                let hat = (-h * 60.0).exp() * noise(i + 1);
                (0.9 * kick + 0.3 * clap + 0.12 * hat) as f32
            })
            .collect();
        let heard = hear(&[&Excerpt { start_s: 0.0, sample_rate: rate, samples }], 1);
        assert_eq!(heard.len(), 1);
        let phase = beat_phase(&heard, 125.0).expect("beats heard");
        let off = (phase / period).rem_euclid(1.0);
        assert!(off.min(1.0 - off) < 0.1, "on the kicks: {phase}");
    }
}
