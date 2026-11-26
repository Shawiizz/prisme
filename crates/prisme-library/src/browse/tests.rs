use super::*;
use crate::catalogue::tests::{record, tagged};
use crate::smart::{Rule, SmartRules, TextField};
use prisme_analysis::Key;
use prisme_media::TrackTags;

fn tags(title: &str, artist: Option<&str>, genre: Option<&str>) -> TrackTags {
    TrackTags {
        title: Some(title.into()),
        artist: artist.map(Into::into),
        genre: genre.map(Into::into),
        ..TrackTags::default()
    }
}

/// A folder of four tracks, three analysed, shown; a fifth track from elsewhere.
fn folder() -> Browser {
    let mut browser = Browser::default();
    browser.set_roots(vec![PathBuf::from("C:/Music")]);
    browser.load(Tracks::of(vec![
        record(
            "C:/Music/a.mp3",
            tags("Strobe", Some("deadmau5"), Some("Progressive House")),
            Some(128.0),
            Some("8A"),
        ),
        record("C:/Music/b.mp3", tags("Alone", Some("Claptone"), None), Some(122.0), Some("3B")),
        record("C:/Music/c.mp3", tags("Café Noir", None, Some("House")), None, None),
        record("D:/Downloads/d.mp3", tags("Dropped", Some("Someone"), None), Some(174.0), Some("1A")),
    ]));
    let rows =
        browser.entries(["a.mp3", "b.mp3", "c.mp3", "e.mp3"].map(|name| (format!("C:/Music/{name}"), None)));
    browser.show(List::Folder("C:/Music".into()), rows);
    browser
}

fn titles(browser: &mut Browser) -> Vec<String> {
    browser.set_window(0, 100);
    browser.window().map(|row| row.track.title.clone()).collect()
}

fn entry(browser: &Browser, name: &str) -> Entry {
    browser.find(&format!("C:/Music/{name}"), None).unwrap()
}

#[test]
fn the_library_is_searched_by_the_lists_rule() {
    let browser = folder();
    let found = |text: &str| browser.search(text, 10).map(|t| t.title.clone()).collect::<Vec<_>>();
    assert_eq!(found("ouse"), ["Strobe", "Café Noir"]);
    assert_eq!(found("e.mp3"), Vec::<String>::new(), "not analysed");
    assert_eq!(found("o").len(), 4);
    assert_eq!(browser.search("o", 2).count(), 2);
}

#[test]
fn a_list_is_shown_by_title_and_searched_by_every_word_in_one_way() {
    let mut browser = folder();
    assert_eq!(titles(&mut browser), ["Alone", "Café Noir", "e", "Strobe"]);
    browser.set_filter("ouse");
    assert_eq!(titles(&mut browser), ["Café Noir", "Strobe"], "a word inside another");
    browser.set_filter("ouse  cafe");
    assert_eq!(titles(&mut browser), ["Café Noir"], "accents aside");
    browser.set_filter("e.mp3");
    assert_eq!(titles(&mut browser), ["e"], "the file name");
    browser.set_filter("");
    assert_eq!(browser.count(), 4);
    browser.set_scope(Scope::Library);
    browser.set_filter("o");
    assert_eq!(
        titles(&mut browser),
        ["Alone", "Café Noir", "Dropped", "Strobe"],
        "the library, by the same rule"
    );
}

#[test]
fn each_column_sorts_the_tracks_without_a_value_last_and_again_the_other_way() {
    let mut browser = folder();
    browser.sort_by(Column::Bpm, None);
    assert_eq!(titles(&mut browser), ["Alone", "Strobe", "Café Noir", "e"]);
    browser.sort_by(Column::Bpm, None);
    assert_eq!(titles(&mut browser), ["Café Noir", "e", "Strobe", "Alone"], "reversed, ties in their order");
    browser.sort_by(Column::Artist, None);
    assert_eq!(titles(&mut browser), ["Alone", "Strobe", "Café Noir", "e"]);
    browser.sort_by(Column::Key, None);
    assert_eq!(titles(&mut browser), ["Alone", "Strobe", "Café Noir", "e"], "3B before 8A");
    browser.sort_by(Column::Genre, None);
    assert_eq!(titles(&mut browser), ["Café Noir", "Strobe", "Alone", "e"]);
}

