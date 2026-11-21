//! Data exchanged with the frontend (camelCase JSON), by area.

mod audio;
mod auto_dj;
mod controllers;
mod deck;
mod fx;
mod library;
mod mixer;
mod preferences;
mod status;
mod stems;
mod vinyl;

pub use audio::*;
pub use auto_dj::*;
pub use controllers::*;
pub use deck::*;
pub use fx::*;
pub use library::*;
pub use mixer::*;
pub use preferences::*;
pub use prisme_actions::stems::{Separation as SeparationDto, StemMix as StemMixDto};
pub use status::*;
pub use stems::*;
pub use vinyl::*;

#[cfg(test)]
mod tests {
    use super::deck::signal_light;
    use super::*;
    use crate::state::{lock, AppState, Section};
    use prisme_actions::view::View;
    use prisme_timecode::SignalState;

    #[test]
    fn a_draft_of_the_audio_settings_follows_the_rules_of_its_driver() {
        let asio = AudioSettings {
            backend: BackendDto::Asio,
            output_device: Some("Card".into()),
            input_device: Some("Card".into()),
            buffer_frames: Some(256),
            ..AudioSettings::default()
        };
        let none = || None;
        let other = asio.changed(AudioChangeDto::Driver(Some("Other".into())), none);
        assert_eq!(
            (other.output_device.as_deref(), other.input_device.as_deref()),
            (Some("Other"), Some("Other"))
        );
        let off = asio.changed(AudioChangeDto::TurntablesOnDriver(false), none);
        assert_eq!(off.input_device, None);
        let alone = off.changed(AudioChangeDto::Driver(Some("Other".into())), none);
        assert_eq!(alone.input_device, None, "no turntables: none on the new driver");
        let first = AudioSettings { output_device: None, ..off }
            .changed(AudioChangeDto::TurntablesOnDriver(true), || Some("First".into()));
        assert_eq!(first.input_device.as_deref(), Some("First"));
        let wasapi = asio.changed(AudioChangeDto::Backend(BackendDto::Wasapi), none);
        assert_eq!(wasapi, AudioSettings { backend: BackendDto::Wasapi, ..AudioSettings::default() });
    }

    #[test]
    fn a_decks_signal_light_shows_the_record_driving_it_and_none_with_its_cables_crossed() {
        assert_eq!(signal_light(true, false, SignalState::Ok), Some(SignalDto::Ok));
        assert_eq!(signal_light(true, true, SignalState::Ok), Some(SignalDto::None));
        assert_eq!(signal_light(false, false, SignalState::Ok), None);
    }

    fn view_of(state: &AppState) -> View {
        crate::actions::view_of(state, false, &Default::default(), 0.0)
    }

    #[test]
    fn a_change_the_interface_mirrors_moves_the_revision_and_reaches_it_a_read_does_not() {
        let state = AppState::new(prisme_session::JsonFile::in_memory(Default::default()));
        let before = state.revision();
        let pitch = lock(&state.decks)[1].pitch;
        assert_eq!((pitch, state.moved_since(before)), (1.0, vec![]), "read");
        lock(&state.decks)[1].strip.eq[0] = 0.5;
        assert_eq!(state.moved_since(before), [Section::Decks]);
        let eq = state.revision();
        state.crossfader.set(0.0);
        assert_eq!(state.moved_since(eq), [], "the crossfader where it was");
        state.crossfader.set(-1.0);
        assert_eq!(state.moved_since(eq), [Section::Mixer]);
        state.update_preferences(|p| p.dj.quantize = true);
        assert_eq!(state.moved_since(eq), [Section::Mixer, Section::Decks], "the routing as it was");
        state.update_preferences(|p| p.mixer.booth = Some(4));
        assert_eq!(state.moved_since(eq), [Section::Mixer, Section::Routing, Section::Decks]);
        assert_eq!(
            ControlStateDto::of(&state, &view_of(&state), [Section::Routing]).routing.unwrap().booth,
            Some(4)
        );
        let controls = ControlStateDto::of(&state, &view_of(&state), Section::ALL);
        assert_eq!(controls.decks.unwrap()[1].strip.eq[0], 0.5);
        assert_eq!(controls.mixer.unwrap().knobs.crossfader.at, 0.0);
        assert_eq!(controls.quantize, Some(true));
        assert_eq!(controls.tracks.unwrap().len(), prisme_engine::MAX_DECKS);
    }

