use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use super::{CasError, CasStore, Digest, Result};

/// Filesystem-backed content-addressed store with git-style 2-char sharding.
///
/// Layout: `<root>/objects/<first 2 hex chars>/<remaining hex chars>`
pub struct FsCasStore {
    objects_dir: PathBuf,
}

impl FsCasStore {
    /// Create a new `FsCasStore` rooted at `root`. Creates `root/objects/` if needed.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let objects_dir = root.as_ref().join("objects");
        fs::create_dir_all(&objects_dir)?;
        Ok(Self { objects_dir })
    }

    fn blob_path(&self, digest: &Digest) -> PathBuf {
        let hex = digest.to_hex();
        self.objects_dir.join(&hex[..2]).join(&hex[2..])
    }
}

impl CasStore for FsCasStore {
    fn put(&self, data: &[u8]) -> Result<Digest> {
        let digest = Digest::compute(data);
        let path = self.blob_path(&digest);

        if path.exists() {
            return Ok(digest);
        }

        let shard_dir = path.parent().expect("blob path always has parent");
        fs::create_dir_all(shard_dir)?;

        // Atomic write: write to temp file in the same directory, then rename.
        let mut tmp = NamedTempFile::new_in(shard_dir)?;
        tmp.write_all(data)?;
        tmp.persist(&path).map_err(|e| e.error)?;

        Ok(digest)
    }

    fn get(&self, digest: &Digest) -> Result<Vec<u8>> {
        let path = self.blob_path(digest);
        fs::read(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CasError::NotFound(*digest)
            } else {
                CasError::Io(e)
            }
        })
    }

    fn exists(&self, digest: &Digest) -> Result<bool> {
        let path = self.blob_path(digest);
        Ok(path.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> (tempfile::TempDir, FsCasStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsCasStore::new(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn blob_roundtrip() {
        let (_dir, store) = make_store();
        let data = b"hello world";
        let digest = store.put(data).unwrap();
        let got = store.get(&digest).unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn dedupe_invariant() {
        let (dir, store) = make_store();
        let data = b"duplicate me";
        let d1 = store.put(data).unwrap();
        let d2 = store.put(data).unwrap();
        assert_eq!(d1, d2);

        // Verify single file on disk.
        let hex = d1.to_hex();
        let shard = dir.path().join("objects").join(&hex[..2]);
        let entries: Vec<_> = std::fs::read_dir(shard).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn empty_blob() {
        let (_dir, store) = make_store();
        let digest = store.put(b"").unwrap();
        let got = store.get(&digest).unwrap();
        assert_eq!(got, b"");
    }

    #[test]
    fn large_blob() {
        let (_dir, store) = make_store();
        let data = vec![0xABu8; 1_100_000]; // ~1.1 MB
        let digest = store.put(&data).unwrap();
        let got = store.get(&digest).unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn get_nonexistent_returns_not_found() {
        let (_dir, store) = make_store();
        let fake = Digest::compute(b"no such blob");
        match store.get(&fake) {
            Err(CasError::NotFound(d)) => assert_eq!(d, fake),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn exists_after_put() {
        let (_dir, store) = make_store();
        let digest = store.put(b"exists check").unwrap();
        assert!(store.exists(&digest).unwrap());
    }

    #[test]
    fn exists_false_for_missing() {
        let (_dir, store) = make_store();
        let fake = Digest::compute(b"missing");
        assert!(!store.exists(&fake).unwrap());
    }

    #[test]
    fn random_bytes_roundtrip() {
        let (_dir, store) = make_store();
        // Deterministic pseudo-random via simple LCG to avoid adding rand dep.
        let mut state: u64 = 0xDEAD_BEEF;
        let mut data = vec![0u8; 4096];
        for byte in &mut data {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *byte = (state >> 33) as u8;
        }
        let digest = store.put(&data).unwrap();
        let got = store.get(&digest).unwrap();
        assert_eq!(got, data);
    }
}
