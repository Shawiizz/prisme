//! The keyboard's shortcuts: the keys Prisme gives the actions, those the DJ changed over them,
//! and a key given to an action taken from the others that had it.

use crate::preferences::KeyPreference;
use prisme_control::{Kind, Target};
use serde::Serialize;
use std::collections::BTreeMap;

/// The keys the DJ changed, by action (none: taken off).
pub type KeyChanges = BTreeMap<String, Vec<KeyPreference>>;

/// A key bound to an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "KeyBinding"))]
pub struct Shortcut {
    pub control: String,
    #[serde(flatten)]
    pub key: KeyPreference,
}

/// Actions whose keys work while typing and over the settings and the palette, if they hold
/// Control or are function keys: a letter alone types.
const EVERYWHERE: [&str; 3] = ["general.help", "general.palette", "general.settings"];

/// The physical keys of each deck's actions: the left hand plays deck A, the right hand deck B;
/// Shift held, the same keys play decks C and D, as the second layer of a four-deck controller.
const DECK_KEYS: [DeckKeys; 2] = [
    DeckKeys {
        hot_cues: ["Digit1", "Digit2", "Digit3", "Digit4", "KeyQ", "KeyW", "KeyE", "KeyR"],
        actions: [
            ("cue", "KeyA"),
            ("play", "KeyS"),
            ("sync", "KeyD"),
            ("loop", "KeyF"),
            ("loop.halve", "KeyC"),
            ("loop.double", "KeyV"),
            ("bend.down", "KeyZ"),
            ("bend.up", "KeyX"),
        ],
    },
    DeckKeys {
        hot_cues: ["Digit7", "Digit8", "Digit9", "Digit0", "KeyU", "KeyI", "KeyO", "KeyP"],
        actions: [
            ("cue", "KeyJ"),
            ("play", "KeyK"),
            ("sync", "KeyL"),
            ("loop", "KeyH"),
            ("loop.halve", "Comma"),
            ("loop.double", "Period"),
            ("bend.down", "KeyN"),
            ("bend.up", "KeyM"),
        ],
    },
];

