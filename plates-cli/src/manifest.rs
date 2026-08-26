//! The record of what a build wrote — so that `clean` can be exact, and a
//! rebuild can take back the page that stopped existing.
//!
//! # Why a build directory needs a memory
//!
//! Two problems, one answer. A rebuild after a document is deleted, renamed or
//! taken off a site leaves the old `.html` sitting in the destination, and it
//! will be deployed alongside the new ones: a page that is no longer part of
//! the site, still reachable, still in the sitemap of whatever indexed it. And
//! `clean` given a directory has no way to tell the site it built from a
//! directory somebody typed one character wrong.
//!
//! So a build writes [`MANIFEST`]: every path it laid down, relative to the
//! destination. The next build removes what the last one wrote and this one did
//! not, and `clean` removes exactly the listed set. **Nothing here ever deletes
//! a file no build of ours recorded writing** — which is the property that makes
//! it safe to point `--out` at a directory that also holds something else, and
//! the reason `clean` refuses a destination with no manifest rather than
//! guessing.
//!
//! # Why it is a dotfile
//!
//! It ships with the site. A build directory *is* the deployable artifact, and
//! the marker has to survive being copied to a host or committed to a branch —
//! a memory kept somewhere else would be gone at exactly the moment the next
//! build needed it. Dot-prefixed because every static host and every web server
//! already declines to serve those, so the record of the site is not part of
//! the site.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// The file a build's record lives in, at the destination root.
pub const MANIFEST: &str = ".plates-build";

/// The first line of the file, so that a person who opens one — or a program
/// checking whether a directory is ours — reads an answer rather than a list.
const HEADER: &str = "# written by plates — every path below is a file this build wrote.";

/// Read the destination's record of what the last build wrote.
///
/// `Ok(None)` means there is no manifest: either nothing has been built here or
/// the directory is not ours, and the two are deliberately the same answer to a
/// caller that must not delete either way.
pub fn read(dest: &Path) -> std::io::Result<Option<BTreeSet<String>>> {
    let text = match std::fs::read_to_string(dest.join(MANIFEST)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some(
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            // A path that climbs out of the destination cannot have been
            // written by a build — nothing here composes one — so it is a
            // hand-edited or corrupted manifest, and the safe reading of a
            // deletion instruction nobody meant to give is to drop it.
            .filter(|line| is_contained(Path::new(line)))
            .map(str::to_string)
            .collect(),
    ))
}

/// Write the destination's record.
pub fn write(dest: &Path, paths: &BTreeSet<String>) -> std::io::Result<()> {
    let mut text = String::from(HEADER);
    text.push('\n');
    for path in paths {
        text.push_str(path);
        text.push('\n');
    }
    std::fs::write(dest.join(MANIFEST), text)
}

/// Remove `paths` from `dest`, then every directory the removals emptied.
///
/// Returns how many files were actually removed — one that had already been
/// deleted by hand is not an error, since the state it was asked to reach is
/// the state it is in.
pub fn remove(dest: &Path, paths: &BTreeSet<String>) -> std::io::Result<usize> {
    let mut removed = 0;
    // Deepest first, so a directory is considered only once everything under it
    // is gone.
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for rel in paths {
        let path = dest.join(rel);
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut parent = path.parent();
        while let Some(dir) = parent {
            if dir == dest {
                break;
            }
            dirs.insert(dir.to_path_buf());
            parent = dir.parent();
        }
    }
    // `remove_dir` on a non-empty directory fails, which is exactly the test
    // wanted: a directory holding something a build did not write stays.
    for dir in dirs.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(removed)
}

/// Whether a manifest line stays inside the destination it was read from.
fn is_contained(rel: &Path) -> bool {
    !rel.is_absolute()
        && rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Whether a destination is safe for a build to write into.
///
/// `Ok(())` for a destination that does not exist, is empty, or carries a
/// manifest; an explanation otherwise. A directory holding somebody else's
/// files is not refused because writing into it would fail — it is refused
/// because the *next* build would prune from it, and the pruning is only
/// bounded by the manifest that directory does not have.
pub fn check_destination(dest: &Path) -> Result<(), String> {
    if !dest.exists() {
        return Ok(());
    }
    if !dest.is_dir() {
        return Err(format!("{} is not a directory", dest.display()));
    }
    if dest.join(MANIFEST).exists() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    if entries.next().is_none() {
        return Ok(());
    }
    Err(format!(
        "{} already holds files, and no build of ours wrote them",
        dest.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    /// The round trip, and the shape a person opening the file sees: a comment
    /// line, then one path per line.
    #[test]
    fn a_manifest_round_trips_and_explains_itself() {
        let dir = tempfile::tempdir().unwrap();
        let paths = set(&["index.html", "notes/post.html"]);
        write(dir.path(), &paths).unwrap();

        let text = std::fs::read_to_string(dir.path().join(MANIFEST)).unwrap();
        assert!(text.starts_with('#'), "{text}");
        assert_eq!(read(dir.path()).unwrap(), Some(paths));
    }

    /// No manifest and a manifest listing nothing are different states, and a
    /// caller deciding whether to delete has to be able to tell them apart.
    #[test]
    fn a_destination_with_no_manifest_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()).unwrap(), None);

        write(dir.path(), &BTreeSet::new()).unwrap();
        assert_eq!(read(dir.path()).unwrap(), Some(BTreeSet::new()));
    }

    /// Removal takes the files and the directories they emptied — and stops at
    /// a directory still holding something nobody here wrote.
    #[test]
    fn removal_prunes_emptied_directories_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("notes/post.html"));
        touch(&dir.path().join("img/logo.png"));
        touch(&dir.path().join("img/theirs.txt"));

        let removed = remove(dir.path(), &set(&["notes/post.html", "img/logo.png"])).unwrap();

        assert_eq!(removed, 2);
        assert!(!dir.path().join("notes").exists(), "emptied, so pruned");
        assert!(
            dir.path().join("img/theirs.txt").exists(),
            "a file no build wrote is untouched, and its directory with it"
        );
    }

    /// A file already gone is the state the caller asked for, not a failure.
    #[test]
    fn removing_what_is_already_gone_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(remove(dir.path(), &set(&["index.html"])).unwrap(), 0);
    }

    /// A hand-edited manifest is still a deletion instruction. One that climbs
    /// out of the destination is dropped rather than obeyed.
    #[test]
    fn a_manifest_line_that_escapes_the_destination_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MANIFEST),
            "# header\nindex.html\n../outside.html\n/etc/passwd\n",
        )
        .unwrap();

        assert_eq!(read(dir.path()).unwrap(), Some(set(&["index.html"])));
    }

    /// The three destinations a build may write into, and the one it may not.
    #[test]
    fn a_destination_is_ours_when_it_is_empty_or_carries_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_destination(&dir.path().join("missing")).is_ok());
        assert!(check_destination(dir.path()).is_ok(), "empty");

        touch(&dir.path().join("theirs.txt"));
        assert!(check_destination(dir.path()).is_err());

        write(dir.path(), &BTreeSet::new()).unwrap();
        assert!(
            check_destination(dir.path()).is_ok(),
            "a manifest bounds what the next build would prune, which is what makes it safe"
        );
    }
}
