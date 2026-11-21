//! What the settings offer to route the software mixer on the running device, and why what was
//! chosen is not heard there: the headphones' and the booth's output pairs, the inputs of the
//! microphone and of the aux line.

use crate::dto::{HeadphonesOutputDto, InputChannelsDto};
use serde::Serialize;

/// Why an output chosen is heard or not on the running device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum OutputNote {
    /// On these outputs (the first, 0-based).
    On {
        first: usize,
    },
    NotHeard,
    /// The headphones have the pair chosen.
    TakenByHeadphones {
        first: usize,
    },
    NotOnDevice,
}

/// Why a live source is not heard on the running device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LiveNote {
    /// Its inputs are not on the device, or the turntables have them.
    NotOnDevice,
    /// The turntables take every input: nothing is left to choose.
    TurntablesTakeAll,
}

/// A pair of outputs the booth can be on: heard there, or `taken` by the headphones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BoothChoice {
    pub first: usize,
    pub taken: bool,
}

/// The pairs after the master's 1-2 of a device with `outputs`.
fn pairs(outputs: usize) -> impl Iterator<Item = usize> {
    (2..).step_by(2).take_while(move |first| first + 2 <= outputs)
}

/// The pairs the headphones can take on a device with `outputs`, with the one `chosen`.
pub fn headphones(outputs: usize, chosen: HeadphonesOutputDto) -> Vec<usize> {
    let mut choices: Vec<usize> = pairs(outputs).collect();
    if let HeadphonesOutputDto::Pair { first } = chosen {
        choices.push(first);
    }
    choices.sort_unstable();
    choices.dedup();
    choices
}

/// The pairs the booth can take on a device with `outputs`: each after the master's but for
/// the `headphones`' (their first, on the software mixer), and the one `chosen` if it is not
/// among them, marked taken when the headphones have it.
pub fn booth(outputs: usize, headphones: Option<usize>, chosen: Option<usize>) -> Vec<BoothChoice> {
    let mut choices: Vec<BoothChoice> = pairs(outputs)
        .filter(|&first| Some(first) != headphones || Some(first) == chosen)
        .map(|first| BoothChoice { first, taken: Some(first) == headphones })
        .collect();
    if let Some(first) = chosen.filter(|c| !choices.iter().any(|b| b.first == *c)) {
        choices.push(BoothChoice { first, taken: false });
    }
    choices.sort_unstable_by_key(|b| b.first);
    choices
}

/// Why the booth `chosen` is heard or not on the software mixer of the running device, its
/// headphones on `headphones`: nothing to say if none is chosen.
pub fn booth_note(
    in_use: Option<usize>,
    chosen: Option<usize>,
    headphones: Option<usize>,
) -> Option<OutputNote> {
    if let Some(first) = in_use {
        return Some(OutputNote::On { first });
    }
    let chosen = chosen?;
    Some(if Some(chosen) == headphones {
        OutputNote::TakenByHeadphones { first: chosen }
    } else {
        OutputNote::NotOnDevice
    })
}

/// Whether two choices of inputs share one.
pub fn overlap(a: InputChannelsDto, b: InputChannelsDto) -> bool {
    a.channels().any(|c| b.channels().any(|d| c == d))
}

/// What a live source can come in on among the `free` inputs the `other` source leaves: each
/// pair, and each input alone if it can be `mono`; the one `chosen` as well, on this device or
/// not.
pub fn inputs(
    free: &[usize],
    other: Option<InputChannelsDto>,
    chosen: Option<InputChannelsDto>,
    mono: bool,
) -> Vec<InputChannelsDto> {
    let left = |c: &usize| other.is_none_or(|o| !o.channels().any(|t| t == *c));
    let free: Vec<usize> = free.iter().copied().filter(left).collect();
    let mut choices = Vec::new();
    for &first in &free {
        if mono {
            choices.push(InputChannelsDto { first, stereo: false });
        }
        if first % 2 == 0 && free.contains(&(first + 1)) {
            choices.push(InputChannelsDto { first, stereo: true });
        }
    }
    if let Some(chosen) = chosen.filter(|c| !choices.contains(c)) {
        choices.push(chosen);
    }
    choices
}