struct DeckKeys {
    hot_cues: [&'static str; 8],
    actions: [(&'static str, &'static str); 8],
}

/// The library's and the windows' keys: code, Control, Shift, action.
const OTHER_KEYS: [(&str, bool, bool, &str); 17] = [
    ("ArrowUp", false, false, "library.previous"),
    ("ArrowDown", false, false, "library.next"),
    ("ArrowLeft", false, true, "library.load1"),
    ("ArrowRight", false, true, "library.load2"),
    ("ArrowLeft", true, true, "library.double1"),
    ("ArrowRight", true, true, "library.double2"),
    ("Enter", false, false, "library.load"),
    ("Space", false, false, "library.preview"),
    ("Delete", false, false, "library.remove"),
    ("KeyP", true, false, "library.prepare"),
    ("KeyA", true, false, "library.selectAll"),
    ("Escape", false, false, "library.clearSelection"),
    ("KeyF", true, false, "library.search"),
    ("F1", false, false, "general.help"),
    ("KeyK", true, false, "general.palette"),
    ("KeyP", true, true, "general.palette"),
    ("Comma", true, false, "general.settings"),
];

fn shortcut(control: String, code: &str, ctrl: bool, shift: bool) -> Shortcut {
    Shortcut { control, key: KeyPreference { code: code.to_owned(), ctrl, shift } }
}

/// The keys Prisme gives the actions, until the DJ changes them.
pub fn defaults() -> Vec<Shortcut> {
    let decks = (0..4).flat_map(|deck| {
        let keys = &DECK_KEYS[deck % 2];
        let shift = deck >= 2;
        let hot_cues =
            keys.hot_cues.iter().enumerate().map(move |(slot, code)| (format!("hotcue{}", slot + 1), *code));
        let actions = keys.actions.iter().map(|(action, code)| ((*action).to_owned(), *code));
        hot_cues
            .chain(actions)
            .map(move |(action, code)| shortcut(format!("deck{}.{action}", deck + 1), code, false, shift))
    });
    let others = OTHER_KEYS
        .iter()
        .map(|(code, ctrl, shift, control)| shortcut((*control).to_owned(), code, *ctrl, *shift));
    decks.chain(others).collect()
}

/// The keys `control` has until the DJ changes them.
pub fn default_keys(control: &str) -> Vec<KeyPreference> {
    keys_of(&defaults(), control)
}

/// The keys of `control` among `shortcuts`.
pub fn keys_of(shortcuts: &[Shortcut], control: &str) -> Vec<KeyPreference> {
    shortcuts.iter().filter(|s| s.control == control).map(|s| s.key.clone()).collect()
}

/// The shortcuts in use: those the DJ gave, and the defaults of the actions they left alone,
/// but for a key they gave another (a default added since).
pub fn effective(changes: &KeyChanges) -> Vec<Shortcut> {
    let given: Vec<Shortcut> = changes
        .iter()
        .flat_map(|(control, keys)| {
            keys.iter().map(|key| Shortcut { control: control.clone(), key: key.clone() })
        })
        .collect();
    let kept = defaults()
        .into_iter()
        .filter(|s| !changes.contains_key(&s.control) && !given.iter().any(|g| g.key == s.key));
    kept.chain(given.iter().cloned()).collect()
}

/// The shortcuts of other actions than `control` that `keys` would take from them.
pub fn conflicts(shortcuts: &[Shortcut], control: &str, keys: &[KeyPreference]) -> Vec<Shortcut> {
    shortcuts.iter().filter(|s| s.control != control && keys.contains(&s.key)).cloned().collect()
}

/// `changes` once `control` has `keys`, taken from the actions that had them; an action back
/// to its default keys is no longer a change.
pub fn assign(changes: &KeyChanges, control: &str, keys: &[KeyPreference]) -> KeyChanges {
    let shortcuts = effective(changes);
    let mut next = changes.clone();
    let mut set = |action: &str, given: Vec<KeyPreference>| {
        let defaults = default_keys(action);
        if given.len() == defaults.len() && given.iter().all(|k| defaults.contains(k)) {
            next.remove(action);
        } else {
            next.insert(action.to_owned(), given);
        }
    };
    let mut taken: Vec<String> =
        conflicts(&shortcuts, control, keys).into_iter().map(|s| s.control).collect();
    taken.sort_unstable();
    taken.dedup();
    for action in taken {
        let left = keys_of(&shortcuts, &action).into_iter().filter(|k| !keys.contains(k)).collect();
        set(&action, left);
    }
    set(control, keys.to_vec());
    next
}

/// Whether `shortcut` acts while typing and over the settings and the palette.
pub fn everywhere(shortcut: &Shortcut) -> bool {
    let function_key = shortcut.key.code.strip_prefix('F').is_some_and(|n| n.parse::<u8>().is_ok());
    EVERYWHERE.contains(&shortcut.control.as_str()) && (shortcut.key.ctrl || function_key)
}

/// Whether `control` is an action a key can press.
pub fn keyed(control: &str) -> bool {
    control.parse::<Target>().is_ok_and(|t| t.kind() == Kind::Button)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: &str) -> KeyPreference {
        KeyPreference { code: code.into(), ctrl: false, shift: false }
    }

    fn of(shortcuts: &[Shortcut], code: &str) -> Vec<String> {
        shortcuts.iter().filter(|s| s.key == key(code)).map(|s| s.control.clone()).collect()
    }

    #[test]
    fn never_binds_the_same_key_twice() {
        let defaults = defaults();
        for (i, s) in defaults.iter().enumerate() {
            assert!(!defaults[i + 1..].iter().any(|t| t.key == s.key), "{s:?}");
        }
    }

    #[test]
    fn only_names_actions_a_key_presses() {
        let unknown: Vec<_> = defaults().into_iter().filter(|s| !keyed(&s.control)).collect();
        assert_eq!(unknown, []);
    }

