/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::logging::Logger;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Mbox,
    Ics,
    Vcf,
}

impl FileKind {
    fn from_extension(ext: &str) -> Option<FileKind> {
        let lower = ext.to_ascii_lowercase();
        match lower.as_str() {
            "mbox" => Some(FileKind::Mbox),
            "ics" => Some(FileKind::Ics),
            "vcf" => Some(FileKind::Vcf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub kind: FileKind,
}

#[derive(Debug, Default)]
pub struct WalkResult {
    pub files: Vec<DiscoveredFile>,
    pub symlink_cycles: u64,
    pub io_failures: u64,
}

impl WalkResult {
    pub fn by_kind(&self, kind: FileKind) -> impl Iterator<Item = &DiscoveredFile> {
        self.files.iter().filter(move |f| f.kind == kind)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

pub fn walk(root: &Path) -> io::Result<WalkResult> {
    walk_with_logger(root, Logger::from_flags(true, 0))
}

pub fn walk_with_logger(root: &Path, logger: Logger) -> io::Result<WalkResult> {
    let mut result = WalkResult::default();
    let mut visited: HashSet<(u64, u64)> = HashSet::new();
    walk_dir(root, &mut result, &mut visited, logger)?;
    result.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn walk_dir(
    dir: &Path,
    result: &mut WalkResult,
    visited: &mut HashSet<(u64, u64)>,
    logger: Logger,
) -> io::Result<()> {
    let metadata = fs::metadata(dir)?;
    let key = (metadata.dev(), metadata.ino());
    if !visited.insert(key) {
        result.symlink_cycles += 1;
        logger.warn(&format!(
            "takeout walk: symlink cycle broken at {dir:?} (target already visited)"
        ));
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) => {
            result.io_failures += 1;
            logger.warn(&format!("takeout walk: read_dir {dir:?}: {e}"));
            return Ok(());
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                result.io_failures += 1;
                logger.warn(&format!("takeout walk: entry in {dir:?}: {e}"));
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(e) => {
                result.io_failures += 1;
                logger.warn(&format!("takeout walk: file_type {path:?}: {e}"));
                continue;
            }
        };
        if file_type.is_dir() {
            walk_dir(&path, result, visited, logger)?;
            continue;
        }
        if file_type.is_symlink() {
            let target_meta = match fs::metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    result.io_failures += 1;
                    logger.warn(&format!("takeout walk: symlink target {path:?}: {e}"));
                    continue;
                }
            };
            if target_meta.is_dir() {
                walk_dir(&path, result, visited, logger)?;
                continue;
            }
            if !target_meta.is_file() {
                continue;
            }
        } else if !file_type.is_file() {
            continue;
        }
        if let Some(kind) = classify(&path) {
            result.files.push(DiscoveredFile { path, kind });
        }
    }
    Ok(())
}

fn classify(path: &Path) -> Option<FileKind> {
    let ext = path.extension()?.to_str()?;
    FileKind::from_extension(ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn collects_only_matching_extensions() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join("Mail/All.mbox"));
        touch(&root.join("Calendar/cal.ics"));
        touch(&root.join("Contacts/c.vcf"));
        touch(&root.join("Calendar/meet_settings.json"));
        touch(&root.join("archive_browser.html"));
        touch(&root.join("Contacts/photo.jpg"));
        let r = walk(root).unwrap();
        assert_eq!(r.files.len(), 3);
        let mut kinds: Vec<FileKind> = r.files.iter().map(|f| f.kind).collect();
        kinds.sort_by_key(|k| format!("{k:?}"));
        assert_eq!(kinds, vec![FileKind::Ics, FileKind::Mbox, FileKind::Vcf]);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join("Inbox.MBOX"));
        touch(&root.join("Cal.Ics"));
        touch(&root.join("All.Vcf"));
        let r = walk(root).unwrap();
        assert_eq!(r.files.len(), 3);
    }

    #[test]
    fn recurses_into_nested_directories() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join("Takeout/Mail/a.mbox"));
        touch(&root.join("Takeout/Calendar/deep/b.ics"));
        touch(&root.join("Takeout/Contacts/All Contacts/c.vcf"));
        let r = walk(root).unwrap();
        assert_eq!(r.files.len(), 3);
    }

    #[test]
    fn empty_or_non_matching_tree_returns_empty() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join("readme.txt"));
        touch(&root.join("nested/photo.jpg"));
        let r = walk(root).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn results_are_sorted_for_determinism() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join("z.mbox"));
        touch(&root.join("a.mbox"));
        touch(&root.join("m.ics"));
        let r = walk(root).unwrap();
        let names: Vec<&str> = r
            .files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.mbox", "m.ics", "z.mbox"]);
    }

    #[test]
    fn by_kind_filters_returned_iterator() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join("a.mbox"));
        touch(&root.join("b.mbox"));
        touch(&root.join("c.ics"));
        let r = walk(root).unwrap();
        assert_eq!(r.by_kind(FileKind::Mbox).count(), 2);
        assert_eq!(r.by_kind(FileKind::Ics).count(), 1);
        assert_eq!(r.by_kind(FileKind::Vcf).count(), 0);
    }

    #[test]
    fn symlink_to_directory_is_followed_once_not_infinitely() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join("real/a.mbox"));
        symlink(root.join("real"), root.join("link-to-real")).unwrap();
        let r = walk(root).unwrap();
        assert_eq!(r.files.len(), 1);
    }

    #[test]
    fn symlink_cycle_is_broken() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        symlink(root.join("a"), root.join("a/b/loop")).unwrap();
        let r = walk(root).unwrap();
        assert!(r.symlink_cycles >= 1, "cycle counter incremented");
    }

    #[test]
    fn dotted_directory_basenames_traverse_fine() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        touch(&root.join(".hidden/m.mbox"));
        let r = walk(root).unwrap();
        assert_eq!(r.files.len(), 1);
    }
}
