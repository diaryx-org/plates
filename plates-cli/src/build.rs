//! One build, and the four ways of looking at it.
//!
//! `build`, `watch` and `serve` all end here. That is the point rather than an
//! economy: a preview that walked the archive differently from a deploy would
//! agree with it right up until one of them was fixed. What comes out is
//! [`BuiltSite`] — bytes and file references, no disk and no socket — and the
//! verbs above differ only in what they do with it.
//!
//! # What is not read
//!
//! Attachments. [`UnreadAttachments`] tells collection every attachment is
//! already accounted for, which leaves each one on disk with only its path,
//! length and MIME type carried forward. `build` copies them, `serve` reads one
//! when a browser asks for it, and neither pays to pull an archive's
//! photographs through memory to render a page of text. See
//! [`plates::digest`] for the machinery this rides on.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use plates::prov::block_on;
use plates::{
    CollectOptions, DigestMemo, NoStamp, SiteTheme, collect_site, plan_site, read_page_shells,
    read_theme,
};
use plates_render::SiteStyle;
use plates_render::html::Generator;
use plates_render::site::{SiteOptions, SourceDoc, humanize_name, render_site};

use crate::session::Session;

/// Frontmatter keys stripped from every collected document.
///
/// A collected source is served publicly, so the archive's own configuration
/// must not ride along in it: prov's `prov:` block, and this binary's `sites:`
/// block, are both configuration that happens to live in a document's
/// frontmatter. Author-facing metadata — title, description, dates — is not
/// this list's business and is what the render reads.
const STRIP_KEYS: &[&str] = &[
    plates::prov::config::ROOT_CONFIG_KEY,
    crate::config::SITES_KEY,
];

/// One rendered site: everything a server would need to serve it.
pub struct BuiltSite {
    /// The site's name — its path segment in every published URL.
    pub name: String,
    /// The audience whose visible set the site was built from.
    pub audience: String,
    /// Rendered bytes keyed by their path below the site root — `index.html`,
    /// `notes/post.html`, `style.css`, `feed.xml`.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Attachments the pages reference, keyed the same way, each pointing at
    /// the file on disk to read it from. Unread on purpose — see
    /// [`UnreadAttachments`].
    ///
    /// A page's own `styles:` and `scripts:` are in here too: `plates_render`
    /// writes the tags and leaves the files to the caller, exactly as it does
    /// for an `attachments:` entry, and collection resolves all three the same
    /// way — so there is one copying rule rather than a second one that could
    /// disagree with it about where a file lands.
    pub attachments: BTreeMap<String, PathBuf>,
    /// How many of [`files`](Self::files) are pages rather than assets.
    pub pages: usize,
    /// What the archive could not deliver, in the words of whoever has to fix
    /// it: a theme file that would not open, a shell that would not compile,
    /// documents held back by a gate their author thought they had matched.
    ///
    /// Never fatal. A shell that will not compile costs the site its design and
    /// not its render, so the site is built in the built-in shell and the
    /// reason is carried here for the command to print. Silently serving the
    /// wrong design is how a broken theme survives a release.
    pub warnings: Vec<String>,
}

/// A [`DigestMemo`] that claims to recognize every attachment it is asked
/// about.
///
/// Collection asks for a digest so a *publish* can diff against what a host
/// already holds. Nothing here publishes, so the answer's only remaining effect
/// is the one that matters: a recognized attachment stays on disk instead of
/// being read into memory. Both callers want it there — the dev server reads an
/// attachment when a browser asks for one, and `build` copies it — and on an
/// archive whose photographs outweigh its prose by two orders of magnitude,
/// reading them all to render a page of text is the difference between a
/// preview and a wait.
///
/// The hash it returns is never read: no diff is computed from these
/// attachments.
struct UnreadAttachments;

impl DigestMemo for UnreadAttachments {
    fn recall(&self, _rel: &Path, _len: u64, _mtime_ms: Option<i64>) -> Option<String> {
        Some(String::new())
    }

    fn remember(&self, _rel: &Path, _len: u64, _mtime_ms: Option<i64>, _hash: &str) {}
}

/// Nothing asks these attachments for a hash, so nothing computes one.
///
/// [`CollectOptions::digest`] is a *protocol* — a digest is compared against
/// what some other system reports — and this binary compares against nothing.
fn no_digest(_bytes: &[u8]) -> String {
    String::new()
}

/// Who to credit in the footer of every shell that carries one.
fn generator() -> Generator {
    Generator::linked("plates", "https://github.com/diaryx-org/plates")
}