#[test]
fn a_list_with_an_order_of_its_own_opens_in_it_and_numbers_its_rows() {
    let mut browser = folder();
    let rows = browser.entries(["b", "a", "b"].map(|name| (format!("C:/Music/{name}.mp3"), None)));
    browser.show(List::Prepare, rows);
    assert_eq!(browser.sort(), Sort { column: Column::Order, descending: false });
    browser.set_window(0, 10);
    let rows: Vec<_> = browser.window().map(|r| (r.track.title.clone(), r.position)).collect();
    assert_eq!(rows, [("Alone".into(), Some(1)), ("Strobe".into(), Some(2)), ("Alone".into(), Some(3))]);
    assert!(browser.reorderable() && browser.removable());
    let folder = browser.entries([("C:/Music/a.mp3".to_string(), None)]);
    browser.show(List::Folder("C:/Music".into()), folder);
    assert_eq!(browser.sort().column, Column::Title, "a folder has no order of its own");
    assert!(!browser.removable());
}

#[test]
fn a_frame_drawn_over_rows_picks_them_and_a_shift_click_goes_on_from_where_it_started() {
    let mut browser = folder();
    browser.select_span(3, 1);
    let picked = |browser: &Browser| {
        let mut paths = browser.selected_paths();
        paths.sort();
        paths.iter().map(|p| p.trim_start_matches("C:/Music/").to_owned()).collect::<Vec<_>>()
    };
    // Alone (b), Café Noir (c), e, Strobe (a)
    assert_eq!(picked(&browser), ["a.mp3", "c.mp3", "e.mp3"]);
    assert_eq!(browser.cursor_index(), Some(1));
    let alone = browser.find("C:/Music/b.mp3", None).unwrap();
    browser.select(alone, Pick::Range);
    assert_eq!(picked(&browser), ["a.mp3", "b.mp3", "c.mp3", "e.mp3"], "from the row it started on");
    browser.select_span(9, 9);
    assert_eq!(picked(&browser), ["a.mp3"], "held to the last row");
}

#[test]
fn a_crate_filled_by_hand_opens_in_its_own_order_and_is_reordered_in_it() {
    let mut browser = folder();
    let rules = SmartRules { match_all: true, rules: vec![] };
    let hand = Crate { id: CrateId(1), name: "Hand".into(), rules: None, track_count: 2 };
    let smart = Crate { id: CrateId(2), name: "Smart".into(), rules: Some(rules), track_count: 0 };
    browser.set_crates(vec![hand, smart]);
    let rows = browser.entries(["b", "a"].map(|name| (format!("C:/Music/{name}.mp3"), None)));
    browser.show(List::Crate(CrateId(1)), rows);
    assert_eq!(titles(&mut browser), ["Alone", "Strobe"]);
    assert!(browser.ordered() && browser.reorderable());
    browser.sort_by(Column::Title, None);
    assert!(browser.ordered() && !browser.reorderable(), "sorted otherwise: numbered, not moved");
    let rows = browser.rows_of(&List::Crate(CrateId(2))).unwrap();
    browser.show(List::Crate(CrateId(2)), rows);
    assert!(!browser.ordered() && !browser.reorderable(), "a smart crate has no order");
}

#[test]
fn a_track_played_twice_in_a_set_is_two_rows_picked_apart() {
    let mut browser = folder();
    let rows = browser.entries([1, 2].map(|at| ("C:/Music/a.mp3".to_string(), Some(at))));
    browser.show(List::History(SessionId(1)), rows);
    let second = browser.find("C:/Music/a.mp3", Some(2)).unwrap();
    browser.select(second, Pick::Only);
    assert_eq!(browser.selected(), [second]);
    browser.select_all();
    assert_eq!(browser.selected_paths(), ["C:/Music/a.mp3"], "one track");
    assert_eq!(browser.selected_count(), 1);
}

#[test]
fn a_cursor_moved_within_the_list_is_told_apart_from_another_list_shown() {
    let mut browser = folder();
    let listed = browser.listed();
    browser.move_cursor(1);
    assert_eq!(browser.listed(), listed, "moved within the list");
    browser.set_filter("e");
    browser.sort_by(Column::Title, None);
    browser.set_scope(Scope::Library);
    assert_eq!(browser.listed(), listed + 3);
    browser.show(List::Prepare, Vec::new());
    assert_eq!(browser.listed(), listed + 4);
}

