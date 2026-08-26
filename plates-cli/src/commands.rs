//! `build`, `watch` and `clean` — the three verbs that put the site on a disk.
//!
//! `serve` is [its own module](crate::serve): it is the only one that has to
//! hold a snapshot, answer requests from it and keep a build running beside
//! them.

use std::path::Path;
use std::time::Duration;

use crate::build::build_sites;
use crate::cli::{OutArgs, SiteArgs};
use crate::manifest;
use crate::scan;
use crate::session::Session;
use crate::write::{describe, write_sites};

/// How often `watch` re-stats the archive.
const POLL: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------

/// `plates build` — render the archive's sites into a directory.
pub fn build(site: &SiteArgs, out: &OutArgs, force: bool) -> Result<(), String> {
    let session = Session::open()?;
    report(&session.warnings);

    if !force {
        manifest::check_destination(&out.out).map_err(|e| {
            format!("{e}\n  (use --force to write into it anyway, or --out to name another)")
        })?;
    }

    let sites = build_sites(&session, site.site.as_deref(), site.base_url.as_deref())?;
    for built in &sites {
        report(&built.warnings);
    }

    let written = write_sites(&sites, &out.out, site.site.is_some())?;
    for built in &sites {
        println!("✓ {}", describe(built));
    }

    println!();
    println!(
        "✓ Wrote {} file{} and {} attachment{} to {}",
        written.files,
        crate::build::plural(written.files),
        written.attachments,
        crate::build::plural(written.attachments),
        out.out.display()
    );
    if written.pruned > 0 {
        println!(
            "  removed {} file{} the last build left behind",
            written.pruned,
            crate::build::plural(written.pruned)
        );
    }
    if site.base_url.is_none() {
        println!("  (no --base-url, so no sitemap, robots.txt or feeds were generated)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// watch
// ---------------------------------------------------------------------------

/// `plates watch` — build, then rebuild whenever the archive changes.
///
/// The archive is re-opened for each build rather than held, because a rebuild
/// has to see files that did not exist when the command started — including a
/// changed config document, which is where views and sites are declared.
///
/// A failure after the first build is printed and waited out rather than fatal:
/// the usual cause is a half-finished edit, and the fix is the next keystroke.
/// The *first* build is fatal, because a command that could never do the thing
/// it was asked to do should say so and stop rather than sit there.
pub fn watch(site: &SiteArgs, out: &OutArgs, force: bool) -> Result<(), String> {
    build(site, out, force)?;

    let root = Session::open()?.root_dir;
    let skip = scan::build_dir_within(&root, &out.out);
    let mut fingerprint = scan::fingerprint(&root, skip.as_deref());

    println!();
    println!("  Watching {}. Press Ctrl-C to stop.", root.display());

    loop {
        std::thread::sleep(POLL);
        let next = scan::fingerprint(&root, skip.as_deref());
        if next == fingerprint {
            continue;
        }

        match rebuild(site, out) {
            Ok(summary) => println!("  rebuilt — {summary}"),
            Err(e) => eprintln!("✗ Rebuild failed: {e}"),
        }
        // Taken *after* the build rather than from `next`, so an edit made
        // while the build was running is seen on the following tick rather than
        // absorbed into a fingerprint that was read before it happened.
        fingerprint = scan::fingerprint(&root, skip.as_deref());
    }
}

/// One rebuild, reported as a line rather than as a block.
fn rebuild(site: &SiteArgs, out: &OutArgs) -> Result<String, String> {
    let session = Session::open()?;
    report(&session.warnings);

    let sites = build_sites(&session, site.site.as_deref(), site.base_url.as_deref())?;
    // Repeated every rebuild rather than said once: a watch is where a person
    // is editing the shell, so it is the one place the message is worth
    // repeating.
    for built in &sites {
        report(&built.warnings);
    }

    let written = write_sites(&sites, &out.out, site.site.is_some())?;
    let pages: usize = sites.iter().map(|s| s.pages).sum();
    Ok(format!(
        "{} page{}{}",
        pages,
        crate::build::plural(pages),
        match written.pruned {
            0 => String::new(),
            n => format!(", {n} stale file{} removed", crate::build::plural(n)),
        }
    ))
}

// ---------------------------------------------------------------------------
// clean
// ---------------------------------------------------------------------------

/// `plates clean` — remove the files a previous build wrote.
///
/// Bounded by the manifest, not by the directory: a destination with no record
/// of a build is refused rather than emptied, because `--out` typed one
/// character wrong should not be a way to delete somebody's work. `--force`
/// removes the whole directory, and is the answer for a destination whose
/// manifest was lost.
pub fn clean(out: &OutArgs, force: bool) -> Result<(), String> {
    let dest = &out.out;
    if !dest.exists() {
        println!("✓ Nothing to clean — {} does not exist", dest.display());
        return Ok(());
    }

    if force {
        std::fs::remove_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        println!("✓ Removed {}", dest.display());
        return Ok(());
    }

    let Some(mut wrote) = manifest::read(dest).map_err(|e| format!("{}: {e}", dest.display()))?
    else {
        return Err(format!(
            "{} holds no record of a plates build, so nothing here knows what it may \
             delete\n  (use --force to remove the whole directory)",
            dest.display()
        ));
    };

    // The record removes itself last, and only as part of the same set — a
    // destination left holding a manifest of files that are gone would refuse
    // the next `clean` for a build that no longer exists.
    wrote.insert(manifest::MANIFEST.to_string());
    let removed = manifest::remove(dest, &wrote).map_err(|e| format!("{}: {e}", dest.display()))?;
    prune_if_empty(dest);

    println!(
        "✓ Removed {removed} file{} from {}",
        crate::build::plural(removed),
        dest.display()
    );
    Ok(())
}

/// Take the destination itself once nothing of ours is left in it — but leave
/// it standing if it holds anything else.
fn prune_if_empty(dest: &Path) {
    if std::fs::read_dir(dest).is_ok_and(|mut d| d.next().is_none()) {
        let _ = std::fs::remove_dir(dest);
    }
}

// ---------------------------------------------------------------------------

/// Warnings, on stderr, so a theme that did not load is visible whether the
/// caller is reading the output or piping it somewhere.
pub fn report(warnings: &[String]) {
    for warning in warnings {
        eprintln!("⚠ {warning}");
    }
}
