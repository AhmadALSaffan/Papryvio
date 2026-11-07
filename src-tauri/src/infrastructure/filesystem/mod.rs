//! Managed file storage inside the app data directory:
//! `library/` (imported PDFs), `staging/` (in-flight copies), `cache/`, `exports/`, `logs/`.
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::ports::{FileStore, SourceInfo};
use crate::domain::ids::DocumentId;

pub struct AppPaths {
    pub root: PathBuf,
    pub library: PathBuf,
    pub staging: PathBuf,
    pub cache: PathBuf,
    pub exports: PathBuf,
    pub logs: PathBuf,
    pub database: PathBuf,
}

impl AppPaths {
    pub fn create(root: PathBuf) -> std::io::Result<Self> {
        let p = Self {
            library: root.join("library"),
            staging: root.join("staging"),
            cache: root.join("cache"),
            exports: root.join("exports"),
            logs: root.join("logs"),
            database: root.join("papryvio.db"),
            root,
        };
        for d in [&p.root, &p.library, &p.staging, &p.cache, &p.exports, &p.logs] {
            fs::create_dir_all(d)?;
        }
        Ok(p)
    }
}

pub struct LocalFileStore {
    paths: AppPaths,
    library_canonical: PathBuf,
}

impl LocalFileStore {
    pub fn new(paths: AppPaths) -> std::io::Result<Self> {
        let library_canonical = fs::canonicalize(&paths.library)?;
        Ok(Self { paths, library_canonical })
    }

    fn staged_path(&self, id: &DocumentId) -> PathBuf {
        self.paths.staging.join(format!("{}.pdf", id.as_str()))
    }

    fn library_path(&self, id: &DocumentId) -> PathBuf {
        self.paths.library.join(format!("{}.pdf", id.as_str()))
    }
}

