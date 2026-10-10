use super::{path_key, DbError, LibraryDb};
use crate::cues::{Cue, CueSlot};
use rusqlite::{params, Row};
use std::path::Path;

impl LibraryDb {
    pub fn cues(&self, path: &Path) -> Result<Vec<Cue>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT slot, position_s, loop_length_s, color, label FROM cues WHERE path = ?1 ORDER BY slot",
        )?;
        let rows = stmt.query_map([path_key(path)], cue_from_row)?;
        Ok(rows.filter_map(Result::transpose).collect::<Result<_, _>>()?)
    }

    pub fn save_cue(&self, path: &Path, cue: &Cue) -> Result<(), DbError> {
        self.conn()
            .prepare_cached(
                "INSERT OR REPLACE INTO cues (path, slot, position_s, loop_length_s, color, label)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                path_key(path),
                cue.slot.to_db(),
                cue.position_s,
                cue.loop_length_s,
                cue.color,
                cue.label
            ])?;
        Ok(())
    }

    pub fn delete_cue(&self, path: &Path, slot: CueSlot) -> Result<(), DbError> {
        self.conn()
            .prepare_cached("DELETE FROM cues WHERE path = ?1 AND slot = ?2")?
            .execute(params![path_key(path), slot.to_db()])?;
        Ok(())
    }
}

/// `None` for a slot this version does not know.
fn cue_from_row(r: &Row<'_>) -> rusqlite::Result<Option<Cue>> {
    let Some(slot) = CueSlot::from_db(r.get(0)?) else { return Ok(None) };
    Ok(Some(Cue {
        slot,
        position_s: r.get(1)?,
        loop_length_s: r.get(2)?,
        color: r.get(3)?,
        label: r.get(4)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_stored_per_track_and_slot() {
        let db = LibraryDb::open_in_memory().unwrap();
        let track = Path::new("C:/Music/a.mp3");
        let main = Cue::point(CueSlot::Main, 0.5);
        let hot = Cue { label: "drop".into(), ..Cue::looped(CueSlot::Hot(2), 64.0, 2.0) };
        db.save_cue(track, &hot).unwrap();
        db.save_cue(track, &main).unwrap();
        db.save_cue(Path::new("C:/Music/b.mp3"), &Cue::point(CueSlot::Hot(0), 1.0)).unwrap();
        assert_eq!(db.cues(track).unwrap(), vec![main, hot]);

        let moved = Cue::point(CueSlot::Hot(2), 70.0);
        db.save_cue(track, &moved).unwrap();
        db.delete_cue(track, CueSlot::Main).unwrap();
        assert_eq!(db.cues(track).unwrap(), vec![moved]);
    }
}