    #[test]
    fn gives_every_deck_the_same_actions_decks_c_and_d_with_shift() {
        let defaults = defaults();
        let of = |deck: usize| -> Vec<(String, String)> {
            let prefix = format!("deck{deck}.");
            defaults
                .iter()
                .filter_map(|s| s.control.strip_prefix(&prefix).map(|a| (a.to_owned(), s.key.code.clone())))
                .collect()
        };
        assert_eq!(of(1).len(), 16);
        assert_eq!(of(3), of(1));
        assert_eq!(of(4), of(2));
        let play = |deck: usize| defaults.iter().find(|s| s.control == format!("deck{deck}.play")).unwrap();
        assert!(!play(1).key.shift && play(3).key.shift);
    }

    #[test]
    fn works_while_typing_only_with_control_or_a_function_key() {
        let shortcuts = effective(&KeyChanges::from([("general.palette".into(), vec![key("KeyG")])]));
        let palette = shortcuts.iter().find(|s| s.control == "general.palette").unwrap();
        assert!(!everywhere(palette));
        let defaults = defaults();
        let find = |c: &str| defaults.iter().find(|s| s.control == c).unwrap().clone();
        assert!(everywhere(&find("general.help")));
        assert!(everywhere(&find("general.settings")));
        assert!(!everywhere(&find("library.search")));
    }

    #[test]
    fn keeps_the_keys_given_and_the_defaults_of_the_other_actions() {
        let changes = assign(&KeyChanges::new(), "deck1.play", &[key("KeyG")]);
        assert_eq!(changes, KeyChanges::from([("deck1.play".into(), vec![key("KeyG")])]));
        let shortcuts = effective(&changes);
        assert_eq!(keys_of(&shortcuts, "deck1.cue"), default_keys("deck1.cue"));
        assert_eq!(of(&shortcuts, "KeyS"), Vec::<String>::new(), "S is free now");
    }

    #[test]
    fn takes_a_key_from_the_action_that_had_it() {
        let changes = assign(&KeyChanges::new(), "deck1.play", &[key("KeyA")]);
        assert_eq!(changes["deck1.cue"], []);
        assert_eq!(of(&effective(&changes), "KeyA"), ["deck1.play"]);
        // one of two keys taken, the other stays
        let ctrl_k = KeyPreference { code: "KeyK".into(), ctrl: true, shift: false };
        let palette = effective(&assign(&KeyChanges::new(), "deck1.play", &[ctrl_k]));
        let ctrl_shift_p = KeyPreference { code: "KeyP".into(), ctrl: true, shift: true };
        assert_eq!(keys_of(&palette, "general.palette"), [ctrl_shift_p]);
    }

    #[test]
    fn forgets_a_change_once_the_action_has_its_default_keys_again() {
        let changes = assign(&KeyChanges::new(), "deck1.play", &[key("KeyG")]);
        let changes = assign(&changes, "deck1.play", &default_keys("deck1.play"));
        assert_eq!(changes, KeyChanges::new());
        let changes = assign(&KeyChanges::new(), "library.search", &[]);
        assert_eq!(changes, KeyChanges::from([("library.search".into(), vec![])]));
        assert_eq!(keys_of(&effective(&changes), "library.search"), []);
    }

    #[test]
    fn leaves_a_key_of_the_dj_to_their_action_alone_though_a_default_has_it_too() {
        // as a default added in a later version would
        let shortcuts = effective(&KeyChanges::from([("deck1.cue".into(), vec![key("KeyS")])]));
        assert_eq!(of(&shortcuts, "KeyS"), ["deck1.cue"]);
    }

    #[test]
    fn tells_what_keys_would_take() {
        let shortcuts = effective(&KeyChanges::new());
        let taken = conflicts(&shortcuts, "deck1.play", &[key("KeyA"), key("KeyG")]);
        assert_eq!(taken.iter().map(|s| s.control.as_str()).collect::<Vec<_>>(), ["deck1.cue"]);
        assert_eq!(conflicts(&shortcuts, "deck1.cue", &[key("KeyA")]), []);
    }
}