    #[test]
    fn a_waveform_is_sent_from_the_first_slice_that_changed() {
        let w = prisme_analysis::Waveform {
            slices_per_second: 150,
            peak: vec![0; 3],
            low: vec![200, 0, 0],
            mid: vec![0, 80, 0],
            high: vec![0, 0, 9],
        };
        let bytes = waveform_bytes(&w, &[1, 2, 3], 10, 1);
        let header: Vec<u32> =
            bytes[..12].chunks(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
        assert_eq!(header, [10, 150, 1]);
        assert_eq!(bytes[12..], [2, 3, 0, 0, 230, 0, 0, 255]);
        assert_eq!(whole_waveform_bytes(&w).len(), 12 + 4 * 3);
    }

    #[test]
    fn the_periods_offered_are_those_the_driver_accepts_with_their_length() {
        let sizes = BufferSizeDto::offered(Some((100, 600)), 48_000);
        assert_eq!(sizes.iter().map(|s| s.frames).collect::<Vec<_>>(), [128, 256, 512]);
        assert!((sizes[0].ms - 2.666).abs() < 0.01);
        assert_eq!(BufferSizeDto::offered(None, 0).len(), 7, "every one, at 48 kHz");
        let kept = AudioSettings::default();
        let draft =
            |output: Option<&str>| AudioSettings { output_device: output.map(String::from), ..kept.clone() };
        let drafted = |output, devices: &[DeviceDto]| AudioDraftDto::of(draft(output), &kept, devices);
        assert_eq!(drafted(None, &[]).buffer_sizes, BufferSizeDto::offered(None, 0), "no device");
        assert!(!drafted(None, &[]).changed);
        let output = |name: &str, is_default| DeviceDto {
            name: name.into(),
            kind: "output",
            channels: 2,
            sample_rate: 44_100,
            buffer_sizes: if is_default { sizes.clone() } else { BufferSizeDto::offered(None, 96_000) },
            is_default,
        };
        let devices = [output("Default", true), output("Card", false)];
        assert_eq!(drafted(None, &devices).buffer_sizes, sizes, "the default output's");
        let chosen = drafted(Some("Card"), &devices);
        assert_eq!((chosen.changed, chosen.buffer_sizes), (true, BufferSizeDto::offered(None, 96_000)));
        assert_eq!(BufferSizeDto::offered(None, 0)[1].ms, 64_000.0 / 48_000.0);
    }

    #[test]
    fn a_change_moves_the_revision_of_its_section_alone() {
        let state = AppState::new(prisme_session::JsonFile::in_memory(Default::default()));
        let before = state.revision();
        lock(&state.perform).decks[0].roll = Some(1.0);
        assert_eq!(state.moved_since(before), [], "a change it does not show");
        lock(&state.perform).decks[0].fx_shown = 1;
        let moved = state.moved_since(before);
        assert_eq!(moved, [Section::Perform]);
        let controls = serde_json::to_value(ControlStateDto::of(&state, &view_of(&state), moved)).unwrap();
        assert_eq!(
            controls.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["libraryOnly", "perform"],
            "the rest left out"
        );
    }

    #[test]
    fn the_rules_of_a_smart_crate_are_checked_as_they_come() {
        let rules =
            |json: serde_json::Value| serde_json::from_value::<SmartRulesDto>(json).unwrap().checked();
        let code = |json| rules(json).map_err(|e| e.to_string());
        let kept = rules(serde_json::json!({ "matchAll": true, "rules": [
            { "kind": "year", "min": 1998, "max": null },
            { "kind": "key", "camelot": ["8a", " 12B"] },
        ] }))
        .unwrap();
        assert_eq!(
            kept.rules,
            [
                prisme_library::Rule::Year { min: Some(1998), max: None },
                prisme_library::Rule::Key { camelot: vec!["8A".into(), "12B".into()] }
            ]
        );
        assert_eq!(
            code(
                serde_json::json!({ "matchAll": true, "rules": [{ "kind": "bpm", "min": 120.5, "max": null }, { "kind": "year", "min": 1998.5, "max": null }] })
            ),
            Err("rule 2: a year is a whole number".into())
        );
        assert_eq!(
            code(
                serde_json::json!({ "matchAll": false, "rules": [{ "kind": "key", "camelot": ["8A", "13C"] }] })
            ),
            Err("rule 1: 13C is not a key of the Camelot wheel".into())
        );
        assert_eq!(
            code(serde_json::json!({ "matchAll": true, "rules": [] })),
            Err("a smart crate needs a rule".into())
        );
    }

    #[test]
    fn the_default_devices_keep_the_simulated_turntable() {
        let chosen = AudioSettings {
            output_device: Some("UMC404HD".into()),
            input_device: Some("UMC404HD".into()),
            simulate: true,
            buffer_frames: Some(128),
            ..AudioSettings::default()
        };
        let fallback = chosen.on_default_devices();
        assert_eq!(
            (fallback.output_device, fallback.input_device, fallback.buffer_frames),
            (None, None, None)
        );
        assert!(fallback.simulate);
        assert!(chosen.same_devices(&AudioSettings { simulate: false, ..chosen.clone() }));
        assert!(!chosen.same_devices(&AudioSettings { output_device: None, ..chosen.clone() }));
    }

    #[test]
    fn a_vinyl_setting_is_changed_by_its_name_but_never_put_on() {
        let lead_in: VinylChangeDto = serde_json::from_str(r#"{"setting":"leadIn","value":null}"#).unwrap();
        assert_eq!(lead_in, VinylChangeDto::LeadIn(None));
        let rpm: VinylChangeDto = serde_json::from_str(r#"{"setting":"rpm","value":"45"}"#).unwrap();
        assert_eq!(rpm, VinylChangeDto::Rpm(RpmDto::Rpm45));
        assert!(serde_json::from_str::<VinylChangeDto>(r#"{"setting":"enabled","value":true}"#).is_err());
    }
}
