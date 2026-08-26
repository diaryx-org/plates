//! What a file hashed to last time — the port that lets a caller decide an
//! attachment is unchanged without reading it.
//!
//! # Why the diff needs this at all
//!
//! A publish is a content diff: for each object, is the SHA-256 of what we
//! would send equal to the `content_hash` the namespace reports? For a markdown
//! source the bytes are *derived* (body filtered for the audience, frontmatter
//! stripped and re-stamped), so they have to be built before they can be
//! hashed. For an **attachment** they are not derived at all — the uploaded
//! bytes are the file's bytes — which means the only reason to read a 40 MB
//! video during a preview is to compute a number we already computed the last
//! time we looked at that same, unchanged file.
//!
//! On a real archive that is the whole cost of the preview: a few megabytes of
//! markdown against hundreds of megabytes of attachments, read and hashed on
//! every rebuild, and discarded the moment the diff finds
//! them unchanged — which, for an attachment, they almost always are.
//!
//! # Why a port rather than a cache
//!
//! The memory has to be device-local (it describes *this* device's disk, and a
//! copy in the vault would sync-conflict over a file whose whole purpose is to
//! not be shared) and it has to be persistent (an in-process one would help the
//! second preview of a session and no others). An application will often
//! already have exactly such a thing — an index cache validated against the
//! same stat, written to the host's application-support directory — and this
//! crate cannot see it, nor should it: nothing here knows what a host
//! application-support directory is.
//!
//! So collection asks an interface, and the layer that owns a disk answers.
//! Callers with nowhere to keep an answer pass [`NoDigests`] and pay what they
//! paid before.
//!
//! # Staleness
//!
//! An answer is served only when the file's length *and* modification time both
//! still match what was recorded — the same test prov's `FixityCache`
//! applies to payload digests, and the same one `IndexCache` already applies to
//! document *text*, which is a strictly larger thing to be wrong about.
//!
//! Being wrong here has a bounded, one-directional cost: a stale digest that
//! matches the server's makes the diff call an attachment unchanged and skip
//! it, so a file edited without its stat moving would not re-upload until it
//! was touched again. It cannot corrupt an upload — every byte that is *sent*
//! is read fresh at materialize time, never
//! recalled — and it cannot leak one, since a digest names no content.

use std::path::Path;

/// A device-local memory of file digests, keyed by workspace-relative path and
/// validated against the file's stat.
///
/// `&self` throughout: an implementation is shared with the collection walk
/// while it runs and must do its own interior locking. The hash is
/// lowercase-hex SHA-256 of the file's bytes — the spelling
/// `CollectOptions::digest` produces, so a recalled digest is comparable to a
/// `content_hash` without conversion.
pub trait DigestMemo {
    /// What `rel` hashed to, if this memory still describes the file that
    /// `len` and `mtime_ms` stat to. `None` is always a legal answer: the
    /// caller reads and hashes.
    ///
    /// `mtime_ms` is `None` for a backend that reports no modification time,
    /// which must never validate — there would be nothing to invalidate it.
    fn recall(&self, rel: &Path, len: u64, mtime_ms: Option<i64>) -> Option<String>;

    /// Remember that `rel` hashed to `hash` at the stat described. Free to
    /// decline (no room, no usable mtime); a memory is an optimization and
    /// every caller is already correct without one.
    fn remember(&self, rel: &Path, len: u64, mtime_ms: Option<i64>, hash: &str);
}

/// A memory that remembers nothing — every attachment is read and hashed.
///
/// For callers with nowhere to persist an answer: the CLI, which is a one-shot
/// process that would write a cache it could never read back, and tests that
/// are asserting on what collection does rather than on what it skips.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDigests;

impl DigestMemo for NoDigests {
    fn recall(&self, _rel: &Path, _len: u64, _mtime_ms: Option<i64>) -> Option<String> {
        None
    }
    fn remember(&self, _rel: &Path, _len: u64, _mtime_ms: Option<i64>, _hash: &str) {}
}

/// The modification time a [`DigestMemo`] key uses, in milliseconds since the
/// Unix epoch — `None` when the backend reports none.
///
/// Milliseconds because that is the resolution `IndexCache` already keys its
/// document text on, and a digest recalled beside a document's text should not
/// be validated more loosely *or* more tightly than the text is.
pub fn mtime_ms(meta: &prov::Metadata) -> Option<i64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// The shape every real implementation has, small enough to reason about:
    /// an answer is served only when both halves of the stat still agree.
    /// One remembered file: the stat it was hashed at, and what it hashed to.
    struct Seen {
        len: u64,
        mtime_ms: Option<i64>,
        hash: String,
    }

    #[derive(Default)]
    struct Map(RefCell<HashMap<PathBuf, Seen>>);

    impl DigestMemo for Map {
        fn recall(&self, rel: &Path, len: u64, mtime_ms: Option<i64>) -> Option<String> {
            let seen = self.0.borrow();
            let entry = seen.get(rel)?;
            (entry.len == len && entry.mtime_ms == mtime_ms && mtime_ms.is_some())
                .then(|| entry.hash.clone())
        }
        fn remember(&self, rel: &Path, len: u64, mtime_ms: Option<i64>, hash: &str) {
            self.0.borrow_mut().insert(
                rel.to_path_buf(),
                Seen {
                    len,
                    mtime_ms,
                    hash: hash.to_string(),
                },
            );
        }
    }

    #[test]
    fn a_remembered_digest_comes_back_at_the_same_stat() {
        let memo = Map::default();
        let path = Path::new("img/a.png");
        assert_eq!(memo.recall(path, 7, Some(1)), None, "nothing yet");

        memo.remember(path, 7, Some(1), "abc");
        assert_eq!(memo.recall(path, 7, Some(1)).as_deref(), Some("abc"));
    }

    /// Either half of the stat moving retires the entry. A length alone misses
    /// an edit that preserved the size; a timestamp alone trusts a clock the
    /// file may have arrived with.
    #[test]
    fn either_half_of_the_stat_moving_retires_it() {
        let memo = Map::default();
        let path = Path::new("img/a.png");
        memo.remember(path, 7, Some(1), "abc");

        assert_eq!(memo.recall(path, 8, Some(1)), None, "length moved");
        assert_eq!(memo.recall(path, 7, Some(2)), None, "mtime moved");
        assert_eq!(memo.recall(path, 7, None), None, "no mtime to check");
    }

    /// The null implementation is the one every caller must stay correct
    /// against: it answers nothing, forever.
    #[test]
    fn no_digests_never_answers() {
        let memo = NoDigests;
        memo.remember(Path::new("a.png"), 1, Some(1), "abc");
        assert_eq!(memo.recall(Path::new("a.png"), 1, Some(1)), None);
    }
}
