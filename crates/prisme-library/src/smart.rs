//! Smart crates: crates filled by rules on the tags and analysis of the tracks.

use crate::db::TrackRecord;
use prisme_analysis::TrackAnalysis;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartRules {
    /// Every rule must match; otherwise one is enough.
    pub match_all: bool,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextField {
    Title,
    Artist,
    Album,
    Genre,
    Comment,
    FileName,
}

/// Bounds are inclusive; a missing bound is open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Rule {
    /// The field contains the text, ignoring case.
    Text {
        field: TextField,
        contains: String,
    },
    Bpm {
        min: Option<f64>,
        max: Option<f64>,
    },
    Year {
        min: Option<u16>,
        max: Option<u16>,
    },
    /// One of these Camelot codes.
    Key {
        camelot: Vec<String>,
    },
}

impl SmartRules {
    /// Whether the track belongs to the crate; no rule, no track.
    pub fn matches(&self, record: &TrackRecord) -> bool {
        if self.rules.is_empty() || record.error.is_some() {
            return false;
        }
        let analysis = record.effective_analysis();
        let check = |rule: &Rule| rule.matches(record, &analysis);
        if self.match_all {
            self.rules.iter().all(check)
        } else {
            self.rules.iter().any(check)
        }
    }
}

impl Rule {
    fn matches(&self, record: &TrackRecord, analysis: &TrackAnalysis) -> bool {
        match self {
            Rule::Text { field, contains } => {
                text(record, *field).is_some_and(|t| t.to_lowercase().contains(&contains.to_lowercase()))
            }
            Rule::Bpm { min, max } => analysis.tempo.is_some_and(|t| within(t.bpm, *min, *max)),
            Rule::Year { min, max } => record.tags.year.is_some_and(|y| within(y, *min, *max)),
            Rule::Key { camelot } => {
                analysis.key.is_some_and(|k| camelot.iter().any(|c| c.eq_ignore_ascii_case(&k.key.camelot())))
            }
        }
    }
}

fn text(record: &TrackRecord, field: TextField) -> Option<&str> {
    let tags = &record.tags;
    match field {
        TextField::Title => tags.title.as_deref(),
        TextField::Artist => tags.artist.as_deref(),
        TextField::Album => tags.album.as_deref(),
        TextField::Genre => tags.genre.as_deref(),
        TextField::Comment => tags.comment.as_deref(),
        TextField::FileName => Path::new(&record.path).file_name().and_then(|n| n.to_str()),
    }
}

fn within<T: PartialOrd>(value: T, min: Option<T>, max: Option<T>) -> bool {
    min.is_none_or(|m| value >= m) && max.is_none_or(|m| value <= m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileStamp;
    use prisme_analysis::{Key, KeyEstimate, Tempo};
    use prisme_media::TrackTags;

    fn track(genre: &str, bpm: f64, camelot: &str, year: u16) -> TrackRecord {
        let analysis = TrackAnalysis {
            tempo: Some(Tempo { bpm, first_beat_s: 0.0, confidence: 1.0 }),
            key: Key::from_camelot(camelot).map(|key| KeyEstimate {
                key,
                confidence: 1.0,
                tuning_cents: 0.0,
            }),
        };
        let tags = TrackTags { genre: Some(genre.into()), year: Some(year), ..TrackTags::default() };
        TrackRecord::analyzed(
            Path::new("C:/Music/track.mp3"),
            FileStamp { size: 1, modified_ms: 1 },
            1.0,
            analysis,
            tags,
        )
    }

    #[test]
    fn rules_combine_with_all_or_any() {
        let house = Rule::Text { field: TextField::Genre, contains: "house".into() };
        let fast = Rule::Bpm { min: Some(125.0), max: None };
        let all = SmartRules { match_all: true, rules: vec![house.clone(), fast.clone()] };
        let any = SmartRules { match_all: false, rules: vec![house, fast] };

        let deep = track("Deep House", 122.0, "8A", 2020);
        let techno = track("Techno", 132.0, "8A", 2020);
        assert!(!all.matches(&deep) && any.matches(&deep));
        assert!(!all.matches(&techno) && any.matches(&techno));
        assert!(all.matches(&track("Tech House", 126.0, "8A", 2020)));
    }

    #[test]
    fn each_kind_of_rule() {
        let t = track("House", 124.0, "8A", 2019);
        let one = |rule| SmartRules { match_all: true, rules: vec![rule] }.matches(&t);
        assert!(one(Rule::Bpm { min: Some(124.0), max: Some(124.0) }), "bounds are inclusive");
        assert!(!one(Rule::Year { min: Some(2020), max: None }));
        assert!(one(Rule::Key { camelot: vec!["7a".into(), "8a".into()] }));
        assert!(one(Rule::Text { field: TextField::FileName, contains: "TRACK".into() }));
        assert!(!one(Rule::Text { field: TextField::Artist, contains: String::new() }), "no artist tag");
        assert!(!SmartRules { match_all: true, rules: vec![] }.matches(&t));
    }

    #[test]
    fn rules_read_back_from_json() {
        let rules = SmartRules {
            match_all: false,
            rules: vec![Rule::Key { camelot: vec!["8A".into()] }, Rule::Bpm { min: None, max: Some(100.0) }],
        };
        let json = serde_json::to_string(&rules).unwrap();
        assert_eq!(serde_json::from_str::<SmartRules>(&json).unwrap(), rules);
    }
}