#[test]
fn rows_are_picked_alone_added_or_by_range_and_the_cursor_moves_through_those_shown() {
    let mut browser = folder();
    let [alone, cafe, e, strobe] = ["b.mp3", "c.mp3", "e.mp3", "a.mp3"].map(|n| entry(&browser, n));
    browser.select(cafe, Pick::Only);
    browser.select(strobe, Pick::Range);
    assert_eq!(browser.selected(), [cafe, e, strobe]);
    browser.select(e, Pick::Toggle);
    assert_eq!(browser.selected(), [cafe, strobe]);
    browser.select(cafe, Pick::Context);
    assert_eq!(browser.selected(), [cafe, strobe], "a right click in the selection keeps it");
    browser.select(alone, Pick::Context);
    assert_eq!(browser.selected(), [alone]);
    browser.move_cursor(2);
    assert_eq!(browser.selected(), [e]);
    browser.move_cursor(-9);
    assert_eq!(browser.cursor_index(), Some(0), "held at the ends");
    let moves = browser.cursor_moves();
    browser.move_cursor(-1);
    assert_eq!(browser.cursor_moves(), moves, "it stayed on its row");
    browser.select_all();
    assert_eq!((browser.selected_paths().len(), browser.selected_count()), (4, 4));
    browser.set_filter("house");
    assert_eq!(browser.selected(), [cafe, strobe], "the picks hidden by the search left out");
    browser.clear_selection();
    assert_eq!(browser.selected(), [], "the cursor's row is hidden");
}

#[test]
fn the_cursor_moves_with_a_gesture_not_when_rows_before_it_move() {
    let mut browser = folder();
    let cafe = entry(&browser, "c.mp3");
    browser.select(cafe, Pick::Only);
    let (at, moves) = (browser.cursor_index(), browser.cursor_moves());
    browser.put(record("C:/Music/e.mp3", tags("Aardvark", None, None), Some(100.0), None));
    assert_eq!(browser.cursor_index(), at.map(|at| at + 1), "a row came before it");
    assert_eq!(browser.cursor_moves(), moves);
    browser.move_cursor(-1);
    assert_eq!(browser.cursor_moves(), moves + 1);
}

#[test]
fn the_cursor_s_track_is_taken_only_while_its_row_is_shown() {
    let mut browser = folder();
    browser.select(entry(&browser, "c.mp3"), Pick::Only);
    assert_eq!(browser.cursor().map(|t| t.title.as_str()), Some("Café Noir"));
    browser.set_filter("strobe");
    assert!(browser.cursor().is_none(), "hidden by the search");
    browser.set_filter("");
    assert_eq!(browser.cursor().map(|t| t.title.as_str()), Some("Café Noir"), "shown again");
    browser.show(List::Prepare, Vec::new());
    assert!(browser.cursor().is_none(), "another list shown");
}

#[test]
fn the_cursor_goes_to_the_row_after_those_that_leave_else_before() {
    let browser = folder();
    let [alone, cafe, e, strobe] = ["b.mp3", "c.mp3", "e.mp3", "a.mp3"].map(|n| entry(&browser, n));
    assert_eq!(browser.neighbour(&[cafe, e]), Some(strobe));
    assert_eq!(browser.neighbour(&[e, strobe]), Some(cafe));
    assert_eq!(browser.neighbour(&[alone, cafe, e, strobe]), None);
}

#[test]
fn a_track_analysed_takes_its_place_in_the_list_and_joins_the_library_searched() {
    let mut browser = folder();
    browser.put(record("C:/Music/e.mp3", tags("Aardvark", None, None), Some(100.0), None));
    assert_eq!(titles(&mut browser), ["Aardvark", "Alone", "Café Noir", "Strobe"]);
    browser.set_scope(Scope::Library);
    browser.set_filter("a");
    let before = browser.count();
    browser.put(tagged("C:/Music/new.mp3", "Zebra", "a"));
    assert_eq!(browser.count(), before + 1);
    assert_eq!(titles(&mut browser).last().map(String::as_str), Some("Zebra"));
    browser.forget(&["C:/Music/new.mp3".into()]);
    assert_eq!(browser.count(), before);
}