fn io_err(context: &str, e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::StorageFull {
        return AppError::storage(format!("{context}: disk full"));
    }
    AppError::file_access(format!("{context}: {e}"))
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|it| it.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

impl FileStore for LocalFileStore {
    fn inspect_source(&self, source: &Path, max_size: u64) -> AppResult<SourceInfo> {
        let meta = fs::metadata(source).map_err(|e| io_err("stat source", e))?;
        if !meta.is_file() {
            return Err(AppError::file_access("not a regular file"));
        }
        let file_name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if meta.len() > max_size {
            return Err(AppError::new(ErrorCode::FileTooLarge, format!("{} bytes", meta.len())));
        }
        let file = File::open(source).map_err(|e| io_err("open source", e))?;
        let mut reader = BufReader::with_capacity(256 * 1024, file);
        let mut hasher = Sha256::new();
        let mut head: Vec<u8> = Vec::with_capacity(1024);
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| io_err("read source", e))?;
            if n == 0 {
                break;
            }
            if head.len() < 1024 {
                let take = (1024 - head.len()).min(n);
                head.extend_from_slice(&buf[..take]);
            }
            hasher.update(&buf[..n]);
        }
        // The PDF header may be preceded by a few junk bytes; spec allows it within the first 1024.
        let looks_like_pdf = head.windows(5).any(|w| w == b"%PDF-");
        Ok(SourceInfo { file_name, size: meta.len(), sha256: hex::encode(hasher.finalize()), looks_like_pdf })
    }

    fn stage_copy(&self, source: &Path, id: &DocumentId) -> AppResult<()> {
        let dest = self.staged_path(id);
        let mut input = File::open(source).map_err(|e| io_err("open source", e))?;
        let mut out = File::create(&dest).map_err(|e| io_err("create staging", e))?;
        let res = std::io::copy(&mut input, &mut out).and_then(|_| out.flush()).and_then(|_| out.sync_all());
        if let Err(e) = res {
            drop(out);
            let _ = fs::remove_file(&dest);
            return Err(io_err("copy to staging", e));
        }
        Ok(())
    }

    fn promote(&self, id: &DocumentId) -> AppResult<String> {
        let from = self.staged_path(id);
        let to = self.library_path(id);
        fs::rename(&from, &to).map_err(|e| io_err("promote", e))?;
        Ok(self.managed_rel_path(id))
    }

    fn managed_rel_path(&self, id: &DocumentId) -> String {
        format!("library/{}.pdf", id.as_str())
    }

    fn resolve_managed(&self, rel: &str) -> AppResult<PathBuf> {
        let name = rel
            .strip_prefix("library/")
            .filter(|n| !n.is_empty() && !n.contains(['/', '\\']) && !n.contains("..") && n.ends_with(".pdf"))
            .ok_or_else(|| AppError::not_allowed("path outside library"))?;
        let candidate = self.paths.library.join(name);
        // If the file exists, also verify the canonical path (symlinks, junctions).
        if let Ok(canon) = fs::canonicalize(&candidate) {
            if !canon.starts_with(&self.library_canonical) {
                return Err(AppError::not_allowed("path outside library"));
            }
            return Ok(canon);
        }
        Ok(candidate)
    }

    fn managed_exists(&self, id: &DocumentId) -> bool {
        self.library_path(id).is_file()
    }

    fn staged_exists(&self, id: &DocumentId) -> bool {
        self.staged_path(id).is_file()
    }

    fn remove_managed(&self, id: &DocumentId) -> AppResult<()> {
        let p = self.library_path(id);
        match fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err("remove managed file", e).retryable()),
        }
    }

    fn remove_staged(&self, id: &DocumentId) {
        let _ = fs::remove_file(self.staged_path(id));
    }

    fn cleanup_staging(&self, known_ids: &HashSet<String>) -> AppResult<u32> {
        let mut removed = 0;
        let entries = fs::read_dir(&self.paths.staging).map_err(|e| io_err("read staging", e))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".pdf").unwrap_or(&name);
            if !known_ids.contains(stem) && entry.path().is_file() && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn data_dir(&self) -> PathBuf {
        self.paths.root.clone()
    }

    fn library_bytes(&self) -> u64 {
        dir_size(&self.paths.library)
    }

    fn database_bytes(&self) -> u64 {
        ["", "-wal", "-shm"]
            .iter()
            .filter_map(|s| fs::metadata(format!("{}{}", self.paths.database.display(), s)).ok())
            .map(|m| m.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, LocalFileStore) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::create(dir.path().join("data")).unwrap();
        (dir, LocalFileStore::new(paths).unwrap())
    }

    #[test]
    fn resolve_rejects_traversal() {
        let (_d, s) = store();
        assert!(s.resolve_managed("library/../papryvio.db").is_err());
        assert!(s.resolve_managed("../x.pdf").is_err());
        assert!(s.resolve_managed("library/a/b.pdf").is_err());
        assert!(s.resolve_managed("library/ok.pdf").is_ok());
    }

    #[test]
    fn stage_promote_and_cleanup() {
        let (d, s) = store();
        let src = d.path().join("مقالة نموذجية.pdf");
        fs::write(&src, b"%PDF-1.7\nhello").unwrap();
        let info = s.inspect_source(&src, 1024).unwrap();
        assert!(info.looks_like_pdf);
        assert_eq!(info.size, 14);

        let id = DocumentId::new();
        s.stage_copy(&src, &id).unwrap();
        assert!(s.staged_exists(&id));
        s.promote(&id).unwrap();
        assert!(s.managed_exists(&id) && !s.staged_exists(&id));
        // source untouched
        assert_eq!(fs::read(&src).unwrap(), b"%PDF-1.7\nhello");

        let orphan = DocumentId::new();
        s.stage_copy(&src, &orphan).unwrap();
        assert_eq!(s.cleanup_staging(&HashSet::new()).unwrap(), 1);
        s.remove_managed(&id).unwrap();
        assert!(!s.managed_exists(&id));
    }

    #[test]
    fn detects_non_pdf_and_size_limit() {
        let (d, s) = store();
        let src = d.path().join("notes.pdf");
        fs::write(&src, b"just text").unwrap();
        assert!(!s.inspect_source(&src, 1024).unwrap().looks_like_pdf);
        assert_eq!(s.inspect_source(&src, 3).unwrap_err().code, ErrorCode::FileTooLarge);
    }
}