/// Collect and render every site the archive declares.
///
/// `only` narrows to a single site by name, case-insensitively, like every
/// other name match here. `base_url` is what canonical URLs, the sitemap and
/// the feeds are written against; without one those are skipped, which is the
/// right default for a preview whose address is `localhost`.
pub fn build_sites(
    session: &Session,
    only: Option<&str>,
    base_url: Option<&str>,
) -> Result<Vec<BuiltSite>, String> {
    if session.sites.is_empty() {
        return Err(session.nothing_to_publish());
    }

    let ws = session.workspace()?;
    // One read scope over the whole run, so a document reached by two sites is
    // parsed once rather than once per site.
    let _scope = ws.read_scope();
    let id_by_path = session.id_by_path(&ws);

    let mut built = Vec::new();
    let mut known = Vec::new();

    for spec in &session.sites {
        known.push(spec.name.clone());
        if !selected(only, &spec.name) {
            continue;
        }

        let plan = block_on(plan_site(
            &ws,
            spec,
            &session.config.views,
            &session.root_doc,
        ))
        .map_err(|e| format!("site {:?}: {e}", spec.name))?;
        let collected = block_on(collect_site(
            &ws,
            &plan,
            &CollectOptions {
                audience: &spec.audience,
                strip_keys: STRIP_KEYS,
                stamp: &NoStamp,
                id_by_path: &id_by_path,
                digests: &UnreadAttachments,
                digest: no_digest,
            },
        ))
        .map_err(|e| format!("site {:?}: {e}", spec.name))?;

        let mut theme = block_on(read_theme(&ws, spec, &session.config.views));
        block_on(read_page_shells(&ws, &collected.sources, &mut theme));

        // Documents the gate held back whose declared audience differs from it
        // only in case. Empty for every archive that never drifted; non-empty
        // means the site is publishing less than its author believes, which is
        // exactly the kind of failure that is invisible from the file alone.
        let mut warnings = theme.warnings.clone();
        if !plan.case_drift.is_empty() {
            warnings.push(format!(
                "{} document(s) declare an audience matching {:?} only in case, so the gate \
                 held them back (e.g. {})",
                plan.case_drift.len(),
                spec.audience,
                plan.case_drift[0].display(),
            ));
        }

        built.push(assemble(
            &spec.name,
            &theme,
            &spec.audience,
            collected,
            &session.root_dir,
            base_url,
            warnings,
        ));
    }

    if built.is_empty() {
        return Err(match only {
            Some(name) => format!(
                "no site named {name:?} — this archive has {}",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ),
            None => session.nothing_to_publish(),
        });
    }

    Ok(built)
}

/// Whether a `--site` filter admits this site.
fn selected(only: Option<&str>, name: &str) -> bool {
    only.is_none_or(|wanted| wanted.trim().eq_ignore_ascii_case(name.trim()))
}

/// Render one collected site into the bytes that represent it.
///
/// `theme` is the site's declaration resolved against the archive — its label,
/// shell, stylesheet, language and arrangement — as distinct from `name`, which
/// is its path segment.
fn assemble(
    name: &str,
    theme: &SiteTheme,
    audience: &str,
    collected: plates::CollectedSite,
    root: &Path,
    base_url: Option<&str>,
    mut warnings: Vec<String>,
) -> BuiltSite {
    let sources: Vec<SourceDoc> = collected
        .sources
        .iter()
        .map(|source| SourceDoc {
            path: source.source_rel_path.clone(),
            markdown: source.source_markdown.clone(),
            is_root: source.is_index,
        })
        .collect();

    let rendered = render_site(
        &sources,
        &SiteOptions {
            audience: Some(audience.to_string()),
            // What the archive calls this site. An authored front page still
            // wins — `render_site` only reaches for this when the site has
            // none — and the case it answers is the ordinary one under per-file
            // audiences: a site with no root page would otherwise take its name
            // from the placeholder title of the index synthesized for it, and
            // call itself "Index" in every `<title>`, `og:site_name` and feed.
            site_title: Some(match theme.title.trim().is_empty() {
                true => humanize_name(name),
                false => theme.title.clone(),
            }),
            base_url: base_url.map(str::to_string),
            generate_seo: true,
            generate_feeds: true,
            style: SiteStyle {
                custom_css: theme.custom_css.clone(),
                generator: Some(generator()),
                ..SiteStyle::default()
            },
            arrangement: theme.arrangement.clone(),
            front_page_supplied: collected.verbatim_front_page,
            template: theme.template.clone(),
            templates: theme
                .shells
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            lang: theme.lang.clone(),
        },
    );

    // What the declaration could not deliver, said once per site rather than
    // once per page.
    if let Some(error) = &rendered.template_error {
        warnings.push(format!(
            "site {name:?} has a shell template that will not compile, so it was ignored: {error}"
        ));
    }
    // A page's own shell, on the same terms — reported per shell rather than
    // per page, and never fatal.
    for error in &rendered.page_shell_errors {
        warnings.push(format!("site {name:?}: {error}"));
    }

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for page in rendered.pages {
        files.insert(page.dest_filename, page.html.into_bytes());
    }
    // Taken after the insert, not from `rendered.pages`: two pages can claim
    // one destination — a synthesized front page and a root document that
    // renders to `index.html` — and the later one wins here exactly as it wins
    // on disk, where both are written to the same path.
    let mut page_keys: HashSet<String> = files.keys().cloned().collect();
    for (filename, bytes) in rendered.assets {
        files.insert(filename, bytes);
    }

    // An attachment whose path collides with a rendered file — an archive
    // holding its own `style.css`, say — loses to the render.
    //
    // A site fronted by a covered directory inverts that, and only that: the
    // directory *is* the site's frame, written by hand to be served whole, so
    // its own `style.css` and `robots.txt` are the site's rather than
    // near-misses of ours. Nothing about it can overwrite a page — a covered
    // file is opaque bytes by definition, and a rendered page is always
    // `.html` — except the one file this exists for, the authored front page.
    let mut attachments = BTreeMap::new();
    for a in &collected.attachments {
        if files.contains_key(&a.dest_rel) {
            if !collected.verbatim_front_page {
                continue;
            }
            files.remove(&a.dest_rel);
            page_keys.remove(&a.dest_rel);
        }
        attachments.insert(a.dest_rel.clone(), root.join(&a.source_path));
    }
    let pages = page_keys.len();

    BuiltSite {
        name: name.to_string(),
        audience: audience.to_string(),
        files,
        attachments,
        pages,
        warnings,
    }
}

/// How many of a built site's [`files`](BuiltSite::files) are assets rather
/// than pages.
pub fn asset_count(built: &BuiltSite) -> usize {
    built.files.len() - built.pages
}

/// `""` or `"s"` — this binary counts things often enough to say it once.
pub fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