/// Why a live source is not heard on the running device with the `free` inputs: its inputs
/// `chosen` are not there, or there is nothing to choose.
pub fn live_note(chosen: Option<InputChannelsDto>, in_use: bool, free: &[usize]) -> Option<LiveNote> {
    match chosen {
        Some(_) if !in_use => Some(LiveNote::NotOnDevice),
        None if free.is_empty() => Some(LiveNote::TurntablesTakeAll),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(first: usize) -> InputChannelsDto {
        InputChannelsDto { first, stereo: true }
    }

    fn one(first: usize) -> InputChannelsDto {
        InputChannelsDto { first, stereo: false }
    }

    fn booths(outputs: usize, headphones: Option<usize>, chosen: Option<usize>) -> Vec<(usize, bool)> {
        booth(outputs, headphones, chosen).iter().map(|b| (b.first, b.taken)).collect()
    }

    #[test]
    fn the_booth_leaves_out_the_master_and_the_headphones() {
        assert_eq!(booths(8, Some(2), None), [(4, false), (6, false)]);
        assert_eq!(booths(4, Some(2), None), []);
        assert_eq!(booths(4, None, None), [(2, false)]);
    }

    #[test]
    fn the_booth_offers_the_pair_chosen_taken_when_the_headphones_have_it() {
        assert_eq!(booths(4, Some(2), Some(2)), [(2, true)]);
        assert_eq!(booths(2, None, Some(4)), [(4, false)]);
    }

    #[test]
    fn the_headphones_take_any_pair_after_the_masters_and_the_one_chosen() {
        assert_eq!(headphones(6, HeadphonesOutputDto::Auto), [2, 4]);
        assert_eq!(headphones(2, HeadphonesOutputDto::Pair { first: 4 }), [4]);
        assert_eq!(headphones(6, HeadphonesOutputDto::Pair { first: 2 }), [2, 4]);
    }

    #[test]
    fn a_booth_not_heard_says_why() {
        assert_eq!(booth_note(Some(4), Some(4), Some(2)), Some(OutputNote::On { first: 4 }));
        assert_eq!(booth_note(None, None, Some(2)), None);
        assert_eq!(booth_note(None, Some(2), Some(2)), Some(OutputNote::TakenByHeadphones { first: 2 }));
        assert_eq!(booth_note(None, Some(6), Some(2)), Some(OutputNote::NotOnDevice));
    }

    #[test]
    fn a_live_source_offers_the_inputs_the_other_leaves() {
        let free = [2, 3, 4, 5];
        assert_eq!(inputs(&free, None, None, false), [pair(2), pair(4)], "the aux line in stereo");
        assert_eq!(
            inputs(&free, Some(pair(4)), None, true),
            [one(2), pair(2), one(3)],
            "the microphone alone or in stereo, not on the aux line's"
        );
        assert_eq!(inputs(&free, Some(one(3)), None, false), [pair(4)], "a pair half taken is not offered");
        assert_eq!(inputs(&[], None, Some(pair(6)), false), [pair(6)], "the one chosen, elsewhere");
    }

    #[test]
    fn inputs_shared_overlap() {
        assert!(overlap(pair(2), one(3)));
        assert!(!overlap(pair(2), pair(4)));
        assert!(overlap(one(2), one(2)));
    }

    #[test]
    fn a_live_source_not_heard_says_why() {
        assert_eq!(live_note(Some(pair(2)), false, &[4, 5]), Some(LiveNote::NotOnDevice));
        assert_eq!(live_note(None, false, &[]), Some(LiveNote::TurntablesTakeAll));
        assert_eq!(live_note(Some(pair(2)), true, &[2, 3]), None);
    }
}
