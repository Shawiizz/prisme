//! Searches that forgive a slip of the finger: a word typed with a letter wrong, missing or
//! too many, or two letters swapped, still finds the tracks it was meant for, after those that
//! hold it as typed.

/// The slips a word of `letters` letters may hold and still find a word: none under four
/// letters (too few to tell a slip from another word), one up to six, two beyond.
fn allowed(letters: usize) -> usize {
    match letters {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    }
}

/// The words of `text`, letters and digits, each as its letters: what [`slips`] reads.
pub(crate) fn words_of(text: &str) -> Box<[Box<[char]>]> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().collect())
        .collect()
}

/// How many slips `typed` (folded words, see `catalogue::search_words`) are from `text` (a
/// track's folded file name and tags), its `words` ([`words_of`]): for each word, none if the
/// text holds it, else the fewest that make it one of the text's words or the start of one (a
/// word being typed). None if a word is further from all of them than it may be.
pub(crate) fn slips(typed: &[String], text: &str, words: &[Box<[char]>]) -> Option<usize> {
    typed.iter().map(|word| slips_of(word, text, words)).sum()
}

fn slips_of(word: &str, text: &str, words: &[Box<[char]>]) -> Option<usize> {
    if text.contains(word) {
        return Some(0);
    }
    let word: Vec<char> = word.chars().collect();
    let allowed = allowed(word.len());
    if allowed == 0 {
        return None;
    }
    words.iter().filter_map(|other| near(&word, other, allowed)).min()
}

/// The slips between `word` and `other`, or the start of `other` a letter shorter, as long or a
/// letter longer: none beyond `allowed`.
fn near(word: &[char], other: &[char], allowed: usize) -> Option<usize> {
    let starts = (word.len().saturating_sub(1)..=word.len() + 1).filter(|&n| n < other.len());
    let found = starts.map(|n| distance(word, &other[..n])).chain([distance(word, other)]).min()?;
    (found <= allowed).then_some(found)
}

/// The fewest letters changed, put in, left out or swapped with the next that make `a` into `b`
/// (the optimal string alignment distance).
fn distance(a: &[char], b: &[char]) -> usize {
    let width = b.len() + 1;
    let mut rows = vec![0usize; (a.len() + 1) * width];
    let at = |i: usize, j: usize| i * width + j;
    for i in 0..=a.len() {
        rows[at(i, 0)] = i;
    }
    for j in 0..=b.len() {
        rows[at(0, j)] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let changed = usize::from(a[i - 1] != b[j - 1]);
            let mut best =
                (rows[at(i - 1, j)] + 1).min(rows[at(i, j - 1)] + 1).min(rows[at(i - 1, j - 1)] + changed);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[at(i - 2, j - 2)] + 1);
            }
            rows[at(i, j)] = best;
        }
    }
    rows[at(a.len(), b.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    const STROBE: &str = "strobe.mp3 strobe deadmau5 for lack of a better name progressive house";

    /// How many slips `text` typed is from [`STROBE`].
    fn slips_from_strobe(text: &str) -> Option<usize> {
        slips(&crate::catalogue::search_words(text), STROBE, &words_of(STROBE))
    }

    #[test]
    fn a_word_as_typed_is_no_slip() {
        assert_eq!(slips_from_strobe("strobe deadmau"), Some(0));
    }

    #[test]
    fn a_letter_wrong_missing_too_many_or_two_swapped_is_one_slip() {
        for slip in ["strpbe", "strbe", "stroobe", "srtobe"] {
            assert_eq!(slips_from_strobe(slip), Some(1), "{slip}");
        }
    }

    #[test]
    fn a_word_being_typed_finds_the_start_of_one() {
        assert_eq!(slips_from_strobe("progrs"), Some(1), "progre(ssive), a letter wrong");
        assert_eq!(slips_from_strobe("deadmua5 progressiev"), Some(2));
    }

    #[test]
    fn a_short_word_or_one_too_far_finds_nothing() {
        assert_eq!(slips_from_strobe("hpuse"), Some(1));
        assert_eq!(slips_from_strobe("hos"), None, "three letters: as typed only");
        assert_eq!(slips_from_strobe("techno"), None);
        assert_eq!(slips_from_strobe("strobe techno"), None, "every word found");
    }

    #[test]
    fn a_long_word_may_hold_two_slips() {
        assert_eq!(slips_from_strobe("progersive"), Some(2), "swapped, then a letter missing");
        assert_eq!(slips_from_strobe("prgersive"), None, "three");
    }
}