#[test]
fn the_tracks_under_none_of_the_music_folders_are_from_elsewhere() {
    let mut browser = folder();
    let names = |b: &Browser| b.elsewhere().map(|t| t.title.clone()).collect::<Vec<_>>();
    assert_eq!(names(&browser), ["Dropped"]);
    browser.put(tagged("E:/Sets/x.mp3", "X", "a"));
    assert_eq!(names(&browser), ["Dropped", "X"]);
    browser.set_roots(vec!["C:/Music".into(), "E:/Sets".into()]);
    assert_eq!(names(&browser), ["Dropped"]);
    let rows = browser.rows_of(&List::Elsewhere).unwrap();
    browser.show(List::Elsewhere, rows);
    browser.forget(&["D:/Downloads/d.mp3".into()]);
    assert_eq!(browser.count(), 0, "the list follows");
}

#[test]
fn a_smart_crate_picks_its_tracks_from_the_library_and_counts_them() {
    let mut browser = folder();
    let rules = SmartRules {
        match_all: true,
        rules: vec![Rule::Text { field: TextField::Genre, contains: "house".into() }],
    };
    let smart = Crate { id: CrateId(2), name: "House".into(), rules: Some(rules), track_count: 0 };
    let hand = Crate { id: CrateId(1), name: "Hand".into(), rules: None, track_count: 7 };
    browser.set_crates(vec![hand, smart]);
    assert_eq!(browser.crates().iter().map(|c| c.track_count).collect::<Vec<_>>(), [7, 2]);
    let rows = browser.rows_of(&List::Crate(CrateId(2))).unwrap();
    browser.show(List::Crate(CrateId(2)), rows);
    assert_eq!(titles(&mut browser), ["Café Noir", "Strobe"]);
    assert!(!browser.removable(), "it picks its own");
    assert!(browser.rows_of(&List::Crate(CrateId(1))).is_none(), "the database's");
    // a track analysed or edited joins the crate shown, and its count, at once
    browser.put(record("C:/Music/b.mp3", tags("Alone", Some("Claptone"), Some("Deep House")), None, None));
    assert_eq!(titles(&mut browser), ["Alone", "Café Noir", "Strobe"]);
    assert_eq!(browser.crates()[1].track_count, 3);
    browser.put(record("C:/Music/a.mp3", tags("Strobe", None, Some("Techno")), None, None));
    assert_eq!(titles(&mut browser), ["Alone", "Café Noir"], "and leaves it");
    browser.forget(&["C:/Music/c.mp3".into()]);
    assert_eq!(browser.crates()[1].track_count, 1);
}

#[test]
fn a_track_edited_into_a_duplicate_shows_among_them_at_once() {
    let mut browser = folder();
    let rows = browser.rows_of(&List::Duplicates).unwrap();
    browser.show(List::Duplicates, rows);
    assert_eq!(browser.count(), 0);
    let count = |b: &Browser| b.lists_revision();
    let before = count(&browser);
    browser.put(record("C:/Music/b.mp3", tags("Strobe", Some("deadmau5"), None), None, None));
    assert_eq!(titles(&mut browser), ["Strobe", "Strobe"]);
    assert!(count(&browser) > before, "the side panel told");
}

#[test]
fn a_controller_points_at_the_lists_held_at_the_ends_and_opens_the_next() {
    let mut browser = folder();
    let named = |id, name: &str| Crate { id: CrateId(id), name: name.into(), rules: None, track_count: 0 };
    browser.set_crates(vec![named(3, "A"), named(7, "B")]);
    browser.point(1);
    assert_eq!((browser.pointed(), browser.focus()), (Shelf::Crate(CrateId(3)), Focus::Lists));
    browser.point(5);
    assert_eq!(browser.pointed(), Shelf::Crate(CrateId(7)));
    assert_eq!(browser.open_pointed(), List::Crate(CrateId(7)));
    assert_eq!(browser.focus(), Focus::Tracks);
    browser.show(List::Prepare, Vec::new());
    assert_eq!(browser.next_list(1), List::Duplicates, "after the list shown");
    browser.point(-4);
    assert_eq!(browser.pointed(), Shelf::Prepare);
}

