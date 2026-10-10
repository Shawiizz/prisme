//! Waveforms of the tracks that were played, so that loading one again shows its
//! waveform at once. Kept for one version of the file ([`FileStamp`]).

use super::{path_key, DbError, FileStamp, LibraryDb};
use prisme_analysis::Waveform;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

impl LibraryDb {
    pub fn save_waveform(&self, path: &Path, stamp: FileStamp, waveform: &Waveform) -> Result<(), DbError> {
        let data: Vec<u8> = [&waveform.peak, &waveform.low, &waveform.mid, &waveform.high]
            .into_iter()
            .flat_map(|band| band.iter().copied())
            .collect();
        self.conn()
            .prepare_cached(
                "INSERT OR REPLACE INTO waveforms (path, size, modified_ms, slices_per_second, data)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                path_key(path),
                stamp.size as i64,
                stamp.modified_ms,
                waveform.slices_per_second,
                data
            ])?;
        Ok(())
    }

    /// The stored waveform of this version of the file, if any.
    pub fn waveform(&self, path: &Path, stamp: FileStamp) -> Result<Option<Waveform>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT slices_per_second, data FROM waveforms WHERE path = ?1 AND size = ?2 AND modified_ms = ?3",
        )?;
        let row = stmt
            .query_row(params![path_key(path), stamp.size as i64, stamp.modified_ms], |r| {
                Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .optional()?;
        Ok(row.filter(|(_, data)| data.len() % 4 == 0).map(|(slices_per_second, data)| {
            let mut bands = data.chunks_exact(data.len() / 4).map(<[u8]>::to_vec);
            let mut band = || bands.next().unwrap_or_default();
            Waveform { slices_per_second, peak: band(), low: band(), mid: band(), high: band() }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveforms_are_kept_for_one_version_of_the_file() {
        let db = LibraryDb::open_in_memory().unwrap();
        let path = Path::new("a.mp3");
        let stamp = FileStamp { size: 10, modified_ms: 20 };
        let waveform = Waveform {
            slices_per_second: 150,
            peak: vec![1, 2, 3],
            low: vec![4, 5, 6],
            mid: vec![7, 8, 9],
            high: vec![10, 11, 12],
        };
        db.save_waveform(path, stamp, &waveform).unwrap();
        assert_eq!(db.waveform(path, stamp).unwrap(), Some(waveform));
        assert_eq!(db.waveform(path, FileStamp { size: 10, modified_ms: 21 }).unwrap(), None);
    }
}
