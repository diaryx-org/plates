//! Putting a build on disk, and taking one back off.
//!
//! The two halves of the same bookkeeping: a write records what it wrote (see
//! [`crate::manifest`]), and both a rebuild and a `clean` act only on that
//! record. Nothing in this module removes a file no build of ours wrote.

use std::collections::BTreeSet;
use std::path::Path;

use crate::build::{BuiltSite, asset_count, plural};
use crate::manifest;

/// What one write of the destination did.
pub struct Written {
    /// Rendered files laid down.
    pub files: usize,
    /// Attachments copied.
    pub attachments: usize,
    /// Files the previous build wrote that this one did not — a page whose
    /// document was deleted, renamed, or taken off the site.
    pub pruned: usize,
}

/// Write every site into `dest`, then take back what the last build left
/// behind.
///
/// `rooted` writes the single site at the destination root rather than under
/// its own name, because someone who named one site asked for one site.
/// Without it each site gets its own directory, which is the layout a host
/// serving several of them uses.
pub fn write_sites(sites: &[BuiltSite], dest: &Path, rooted: bool) -> Result<Written, String> {
    let previous = manifest::read(dest)
        .map_err(|e| format!("{}: {e}", dest.join(manifest::MANIFEST).display()))?
        .unwrap_or_default();

    let mut wrote: BTreeSet<String> = BTreeSet::new();
    let mut files = 0usize;
    let mut attachments = 0usize;

    for built in sites {
        let prefix = if rooted {
            String::new()
        } else {
            format!("{}/", built.name)
        };

        for (rel, bytes) in &built.files {
            let key = format!("{prefix}{rel}");
            write_file(&dest.join(&key), bytes)?;
            wrote.insert(key);
            files += 1;
        }

        for (rel, source) in &built.attachments {
            let key = format!("{prefix}{rel}");
            let out = dest.join(&key);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::copy(source, &out).map_err(|e| format!("{}: {e}", source.display()))?;
            wrote.insert(key);
            attachments += 1;
        }
    }

    // Prune before the manifest is rewritten, so a failure here leaves a
    // destination described by a manifest that still lists everything in it.
    let stale: BTreeSet<String> = previous.difference(&wrote).cloned().collect();
    let pruned = manifest::remove(dest, &stale).map_err(|e| format!("{}: {e}", dest.display()))?;

    manifest::write(dest, &wrote)
        .map_err(|e| format!("{}: {e}", dest.join(manifest::MANIFEST).display()))?;

    Ok(Written {
        files,
        attachments,
        pruned,
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// One line per site: what it holds and who it is for.
pub fn describe(built: &BuiltSite) -> String {
    format!(
        "{} — {} page{}, {} asset{}, {} attachment{} ({})",
        built.name,
        built.pages,
        plural(built.pages),
        asset_count(built),
        plural(asset_count(built)),
        built.attachments.len(),
        plural(built.attachments.len()),
        built.audience,
    )
}
