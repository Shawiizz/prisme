//! The page's types of what the backend sends it and takes from it, generated from those of
//! Rust: written with `PRISME_WRITE_TYPES=1`, else compared with what the page has.

use crate::actions::edits::GridChange;
use crate::commands::export::ExportListDto;
use crate::commands::folders::DroppedDto;
use crate::commands::new_version::NewVersionDto;
use crate::commands::pads::PadPageDto;
use crate::commands::preferences::Setting;
use crate::commands::screens::SpanDto;
use crate::controllers::PluggedIn;
use crate::dto::*;
use crate::error::ErrorDto;
use crate::preferences::KeyPreference;
use prisme_actions::screen::{Drive, Reading};
use prisme_actions::switch::LiveSource;
use prisme_control::{Band, Encoding};
use std::any::TypeId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::{Config, TypeVisitor, TS};

/// The page's file of them.
const TYPES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../frontend/src/app/core/models/backend.ts");

/// The declarations of the types visited and of every type they use, by name, as ts-rs writes
/// and formats them in `folder`, the imports of the others left out: they share one file.
struct Declarations {
    cfg: Config,
    folder: PathBuf,
    declared: BTreeMap<String, (TypeId, String)>,
}

impl Declarations {
    fn new(folder: &Path) -> Self {
        let cfg = Config::new().with_large_int("number").with_out_dir(folder);
        Self { cfg, folder: folder.to_owned(), declared: BTreeMap::new() }
    }

    fn of<T: TS + 'static>(&mut self) -> &mut Self {
        self.visit::<T>();
        self
    }

    fn text(&self) -> String {
        let header = "// What the backend sends the page and takes from it, as it declares it: written by\n\
                      // its test (ts_types, PRISME_WRITE_TYPES=1), never by hand.\n\n";
        let declarations: Vec<&str> = self.declared.values().map(|(_, text)| text.as_str()).collect();
        format!("{header}{}\n", declarations.join("\n\n"))
    }
}

impl TypeVisitor for Declarations {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if let Some(path) = T::output_path() {
            let name = T::ident(&self.cfg);
            if let Some((id, _)) = self.declared.get(&name) {
                assert!(*id == TypeId::of::<T>(), "two types are named {name}");
                return;
            }
            T::export(&self.cfg).unwrap();
            let file = std::fs::read_to_string(self.folder.join(path)).unwrap();
            let own =
                file.lines().filter(|line| !line.starts_with("import ") && !line.starts_with("// This file"));
            let text = own.collect::<Vec<_>>().join("\n").trim().to_owned();
            self.declared.insert(name, (TypeId::of::<T>(), text));
        }
        T::visit_dependencies(self);
        T::visit_generics(self);
    }
}

#[test]
fn the_page_has_the_types_the_backend_declares() {
    let folder = std::env::temp_dir().join(format!("prisme-types-{}", std::process::id()));
    let mut types = Declarations::new(&folder);
    types
        .of::<StatusDto>()
        .of::<ControlStateDto>()
        .of::<ErrorDto>()
        // what the commands take and answer
        .of::<AudioDraftDto>()
        .of::<AudioChangeDto>()
        .of::<BackendChoiceDto>()
        .of::<DeviceDto>()
        .of::<LatencyChannelsDto>()
        .of::<LatencyDto>()
        .of::<TimecodeMakerDto>()
        .of::<Encoding>()
        .of::<Band>()
        .of::<VinylChangeDto>()
        .of::<Drive>()
        .of::<Reading>()
        .of::<GridChange>()
        .of::<ExportListDto>()
        .of::<ExportSummaryDto>()
        .of::<ImportSummaryDto>()
        .of::<DroppedDto>()
        .of::<StoragePlaceDto>()
        .of::<DirListingDto>()
        .of::<PickDto>()
        .of::<LibraryMenuDto>()
        .of::<SeparateDto>()
        .of::<LiveSource>()
        .of::<NewVersionDto>()
        .of::<Setting>()
        .of::<KeyPreference>()
        .of::<TakenKeyDto>()
        .of::<SpanDto>()
        .of::<TargetDto>()
        .of::<PadPageDto>()
        // what the events tell
        .of::<CardOffer>()
        .of::<OpenedDto>()
        .of::<PluggedIn>()
        .of::<UiActionDto>()
        .of::<ProgressDto>();
    let text = types.text();
    std::fs::remove_dir_all(&folder).unwrap();
    if std::env::var_os("PRISME_WRITE_TYPES").is_some() {
        std::fs::write(TYPES, &text).unwrap();
    }
    let written = std::fs::read_to_string(TYPES).unwrap_or_default().replace("\r\n", "\n");
    assert!(written == text, "backend.ts is behind: run the test again with PRISME_WRITE_TYPES=1");
}