#[test]
fn a_controller_opens_the_lists_of_the_side_panel_in_turn() {
    let mut browser = Browser::default();
    browser.set_roots(vec![PathBuf::from("C:/Music")]);
    browser.load(Tracks::of(vec![
        record("C:/Music/Mixxx/sets/a.mp3", tags("A", None, None), None, None),
        record("C:/Music/house/b.mp3", tags("B", None, None), None, None),
        record("C:/Music/c.mp3", tags("C", None, None), None, None),
    ]));
    browser.set_crates(vec![Crate { id: CrateId(3), name: "A".into(), rules: None, track_count: 0 }]);
    browser.set_sessions(vec![HistorySession { id: SessionId(9), started_at_ms: 0, play_count: 1 }]);
    browser.show(List::Prepare, Vec::new());
    let opened: Vec<List> = (0..7)
        .map(|_| {
            let next = browser.next_list(1);
            browser.show(next.clone(), Vec::new());
            next
        })
        .collect();
    let folder = |path: &str| List::Folder(PathBuf::from(path));
    assert_eq!(
        opened,
        [
            List::Duplicates,
            folder("C:/Music"),
            folder("C:/Music/house"),
            folder("C:/Music/Mixxx"),
            List::Crate(CrateId(3)),
            List::History(SessionId(9)),
            List::History(SessionId(9)),
        ]
    );
    browser.show(folder("C:/Music/Mixxx/sets"), Vec::new());
    assert_eq!(browser.next_list(-1), folder("C:/Music/house"), "from the folder it is in");
    assert_eq!(browser.focus(), Focus::Tracks);
}

#[test]
fn the_rows_follow_the_deck_they_are_ranked_against() {
    let mut browser = folder();
    let mix = |key: &str| MixReference {
        bpm: Some(128.0),
        key: Key::from_camelot(key),
        path: None,
        range: 0.08,
        key_lock: true,
    };
    browser.sort_by(Column::Mix, Some(mix("8A")));
    assert_eq!(titles(&mut browser)[..2], ["Strobe", "Alone"]);
    browser.set_mix(mix("3B"));
    assert_eq!(titles(&mut browser)[..2], ["Alone", "Strobe"]);
    browser.set_tonight(["C:/Music/b.mp3"]);
    assert_eq!(titles(&mut browser)[..2], ["Strobe", "Café Noir"], "played tonight: last");
}

#[test]
fn the_window_holds_the_rows_asked_for_and_says_what_each_row_is() {
    let mut browser = folder();
    browser.set_tonight(["C:/Music/a.mp3"]);
    let strobe = entry(&browser, "a.mp3");
    browser.select(strobe, Pick::Only);
    browser.set_window(3, 1);
    let rows: Vec<_> = browser.window().map(|r| (r.track.title.clone(), r.selected, r.played)).collect();
    assert_eq!(rows, [("Strobe".into(), true, true)]);
    assert_eq!(browser.window_first(), 3);
    let seq = browser.seq();
    browser.set_window(3, 1);
    assert_eq!(browser.seq(), seq, "the same window");
    browser.set_filter("ouse");
    assert_eq!(titles_in_window(&browser), ["Strobe"], "the list got shorter: its end");
    browser.set_filter("zzz");
    assert_eq!((browser.window_first(), browser.window().count()), (0, 0));
    assert!(browser.seq() > seq);
}

fn titles_in_window(browser: &Browser) -> Vec<String> {
    browser.window().map(|row| row.track.title.clone()).collect()
}

#[test]
fn duplicates_and_the_prepare_list_are_made_from_the_library() {
    let mut browser = folder();
    browser.put(record("C:/Music/copy.mp3", tags("Strobe!", Some("Deadmau5"), None), None, None));
    assert_eq!(browser.rows_of(&List::Duplicates).unwrap().len(), 2);
    browser.set_prepare(&["C:/Music/c.mp3".into(), "C:/Music/a.mp3".into()]);
    let rows = browser.rows_of(&List::Prepare).unwrap();
    browser.show(List::Prepare, rows);
    assert_eq!(titles(&mut browser), ["Café Noir", "Strobe"]);
    browser.set_prepare(&["C:/Music/a.mp3".into()]);
    assert_eq!(titles(&mut browser), ["Strobe"], "the list shown follows");
}
