use crate::actions::pads;
use crate::error::AppResult;
use crate::state::AppState;
use prisme_actions::pads::Page;
use prisme_control::{beats_text, Beats, DeckAction, Label, Target, CUT_BEATS, ROLL_BEATS};
use serde::Serialize;
use tauri::State;

/// A page of a deck's pads on the screen whose pads are named by their action.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PadPageDto {
    pub page: Page,
    pub pads: Vec<PadDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PadDto {
    /// Its action, of whichever deck (`loop.0.25`).
    pub action: String,
    /// What it shows: its beats, or its number.
    pub caption: String,
    pub label: Label,
}

/// The pages of pads the screen names by their action, in the order of `Page::ALL`: all but the
/// effects', which it names by the effect.
#[tauri::command]
pub fn pad_pages() -> Vec<PadPageDto> {
    let caption = |action| match action {
        DeckAction::LoopBeats(Beats(beats)) | DeckAction::JumpBeats(Beats(beats)) => beats_text(beats),
        DeckAction::Roll(roll) => beats_text(ROLL_BEATS[roll]),
        DeckAction::Cut(cut) => beats_text(CUT_BEATS[cut]),
        DeckAction::Slice(slice) => (slice + 1).to_string(),
        _ => String::new(),
    };
    let pads = |page: Page| {
        page.actions(0)
            .into_iter()
            .map(|action| PadDto {
                action: action.to_string(),
                caption: caption(action),
                label: Target::Deck(0, action).label(),
            })
            .collect()
    };
    Page::ALL
        .into_iter()
        .filter(|page| *page != Page::Fx)
        .map(|page| PadPageDto { page, pads: pads(page) })
        .collect()
}

/// The slot of the chain of `deck` the screen's pads and knobs act on.
#[tauri::command]
pub fn show_fx_slot(state: State<'_, AppState>, deck: usize, slot: usize) -> AppResult<()> {
    crate::actions::fx::show_fx_slot(&state, deck, slot)
}

/// The slicer of `deck` put on the bar the track is in, as its pads are shown.
#[tauri::command]
pub fn anchor_slicer(state: State<'_, AppState>, deck: usize) -> AppResult<()> {
    pads::anchor_slicer(&state, deck)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_of_pads_name_each_pad_by_its_action_and_its_beats_or_number() {
        let pages = pad_pages();
        let first = |page: Page| {
            pages.iter().find(|p| p.page == page).map(|p| (&p.pads[0].action, &p.pads[0].caption))
        };
        assert_eq!(first(Page::Loops), Some((&"loop.0.25".to_owned(), &"1/4".to_owned())));
        assert_eq!(first(Page::Jump), Some((&"jump.-32".to_owned(), &"-32".to_owned())));
        assert_eq!(first(Page::Slice), Some((&"slice1".to_owned(), &"1".to_owned())));
        assert_eq!(pages.iter().find(|p| p.page == Page::Cut).map(|p| p.pads.len()), Some(4));
        assert!(pages.iter().all(|p| p.page != Page::Fx), "the effects are named by the effect");
    }
}
