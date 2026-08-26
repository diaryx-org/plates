//! Noticing that the archive changed.
//!
//! A stat walk, compared against the last one. No filesystem-watch dependency:
//! the platform APIs disagree about what an event is, need a fallback poll
//! anyway for network and synced volumes — which is what an archive kept in a
//! sync folder lives on — and the whole of what is wanted here is one bit.
//!
//! [`watch`](crate::commands::watch) and [`serve`](crate::serve) share this so a rebuild
//! means the same thing to both of them.

use std::path::{Path, PathBuf};

/// A number that changes when the archive does: every visible file's path, size
/// and modification time, folded together.
///
/// Order-independent — the per-file hashes are summed — because directory order
/// is not, and a fingerprint that moved when the filesystem felt like
/// reordering a directory would rebuild for nothing.
///
/// Dot-prefixed names are skipped, which is what leaves out prov's own
/// bookkeeping: a registry write during a read would otherwise look like a
/// change to the archive's content. It also leaves out a build directory named
/// by convention (`_site` is not dot-prefixed, so `skip` covers that
/// separately). Symlinks are read as links, not followed, so a cycle cannot
/// hang the walk.
pub fn fingerprint(root: &Path, skip: Option<&Path>) -> u64 {
    let mut total: u64 = 0;
    let mut stack = vec![root.to_path_buf()];
    let skip = skip.and_then(|p| std::fs::canonicalize(p).ok());

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if is_skipped(&entry.path(), skip.as_deref()) {
                    continue;
                }
                stack.push(entry.path());
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            let mut hash = fnv(FNV_OFFSET, entry.path().to_string_lossy().as_bytes());
            hash = fnv(hash, &meta.len().to_le_bytes());
            hash = fnv(hash, &mtime.to_le_bytes());
            total = total.wrapping_add(hash);
        }
    }
    total
}

/// Whether this directory is the one the build writes into.
///
/// Without it a `watch` writing inside the archive it is watching would notice
/// its own output, rebuild, notice that, and never stop.
fn is_skipped(dir: &Path, skip: Option<&Path>) -> bool {
    let Some(skip) = skip else { return false };
    std::fs::canonicalize(dir).is_ok_and(|dir| dir == skip)
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// The path a fingerprint should skip, given where a build lands — `None` when
/// the destination is outside the archive and could not be noticed anyway.
pub fn build_dir_within(root: &Path, dest: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(root).ok()?;
    let dest = std::fs::canonicalize(dest).ok()?;
    dest.starts_with(&root).then_some(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one property the whole loop rests on: an edit moves the number, and
    /// nothing else does.
    #[test]
    fn the_fingerprint_moves_only_when_a_file_does() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "one").unwrap();

        let before = fingerprint(dir.path(), None);
        assert_eq!(
            before,
            fingerprint(dir.path(), None),
            "stable when nothing moves"
        );

        std::fs::write(dir.path().join("a.md"), "one and a half").unwrap();
        assert_ne!(before, fingerprint(dir.path(), None), "an edit is seen");
    }

    /// prov's registry is written during reads. Noticing it would make every
    /// build cause the next one.
    #[test]
    fn dot_prefixed_bookkeeping_is_not_a_change() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "one").unwrap();
        let before = fingerprint(dir.path(), None);

        std::fs::create_dir(dir.path().join(".prov")).unwrap();
        std::fs::write(dir.path().join(".prov/cache"), "x").unwrap();

        assert_eq!(before, fingerprint(dir.path(), None));
    }

    /// A build directory inside the archive is the watcher's own output. Seeing
    /// it would be a loop with no exit.
    #[test]
    fn the_build_directory_is_not_part_of_the_archive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "one").unwrap();
        let out = dir.path().join("_site");
        std::fs::create_dir(&out).unwrap();
        let before = fingerprint(dir.path(), Some(&out));

        std::fs::write(out.join("index.html"), "<html>").unwrap();

        assert_eq!(before, fingerprint(dir.path(), Some(&out)));
        assert_ne!(
            before,
            fingerprint(dir.path(), None),
            "and it would have been seen without the skip"
        );
    }
}
