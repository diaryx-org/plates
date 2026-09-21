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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use plates::prov::{Descent, PeerFile, block_on};
use plates::{
    CollectOptions, DigestMemo, MountOptions, NoStamp, RegistryLinks, SiteTheme, collect_mounted,
    collect_site, plan_site, read_page_shells, read_term_config, read_theme,
};
use plates_render::SiteStyle;
use plates_render::html::Generator;
use plates_render::site::{SiteOptions, SourceDoc, humanize_name, render_site};

use crate::config::Source;
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

/// Whether, and how far, a build follows foreign references into peer
/// workspaces and mounts what it finds — `--follow`, resolved.
pub struct Follow {
    /// Where the peers are.
    pub peers: PeerFile,
    /// How many boundaries to cross, and on whose say-so.
    pub descent: Descent,
}

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
    /// Files a page of this site referenced that the site does not ship —
    /// each described by a sidecar that says it is for someone else. Named so
    /// the report can say a site held something back; the render has already
    /// marked every reference to one.
    pub withheld: Vec<String>,
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
    follow: Option<&Follow>,
) -> Result<Vec<BuiltSite>, String> {
    if session.sites.is_empty() {
        return Err(session.nothing_to_publish());
    }

    let ws = session.workspace()?;
    // One read scope over the whole run, so a document reached by two sites is
    // parsed once rather than once per site.
    let _scope = ws.read_scope();
    let id_by_path = session.id_by_path(&ws);
    // Every link in the archive, resolved against the archive — walked once for
    // the whole run, since the answer does not depend on which site is asking.
    // The census is what lets a build tell a link to an unpublished page (the
    // gate working, and nothing to fix) from a link to nothing at all, and read
    // forwards it is each page's own typed relations; its inversion is each
    // page's backlinks. Collection narrows both to a site's own admitted set.
    // The second call re-iterates the read scope's memo, not the disk.
    //
    // An archive whose census cannot be read is one no site can be planned from
    // either, so the planner below says so with the whole build's exit code; a
    // report is never the thing that stops one. Backlinks are different: they
    // land in published pages, and shipping them silently empty is worse than
    // stopping.
    let census = block_on(ws.census(&session.root_doc)).unwrap_or_default();
    let backlinks = block_on(ws.backlinks(&session.root_doc))
        .map_err(|e| format!("cannot read this archive's links: {e}"))?;

    let mut built = Vec::new();
    let mut known = Vec::new();

    for spec in &session.sites {
        known.push(spec.name.clone());
        if !selected(only, &spec.name) {
            continue;
        }

        // The render-facing half of the declaration, read off the term node the
        // site's gate value names. It is folded in *here*, rather than where the
        // rest of the declaration is read, because reaching a term node takes an
        // open workspace with its id index loaded — `front_page:` may be an
        // `id:` link, and the term node itself is found by walking the archive's
        // spanning relation.
        //
        // Only for a site derived from an export. A `sites:` block wins whole or
        // not at all: half a declaration from a block and half from a term node
        // is a site nobody wrote, which is what the whole-block rule exists to
        // prevent.
        let (spec, term_warnings) = match session.source {
            Source::Declared => (spec.clone(), Vec::new()),
            _ => spec.with_term_config(block_on(read_term_config(
                &ws,
                &session.root_doc,
                &session.config,
                spec.gate_field(),
                // The value the gate compares, trimmed as prov trims it, so
                // the term node found here is the term node the gate judged
                // against.
                spec.audience.trim(),
            ))),
        };
        let spec = &spec;

        let plan = block_on(plan_site(
            &ws,
            spec,
            &session.config.views,
            &session.root_doc,
            &census,
        ))
        .map_err(|e| format!("site {:?}: {e}", spec.name))?;
        // Where each `id:` link in prose lands: this archive's registry, in the
        // site's coordinates. A mount widens the table with each peer's.
        let no_foreign = HashMap::new();
        let id_links = RegistryLinks::new(
            ws.workspace_id(),
            &id_by_path,
            &plates::anchor_of(&plan),
            "",
            &no_foreign,
        );
        let options = CollectOptions {
            audience: &spec.audience,
            gate_field: spec.gate_field(),
            strip_keys: STRIP_KEYS,
            stamp: &NoStamp,
            id_by_path: &id_by_path,
            backlinks: &backlinks,
            census: &census,
            // The same document the plan was walked from, so the site
            // carries the archive's own hierarchy for its nav to be built
            // from rather than one re-derived from `contents:` strings.
            spanning_root: Some(&session.root_doc),
            digests: &UnreadAttachments,
            digest: no_digest,
            id_links: &id_links,
            mount: "",
        };
        let mut warnings = term_warnings;
        // Where each mounted peer's pages land, and what that peer calls
        // itself — what qualifies a mounted page's identifier with the archive
        // it is actually a document of, rather than with this one.
        let mut mounted_at: Vec<(String, String)> = Vec::new();
        let collected = match follow {
            None => block_on(collect_site(&ws, &plan, &options))
                .map_err(|e| format!("site {:?}: {e}", spec.name))?,
            Some(follow) => {
                let mounted = block_on(collect_mounted(
                    &ws,
                    spec,
                    &plan,
                    &session.root_doc,
                    &options,
                    &MountOptions {
                        peers: &follow.peers,
                        descent: follow.descent,
                    },
                ))
                .map_err(|e| format!("site {:?}: {e}", spec.name))?;
                for mount in &mounted.mounts {
                    mounted_at.push((mount.prefix.clone(), mount.name.clone()));
                    println!(
                        "  mounted {} at /{} — {} page{} from {}",
                        mount.name,
                        mount.prefix,
                        mount.pages,
                        plural(mount.pages),
                        mount.root_dir.display()
                    );
                }
                warnings.extend(
                    mounted
                        .warnings
                        .into_iter()
                        .map(|w| format!("site {:?}: {w}", spec.name)),
                );
                mounted.site
            }
        };

        let mut theme = block_on(read_theme(&ws, spec, &session.config.views));
        block_on(read_page_shells(&ws, &collected.sources, &mut theme));

        // Documents the gate held back whose declared audience differs from it
        // only in case. Empty for every archive that never drifted; non-empty
        // means the site is publishing less than its author believes, which is
        // exactly the kind of failure that is invisible from the file alone.
        warnings.extend(theme.warnings.iter().cloned());
        if !plan.case_drift.is_empty() {
            warnings.push(format!(
                "{} document(s) declare an audience matching {:?} only in case, so the gate \
                 held them back (e.g. {})",
                plan.case_drift.len(),
                spec.audience,
                plan.case_drift[0].display(),
            ));
        }

        // Links this site's pages write that lead nowhere. One line each rather
        // than a count, because each one is a different file to open and a
        // different thing to type — and because the page they are written in
        // publishes either way, with the link demoted to text nobody can click
        // and no other sign that it was ever meant to go somewhere.
        for diagnostic in &plan.link_diagnostics {
            warnings.push(format!("site {:?}: {diagnostic}", spec.name));
        }

        let identifiers = identifiers(&collected, ws.workspace_id(), &mounted_at);
        built.push(assemble(
            Site {
                name: &spec.name,
                theme: &theme,
                audience: &spec.audience,
                root: &session.root_dir,
                base_url,
            },
            collected,
            identifiers,
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

/// What each page's document is called, keyed the way the render keys a page:
/// by [`plates::SourceFile::source_rel_path`].
///
/// A document's `id` is unique within the archive that registered it, and a
/// site can carry the pages of several — every mounted peer is one. So the
/// identifier written is the qualified reference, `id:<workspace>/<id>`, which
/// is the form a reference *from outside* names the document by and the form
/// prov resolves. Which workspace a page belongs to is read off its path: a
/// mounted peer's pages are collected under its prefix, and everything else is
/// this archive's own.
///
/// An anonymous workspace qualifies nothing — there is no name to qualify with
/// — and the page falls back to the render's own plain `id:<id>`.
fn identifiers(
    collected: &plates::CollectedSite,
    workspace: &str,
    mounted_at: &[(String, String)],
) -> HashMap<String, Vec<String>> {
    collected
        .sources
        .iter()
        .filter_map(|source| {
            let id = source.id.as_deref()?.trim();
            if id.is_empty() {
                return None;
            }
            // The longest matching prefix, so a peer mounted below another peer
            // is named by the one it is actually a document of.
            let owner = mounted_at
                .iter()
                .filter(|(prefix, _)| source.source_rel_path.starts_with(prefix.as_str()))
                .max_by_key(|(prefix, _)| prefix.len())
                .map(|(_, name)| name.as_str())
                .unwrap_or(workspace)
                .trim();
            if owner.is_empty() {
                return None;
            }
            Some((
                source.source_rel_path.clone(),
                vec![format!("id:{owner}/{id}")],
            ))
        })
        .collect()
}

/// What a site is, apart from the documents in it: the five values the whole
/// of [`assemble`] reads and none of it writes.
///
/// A struct because they arrived one at a time and the call had grown to eight
/// positional arguments, three of them `&str` — which is a call whose next
/// argument goes in the wrong place and still compiles.
struct Site<'a> {
    /// The site's path segment, as its declaration names it.
    name: &'a str,
    /// Its declaration resolved against the archive — label, shell,
    /// stylesheet, language and arrangement.
    theme: &'a SiteTheme,
    /// The audience the gate admitted these documents to.
    audience: &'a str,
    /// The archive's root directory, which an attachment's path is relative to.
    root: &'a Path,
    /// The address the site is served at, for canonical links and feeds.
    base_url: Option<&'a str>,
}

/// Render one collected site into the bytes that represent it.
///
/// `theme` is the site's declaration resolved against the archive — its label,
/// shell, stylesheet, language and arrangement — as distinct from `name`, which
/// is its path segment.
fn assemble(
    site: Site<'_>,
    collected: plates::CollectedSite,
    identifiers: HashMap<String, Vec<String>>,
    mut warnings: Vec<String>,
) -> BuiltSite {
    let Site {
        name,
        theme,
        audience,
        root,
        base_url,
    } = site;
    let sources: Vec<SourceDoc> = collected
        .sources
        .iter()
        .map(|source| SourceDoc {
            path: source.source_rel_path.clone(),
            markdown: source.source_markdown.clone(),
            is_root: source.is_index,
            // Both directions already narrowed to this site by collection,
            // which is the layer that knows what the gate refused.
            inbound: source.inbound.clone(),
            outbound: source.outbound.clone(),
        })
        .collect();

    // What the site ships, so a page's reference to a file it does not — one
    // whose sidecar says it is for someone else — is marked like a link to a
    // page the gate refused, rather than pointing at nothing.
    let published_files = collected
        .attachments
        .iter()
        .map(|a| a.dest_rel.clone())
        .collect();

    let rendered = render_site(
        &sources,
        &SiteOptions {
            audience: Some(audience.to_string()),
            published_files: Some(published_files),
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
            // Which document contains which, as the archive itself says —
            // collection walked the relation this workspace configures, and the
            // render layer has no workspace to ask.
            outline: collected.outline,
            // Which document each page is, named the way a reference from
            // outside this archive would name it.
            identifiers,
            front_page_supplied: collected.verbatim_front_page,
            template: theme.template.clone(),
            templates: theme
                .shells
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            lang: theme.lang.clone(),
            syntaxes: theme
                .syntaxes
                .iter()
                .map(|(path, text)| (path.clone(), text.clone()))
                .collect(),
            header: theme.header.clone(),
            footer: theme.footer.clone(),
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
    // And a grammar that will not parse, which costs the languages it covered
    // their colour and nothing else.
    for error in &rendered.syntax_errors {
        warnings.push(format!("site {name:?}: {error}"));
    }
    // A body template, which unlike the three above is authorial: a page names
    // itself here when its template will not expand, or when it still writes a
    // `{{ }}` in a position that is no longer a template.
    for error in &rendered.body_template_errors {
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
        withheld: collected.withheld,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn source(path: &str, id: Option<&str>) -> plates::SourceFile {
        plates::SourceFile {
            source_markdown: String::new(),
            source_rel_path: path.to_string(),
            dest_path: path.replace(".md", ".html"),
            id: id.map(str::to_string),
            is_index: false,
            inbound: Vec::new(),
            outbound: Vec::new(),
        }
    }

    fn site(sources: Vec<plates::SourceFile>) -> plates::CollectedSite {
        plates::CollectedSite {
            sources,
            ..Default::default()
        }
    }

    /// A page of this archive is named by this archive; a page of a peer
    /// mounted under it is named by the peer, because that is whose registry
    /// the id is in.
    #[test]
    fn a_mounted_page_is_named_by_the_archive_it_belongs_to() {
        let collected = site(vec![
            source("index.md", Some("p8ftrd5")),
            source("fig/index.md", Some("ajp7eq")),
        ]);
        let map = identifiers(
            &collected,
            "plates",
            &[("fig/".to_string(), "fig".to_string())],
        );

        assert_eq!(map["index.md"], vec!["id:plates/p8ftrd5".to_string()]);
        assert_eq!(map["fig/index.md"], vec!["id:fig/ajp7eq".to_string()]);
    }

    /// Nothing to say is said with silence: a document with no id, and an
    /// archive with no name, are both left to the render's own answer.
    #[test]
    fn a_page_with_nothing_to_qualify_is_absent() {
        let collected = site(vec![source("index.md", None), source("a.md", Some("x1"))]);

        assert!(!identifiers(&collected, "plates", &[]).contains_key("index.md"));
        assert!(identifiers(&collected, "", &[]).is_empty());
    }
}
