use crate::Error;
use std::path::Path;

/// Unpacks a package (`.tar.gz`, as the release makes them) into `folder`, made if need be.
pub fn unpack(bytes: &[u8], folder: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(folder)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);
    archive.unpack(folder)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A package holding `files` (path, contents), as the release makes them.
    pub(crate) fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut archive = tar::Builder::new(gz);
        for (path, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive.append_data(&mut header, path, *bytes).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn a_package_unpacks_into_its_folder_whole() {
        let folder = tempfile::tempdir().unwrap();
        let package = pack(&[("Prisme.app/Contents/MacOS/prisme-desktop", b"new"), ("a.txt", b"a")]);
        unpack(&package, &folder.path().join("in")).unwrap();
        let read = |p: &str| std::fs::read(folder.path().join("in").join(p)).unwrap();
        assert_eq!(read("Prisme.app/Contents/MacOS/prisme-desktop"), b"new");
        assert_eq!(read("a.txt"), b"a");
        assert!(unpack(b"not a package", &folder.path().join("bad")).is_err());
    }
}
