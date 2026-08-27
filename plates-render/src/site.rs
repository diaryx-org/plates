//! Server-side site reconstruction and rendering (ARK Layer 3, Phase 2).
//!
//! Rebuilds [`PublishedPage`]s from stored **sources** and renders the whole
//! site, mirroring the publish plugin's page-derivation rules so the server can
//! render-on-write. The stored sources are already audience-scoped and
//! visibility-filtered (Layer 2), but pre-template — so the per-page pipeline
//! here is: parse → template → preprocess → render → transform_links → page
//! assembly. Gated behind the `templating` feature.
//!
//! Each source is parsed in its own grammar, which [`crate::body`] reads off the
//! path's extension. A site is not required to be all one format.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::frontmatter;
use indexmap::IndexMap;
use prov::ContentFormat;
use prov::Value as YamlValue;
use prov::views::{Row, Selection};
use serde_json::Value as JsonValue;

use crate::dates;

use crate::html::{HtmlRenderer, PageContext, SiteStyle};
use crate::nav::{build_site_nav_tree, nav_for_page};
use crate::shell::ShellTemplate;
use crate::types::{NavLink, PageLayout, PublishedPage};
use crate::{body, links, page, template};

/// A stored source document to render.
pub struct SourceDoc {
    /// Canonical workspace-relative path including its extension, e.g.
    /// `"subdir/child.md"`. No leading slash. The extension is what decides the
    /// body's grammar, so it is load-bearing rather than decorative.
    pub path: String,
    /// The raw source (frontmatter + visibility-filtered, pre-template body), as
    /// stored by Layer 2.
    pub markdown: String,
    /// Whether this is the workspace root/index page (renders to `index.html`).
    pub is_root: bool,
}

/// A fully rendered page: its output filename, HTML, and source identifier.
pub struct RenderedPage {
    /// Destination filename, e.g. `"index.html"` or `"subdir/child.html"`.
    pub dest_filename: String,
    /// The complete HTML document.
    pub html: String,
    /// The source document's identifier (frontmatter `id`), if it has one.
    pub id: Option<String>,
    /// Stylesheets the page links (frontmatter `styles`), as paths below the
    /// site root. The rendered HTML already points at them; **copying the files
    /// there is the caller's**, exactly as it is for `attachments`.
    pub styles: Vec<String>,
    /// Scripts the page loads (frontmatter `scripts`), on the same terms as
    /// [`styles`](Self::styles).
    pub scripts: Vec<String>,
}

pub use crate::types::{Arrangement, Grain, Grouping, serve_at_dest};

/// Options controlling a site render.
pub struct SiteOptions {
    /// Target audience (used for template `viewer_audience` variables).
    ///
    /// The *gate*, not the site's name: a site's public path segment is its own
    /// (`exports.<name>`), deliberately separate so an audience named to be
    /// precise about who is reading never becomes a URL. This field feeds the
    /// body templating only.
    pub audience: Option<String>,
    /// Site title override; defaults to the root page's title.
    pub site_title: Option<String>,
    /// Base URL for sitemap/canonical/feeds; when empty those are skipped.
    pub base_url: Option<String>,
    /// Generate SEO meta + sitemap/robots.
    pub generate_seo: bool,
    /// Generate Atom/RSS feeds + feed `<link>` tags. Both together, and only
    /// with a `base_url`: a feed needs absolute URLs, so without one there is
    /// no feed to advertise either.
    pub generate_feeds: bool,
    /// Caller-supplied appearance (theme/custom CSS/custom favicon).
    pub style: SiteStyle,
    /// How the site is arranged, from its declared view.
    pub arrangement: Arrangement,
    /// The site's shell template, as the text of the template file.
    ///
    /// A string rather than a path because this crate reads nothing: it is
    /// `wasm32-unknown-unknown`-portable, so a caller with a template on disk
    /// loads it and passes it in. `None` uses the built-in shell, which is what
    /// every site published before templates existed gets — byte for byte.
    ///
    /// The template is the whole document, `<!DOCTYPE html>` to `</html>`, with
    /// named slots for the parts a render computes. `{{name}}` inserts a text
    /// slot HTML-escaped; `{{{name}}}` inserts a raw HTML slot verbatim; each
    /// slot is one kind and writing it the other way is an error rather than a
    /// page full of `&lt;div&gt;`.
    ///
    /// | Slot | Kind | What it holds |
    /// |---|---|---|
    /// | `lang` | text | [`SiteOptions::lang`], for `<html lang="…">` |
    /// | `document_title` | text | `"Entry - Site"`, or the site's name on the front page |
    /// | `site_title` | text | the site's name on its own |
    /// | `body_class` | text | `has-site-nav`, or empty — write it inside `class="…"` |
    /// | `head` | raw | stylesheet, favicon, SEO meta, feed links, the page's `styles:` |
    /// | `site_nav` | raw | the navigation sidebar, empty when the site has no tree |
    /// | `breadcrumbs` | raw | the breadcrumb trail |
    /// | `content` | raw | the rendered body, links already rewritten |
    /// | `footer` | raw | the built-in attribution footer |
    /// | `scripts` | raw | the built-in interactivity script, then the page's `scripts:` |
    ///
    /// `<title>` is not part of `head`, so a template decides where its own
    /// title tag goes. A page whose frontmatter says `layout: bare` or
    /// `layout: verbatim` ignores the template entirely — both are statements
    /// that the page carries its own frame. See [`PageLayout`].
    ///
    /// A template that will not compile does not fail the render: the site is
    /// published in the built-in shell and the reason is reported on
    /// [`SiteRender::template_error`], because a broken theme should cost a
    /// site its styling rather than its publication.
    pub template: Option<String>,
    /// Shell templates a *page* may name, keyed by the vault-relative path it
    /// names them by — the text of each file, for the same reason
    /// [`template`](Self::template) is text.
    ///
    /// A page whose frontmatter says `shell: .config/sites/blog/poster.html` is
    /// wrapped in the entry under that key instead of the site's own shell. A
    /// key this map does not hold, or a template that will not compile, falls
    /// back to the site shell and is reported on
    /// [`SiteRender::page_shell_errors`] — the same bargain a broken site
    /// template gets, for the same reason. `bare` and `verbatim` pages take no
    /// shell at all, so a `shell:` on one of them is nothing to apply.
    pub templates: IndexMap<String, String>,
    /// BCP 47 language tag for every page's `<html lang="…">`. `"en"` unless
    /// the caller knows better; a vault written in another language should say
    /// so, since this is what a screen reader picks a voice from.
    pub lang: String,
    /// The caller is supplying `index.html` itself, so do not synthesize one.
    ///
    /// A site fronted by a manifest node (`plates::IndexDirectory`)
    /// serves an authored page copied verbatim, which never passes through this
    /// crate — so from here the render set simply has no root, which is
    /// otherwise precisely the signal that a front page must be generated.
    /// Without this flag the synthesized listing is written over the authored
    /// page, in the build directory and in the published namespace alike, and
    /// the site loses the front door it was fronted with.
    ///
    /// It does not make the nav, the feeds or the sitemap pretend a root exists.
    /// A supplied front page is outside this crate's knowledge — it contributes
    /// no title, no description, no nav entry and no sitemap row, exactly as a
    /// page nobody rendered should. An authored landing page carries its own
    /// `<title>` and meta tags; that is what fronting a site with one is for.
    pub front_page_supplied: bool,
    /// Grammars for highlighting code beyond the built-in set, keyed by the
    /// path the declaration named them by — the **text** of each
    /// `.sublime-syntax` file, for the reason [`template`](Self::template) is
    /// text.
    ///
    /// The built-in set is `two-face`'s 213 grammars, which is already most
    /// languages anyone fences a block in. This is for the rest: an in-house
    /// language, a config dialect, a notation a vault invented. A key here
    /// whose grammar declares `file_extensions: [wat]` is what makes
    /// ```` ```wat ```` colour.
    ///
    /// Assembled once for the whole site. A grammar that will not parse is
    /// reported on [`SiteRender::syntax_errors`] and skipped, never fatal —
    /// the bargain a broken shell template gets, for the same reason.
    ///
    /// Ignored entirely without the `syntax-highlighting` feature, where no
    /// block is coloured and there is nothing for a grammar to do.
    pub syntaxes: IndexMap<String, String>,
}

impl Default for SiteOptions {
    fn default() -> Self {
        Self {
            audience: None,
            site_title: None,
            base_url: None,
            generate_seo: true,
            generate_feeds: true,
            style: SiteStyle::default(),
            arrangement: Arrangement::default(),
            template: None,
            templates: IndexMap::new(),
            lang: DEFAULT_LANG.to_string(),
            front_page_supplied: false,
            syntaxes: IndexMap::new(),
        }
    }
}

/// The grammar set one render uses, and whether it had to be built.
///
/// The overwhelmingly common case is a site with no grammars of its own, which
/// should cost nothing: that arm borrows the process-wide bundle rather than
/// unpacking a second copy of a megabyte of dumps.
#[cfg(feature = "syntax-highlighting")]
enum ResolvedSyntaxes {
    Bundled,
    Custom(crate::syntax::Syntaxes),
}

#[cfg(feature = "syntax-highlighting")]
impl ResolvedSyntaxes {
    fn get(&self) -> &crate::syntax::Syntaxes {
        match self {
            Self::Bundled => crate::syntax::Syntaxes::bundled(),
            Self::Custom(set) => set,
        }
    }

    fn warnings(&self) -> &[String] {
        self.get().warnings()
    }
}

/// Assemble the grammars for one render — **once**, because unpacking the
/// dumps costs far more than using them and a site that did it per page would
/// pay that for every document it publishes.
#[cfg(feature = "syntax-highlighting")]
fn resolve_syntaxes(opts: &SiteOptions) -> ResolvedSyntaxes {
    if opts.syntaxes.is_empty() {
        return ResolvedSyntaxes::Bundled;
    }
    ResolvedSyntaxes::Custom(crate::syntax::Syntaxes::with_custom(
        opts.syntaxes
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str())),
    ))
}

/// The language a site is assumed to be in when its caller does not say.
const DEFAULT_LANG: &str = "en";

/// The result of rendering a site: the pages plus the static/supplementary
/// assets (`style.css`, favicon, `sitemap.xml`, `robots.txt`, feeds).
pub struct SiteRender {
    /// Rendered pages.
    pub pages: Vec<RenderedPage>,
    /// `(filename, bytes)` assets to write alongside the pages.
    pub assets: Vec<(String, Vec<u8>)>,
    /// Why [`SiteOptions::template`] was ignored, when it was.
    ///
    /// A render has no error channel — every page in `pages` is real HTML — so a
    /// template that will not compile falls back to the built-in shell and says
    /// so here. A caller that can report it should: silently serving the wrong
    /// design is how a broken theme survives a release.
    pub template_error: Option<String>,
    /// Why a page's own `shell:` was ignored, once per shell rather than once
    /// per page that named it.
    ///
    /// Separate from [`template_error`](Self::template_error) because the two
    /// are different failures with different fixes: that one is the site's
    /// shell, this one is a page naming a template the site does not carry or
    /// cannot compile. Both fall back to a shell that works, and both are the
    /// caller's to report.
    pub page_shell_errors: Vec<String>,
    /// Why a grammar in [`SiteOptions::syntaxes`] was ignored, once per
    /// grammar.
    ///
    /// A `.sublime-syntax` that will not parse costs the languages it covered
    /// their colour and nothing else — every other block still highlights, and
    /// the site still publishes. Empty without the `syntax-highlighting`
    /// feature, where no grammar is consulted in the first place.
    pub syntax_errors: Vec<String>,
    /// What went wrong in a page's **body** template, named by the page.
    ///
    /// A body whose template will not expand publishes its own source, which
    /// used to happen with nothing reported at all — the failure this crate
    /// spent [`template_error`](Self::template_error) and
    /// [`page_shell_errors`](Self::page_shell_errors) refusing to allow for a
    /// *shell*, arriving through the one surface that had no channel for it.
    /// It carries the `{{ }}` migration's warnings too, which are not failures:
    /// a brace outside a link destination is no longer a template, and the page
    /// publishes it as the text it is.
    pub body_template_errors: Vec<String>,
}

/// Reconstruct [`PublishedPage`]s from stored sources, fully rendering each
/// page's `rendered_body` (template → preprocess → comrak → link rewrite).
///
/// The returned set does **not** include a synthesized index; that is
/// [`synthesize_index`]'s job, applied by [`render_site`] when no source claims
/// `is_root`.
/// The returned set also drops whatever the body templates had to say. A
/// caller that wants those calls [`render_site`], which carries them in
/// [`SiteRender::body_template_errors`]; this entry point has no error channel
/// and adding one to its return type would change what a page *is*.
pub fn build_pages(sources: &[SourceDoc], opts: &SiteOptions) -> Vec<PublishedPage> {
    #[cfg(feature = "syntax-highlighting")]
    let syntaxes = resolve_syntaxes(opts);
    pages_from(
        sources,
        opts,
        #[cfg(feature = "syntax-highlighting")]
        syntaxes.get(),
        &mut Vec::new(),
    )
}

/// [`build_pages`], against a grammar set the caller has already assembled.
///
/// Split out so that [`render_site`] — which needs the set's warnings for
/// [`SiteRender::syntax_errors`] — can assemble it once and read both, rather
/// than unpacking the dumps a second time to ask what went wrong the first.
fn pages_from(
    sources: &[SourceDoc],
    opts: &SiteOptions,
    #[cfg(feature = "syntax-highlighting")] syntaxes: &crate::syntax::Syntaxes,
    reports: &mut Vec<String>,
) -> Vec<PublishedPage> {
    // Map sanitized canonical `.md` path → output `.html` filename (root →
    // index.html, a `serve_at:` claim to what it claims). Sources are keyed by
    // their workspace-relative path; we sanitize keys so that frontmatter links
    // (which may carry unsanitized characters) resolve against them.
    //
    // …and, from the same parse, the frontmatter title (for contents/parent
    // titles). One pass because both answers come out of one metadata block,
    // and parsing the corpus twice to ask it two questions is a parse per
    // document nobody needs.
    let mut path_to_filename: HashMap<PathBuf, String> = HashMap::new();
    let mut title_map: HashMap<PathBuf, String> = HashMap::new();
    for s in sources {
        let key = PathBuf::from(links::sanitize_rel_path(&s.path));
        let fm = frontmatter::parse_or_empty(&s.markdown)
            .map(|parsed| parsed.frontmatter)
            .unwrap_or_default();
        if let Some(t) = frontmatter::get_string(&fm, "title") {
            title_map.insert(key.clone(), t.to_string());
        }
        path_to_filename.insert(key, dest_for(&s.path, s.is_root, &fm));
    }

    // The collection context, from the same sources and therefore from the
    // same gate: `build_pages` is handed the audience-admitted set, so a
    // template cannot name a withheld document because the data holding it was
    // never assembled. That is a property of *where* this is built, which is
    // why `a_template_cannot_reach_a_withheld_document` tests the shape of the
    // pipeline rather than a check inside it.
    let collected = collect_context(sources, opts, &path_to_filename);

    sources
        .iter()
        .map(|s| {
            build_page(
                s,
                opts,
                &path_to_filename,
                &title_map,
                &collected,
                #[cfg(feature = "syntax-highlighting")]
                syntaxes,
                reports,
            )
        })
        .collect()
}

// ── The template context ────────────────────────────────────────────────────

/// The site-level context, plus the two per-path lookups a page's own half of
/// it is assembled from.
struct Collected {
    /// `site`, `entries` and `groups` — one copy for the whole render, borrowed
    /// by every page rather than cloned into each.
    context: template::SiteContext,
    /// The entry record for each source, keyed by its sanitized path. This is
    /// what a page names as `page`, and what a breadcrumb trail is made of.
    by_path: HashMap<PathBuf, JsonValue>,
    /// Each source's `part_of` target, for walking a trail back to the root.
    parent_of: HashMap<PathBuf, PathBuf>,
    /// Entry records in the site's order, so a breadcrumb walk and `entries`
    /// agree about what an entry is.
    order: Vec<PathBuf>,
}

/// Assemble everything a template can name, from frontmatter alone.
///
/// Frontmatter alone is the point: an entry record needs a title, a href, a
/// date and its group keys, and every one of those is metadata. Nothing here
/// renders a body, so the context is available *before* the first page is
/// built — which is what breaks the circularity of a page whose template lists
/// the pages.
fn collect_context(
    sources: &[SourceDoc],
    opts: &SiteOptions,
    path_to_filename: &HashMap<PathBuf, String>,
) -> Collected {
    let mut by_path: HashMap<PathBuf, JsonValue> = HashMap::new();
    let mut parent_of: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut sortable: Vec<(i32, PathBuf)> = Vec::new();
    let mut root_title: Option<String> = None;
    // Kept whole rather than reduced to group keys here, because prov's grouper
    // takes metadata and answers the grouping question itself — see
    // [`groups_of`].
    let mut meta_of: HashMap<PathBuf, YamlValue> = HashMap::new();

    for (idx, s) in sources.iter().enumerate() {
        let key = PathBuf::from(links::sanitize_rel_path(&s.path));
        let fm = frontmatter::parse_or_empty(&s.markdown)
            .map(|parsed| parsed.frontmatter)
            .unwrap_or_default();

        let title = frontmatter::get_string(&fm, "title")
            .map(String::from)
            .unwrap_or_else(|| filename_to_title(&s.path));
        if s.is_root {
            root_title = Some(title.clone());
        }

        let date = frontmatter::get_string(&fm, "date_of_document")
            .or_else(|| frontmatter::get_string(&fm, "created"))
            .or_else(|| frontmatter::get_string(&fm, "updated"))
            .filter(|d| !d.is_empty())
            .map(String::from);
        let group_keys = match &opts.arrangement {
            Arrangement::Containment => Vec::new(),
            Arrangement::Grouped(grouping) => grouping.keys_of(&YamlValue::Mapping(fm.clone())),
        };

        if let Some(parent) = frontmatter::get_string(&fm, "part_of") {
            let link = prov::Link::parse_path_only(parent.trim());
            let canonical = prov::link::resolve(Path::new(&s.path), &link.target);
            parent_of.insert(
                key.clone(),
                PathBuf::from(links::sanitize_rel_path(&canonical.to_string_lossy())),
            );
        }

        let href = path_to_filename
            .get(&key)
            .cloned()
            .unwrap_or_else(|| dest_for(&s.path, s.is_root, &fm));

        by_path.insert(
            key.clone(),
            entry_value(&s.path, &title, &href, date, &fm, group_keys, s.is_root),
        );
        meta_of.insert(key.clone(), YamlValue::Mapping(fm.clone()));

        // Source order, `nav_order` overriding — the rule `crate::nav` sorts
        // siblings by, restated here so a template listing entries and a nav
        // listing them cannot disagree.
        let order_key = fm
            .get("nav_order")
            .and_then(|v| match v {
                YamlValue::Int(i) => Some(*i as i32),
                YamlValue::Float(f) => Some(*f as i32),
                YamlValue::String(st) => st.parse::<i32>().ok(),
                _ => None,
            })
            .unwrap_or(idx as i32);
        sortable.push((order_key, key));
    }

    sortable.sort_by_key(|(k, _)| *k);
    let order: Vec<PathBuf> = sortable.into_iter().map(|(_, key)| key).collect();
    let entries: Vec<JsonValue> = order
        .iter()
        .filter_map(|key| by_path.get(key).cloned())
        .collect();

    let site = serde_json::json!({
        "title": opts
            .site_title
            .clone()
            .or(root_title)
            .unwrap_or_else(|| DEFAULT_SITE_TITLE.to_string()),
        "lang": opts.lang.clone(),
        "base_url": opts.base_url.clone().unwrap_or_default(),
    });

    Collected {
        context: template::SiteContext::new(
            site,
            entries.clone(),
            groups_of(&order, &meta_of, &by_path, &opts.arrangement),
        ),
        by_path,
        parent_of,
        order,
    }
}

/// One entry, as a template names it.
///
/// `date_year` and `date_month` are here rather than in a filter syntax on
/// purpose: a filter language is the thing that turns a template format into a
/// template *engine*, and this crate already knows how to read a date. A field
/// that turns out to be wanted is one line; a filter grammar is permanent.
fn entry_value(
    path: &str,
    title: &str,
    href: &str,
    date: Option<String>,
    fm: &IndexMap<String, YamlValue>,
    group_keys: Vec<String>,
    is_root: bool,
) -> JsonValue {
    let normalized = date.as_deref().and_then(dates::to_rfc3339);
    serde_json::json!({
        "path": path,
        "title": title,
        "href": href,
        "date": date,
        "date_year": normalized.as_deref().and_then(|d| d.get(0..4)),
        "date_month": normalized.as_deref().and_then(|d| d.get(0..7)),
        "id": frontmatter::get_string(fm, "id"),
        "description": frontmatter::get_string(fm, "description"),
        "group_keys": group_keys,
        "is_root": is_root,
    })
}

/// Gather entries into `{key, entries}` records, ascending by group key.
///
/// The buckets and their order are prov's, not this crate's: a published site
/// and the picker over the archive it came from must file a letter about two
/// people under both their names, and in the same order, or the site reads
/// differently from the vault. That is the same reasoning that made
/// [`Grouping`] prov's rather than a copy of it, applied one layer further out
/// — the grouper is pure, so it costs this crate none of its portability.
///
/// Empty when the arrangement is containment, because then nothing is grouped —
/// which is also the honest answer for `:::group` on an ungrouped site: no
/// groups, so no repetitions.
///
/// prov's `ungrouped` bucket is deliberately dropped rather than appended as a
/// group of its own: an entry no field gave a key for belongs under no heading,
/// and `entries` already lists every one of them for a template that wants the
/// flat set. The synthesized index keeps its own labelled bucket
/// ([`group_entries`]), which is a *nav* question and answered there.
fn groups_of(
    order: &[PathBuf],
    meta_of: &HashMap<PathBuf, YamlValue>,
    by_path: &HashMap<PathBuf, JsonValue>,
    arrangement: &Arrangement,
) -> Vec<JsonValue> {
    let Arrangement::Grouped(grouping) = arrangement else {
        return Vec::new();
    };
    // The view name is prov's handle for the selection and nothing here reads
    // it back; the arrangement arrives without one.
    let selection = Selection {
        view: String::new(),
        rows: order
            .iter()
            .filter_map(|key| {
                Some(Row {
                    path: key.clone(),
                    meta: meta_of.get(key)?.clone(),
                })
            })
            .collect(),
    };
    prov::views::group(&selection, grouping)
        .groups
        .into_iter()
        .map(|group| {
            let entries: Vec<JsonValue> = group
                .rows
                .iter()
                .filter_map(|row| by_path.get(&row.path).cloned())
                .collect();
            serde_json::json!({ "key": group.key, "entries": entries })
        })
        .collect()
}

/// This page's own half of the context: what it is, what it contains, what
/// contains it, and the trail from the root down to it.
fn page_context_values(
    s: &SourceDoc,
    fm: &IndexMap<String, YamlValue>,
    collected: &Collected,
    contents_links: &[NavLink],
    parent_link: Option<&NavLink>,
    audience: Option<&str>,
) -> serde_json::Map<String, JsonValue> {
    let viewer: Vec<&str> = audience.into_iter().collect();
    let mut values = template::page_values(fm, Path::new(&s.path), None, &viewer);
    let key = PathBuf::from(links::sanitize_rel_path(&s.path));

    if let Some(entry) = collected.by_path.get(&key) {
        values.insert("page".into(), entry.clone());
    }
    values.insert(
        "children".into(),
        JsonValue::Array(contents_links.iter().map(nav_link_value).collect()),
    );
    values.insert(
        "parent".into(),
        parent_link.map(nav_link_value).unwrap_or(JsonValue::Null),
    );
    values.insert(
        "breadcrumbs".into(),
        JsonValue::Array(breadcrumbs_of(&key, collected)),
    );
    values
}

fn nav_link_value(link: &NavLink) -> JsonValue {
    serde_json::json!({ "title": link.title, "href": link.href })
}

/// The trail from the site's root down to one page, itself included.
///
/// Walked over `part_of` rather than over the nav tree, because the nav tree is
/// built from rendered pages and this runs before any of them exist. The walk
/// is bounded by the number of entries and refuses to revisit a path, so a
/// vault whose `part_of` links form a cycle produces a short trail instead of
/// hanging a publish.
fn breadcrumbs_of(from: &Path, collected: &Collected) -> Vec<JsonValue> {
    let mut trail: Vec<JsonValue> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut at = from.to_path_buf();
    for _ in 0..=collected.order.len() {
        if seen.contains(&at) {
            break;
        }
        let Some(entry) = collected.by_path.get(&at) else {
            break;
        };
        trail.push(entry.clone());
        seen.push(at.clone());
        let Some(parent) = collected.parent_of.get(&at) else {
            break;
        };
        at = parent.clone();
    }
    trail.reverse();
    trail
}

/// Reconstruct and render a whole site from stored sources.
///
/// When no source claims `is_root`, a front page is synthesized from the render
/// set (see [`synthesize_index`]). Under explicit-only audience visibility that
/// is the ordinary case, not a fallback: a vault's root document is private
/// unless its author tagged it, and promoting whichever entry happened to sort
/// first made the site's front page — and, through the publish layer, its ARK —
/// depend on traversal order.
pub fn render_site(sources: &[SourceDoc], opts: &SiteOptions) -> SiteRender {
    #[cfg(feature = "syntax-highlighting")]
    let syntaxes = resolve_syntaxes(opts);
    let mut body_template_errors = Vec::new();
    let mut pages = pages_from(
        sources,
        opts,
        #[cfg(feature = "syntax-highlighting")]
        syntaxes.get(),
        &mut body_template_errors,
    );

    // Read the site's name off an **authored** root, before synthesis can add
    // one. A synthesized front page is named *after* the site, so asking it
    // what the site is called only ever returns the placeholder back — which is
    // how every page of a rootless site came to be titled "… - Index" and to
    // announce `og:site_name: Index` to every reader that scraped it.
    let site_title = opts
        .site_title
        .clone()
        .or_else(|| pages.iter().find(|p| p.is_root).map(|p| p.title.clone()))
        .unwrap_or_else(|| DEFAULT_SITE_TITLE.to_string());

    if !pages.iter().any(|p| p.is_root) && !pages.is_empty() && !opts.front_page_supplied {
        let index = synthesize_index(&pages, opts);
        pages.insert(0, index);
    }

    let renderer = HtmlRenderer::with_style(opts.style.clone());
    let nav_tree = build_site_nav_tree(&pages);

    // Compiled once for the whole site, not once per page: a template's errors
    // are about the template, and reporting them per page would say the same
    // thing as many times as the vault has entries.
    let (template, template_error) = match opts.template.as_deref() {
        None => (None, None),
        Some(source) => match ShellTemplate::parse(source) {
            Ok(compiled) => (Some(compiled), None),
            Err(err) => (None, Some(err.to_string())),
        },
    };

    // Every shell a page named, compiled once per shell — not once per page
    // that named it. A poster template shared by forty entries is one template,
    // and a template that will not compile is one report.
    let mut page_shell_errors: Vec<String> = Vec::new();
    let mut page_templates: IndexMap<&str, Option<ShellTemplate>> = IndexMap::new();
    for p in &pages {
        let Some(key) = p.shell.as_deref() else {
            continue;
        };
        if page_templates.contains_key(key) {
            continue;
        }
        let compiled = match opts.templates.get(key) {
            None => {
                page_shell_errors.push(format!(
                    "{} asks for the shell {key:?}, which this site does not carry — \
                     it is rendered in the site's own shell",
                    p.source_path.display()
                ));
                None
            }
            Some(source) => match ShellTemplate::parse(source) {
                Ok(compiled) => Some(compiled),
                Err(err) => {
                    page_shell_errors.push(format!(
                        "the shell {key:?}, which {} asks for, will not compile ({err}) — \
                         it is rendered in the site's own shell",
                        p.source_path.display()
                    ));
                    None
                }
            },
        };
        page_templates.insert(key, compiled);
    }

    let base_url = opts.base_url.as_deref().unwrap_or("");
    let writes_feeds = opts.generate_feeds && !base_url.is_empty();

    let mut out_pages = Vec::with_capacity(pages.len());
    for p in &pages {
        let nav = nav_for_page(&nav_tree, &p.dest_filename, &pages);
        let seo = if opts.generate_seo {
            page::generate_seo_meta(p, &site_title, base_url)
        } else {
            String::new()
        };
        // Advertised on exactly the condition the files are written under
        // below. The tags used to hang off `generate_feeds` alone, so a render
        // with no base URL — every published site, since no client sent one —
        // put a `<link rel="alternate">` on every page pointing at a
        // `feed.xml` the same render had just decided to skip.
        let feeds = if writes_feeds {
            page::generate_feed_link_tags(&links::root_prefix(&p.dest_filename))
        } else {
            String::new()
        };
        // The page's own shell when it named one this render could compile,
        // and the site's otherwise — which is also what a page that named a
        // shell nobody could load falls back to.
        let shell = p
            .shell
            .as_deref()
            .and_then(|key| page_templates.get(key))
            .and_then(Option::as_ref)
            .or(template.as_ref());
        let html = renderer.render_page_in_site(
            p,
            &PageContext {
                site_title: &site_title,
                nav: &nav,
                seo_meta: &seo,
                feed_links: &feeds,
                lang: &opts.lang,
                template: shell,
            },
        );
        out_pages.push(RenderedPage {
            dest_filename: p.dest_filename.clone(),
            html,
            id: p.id.clone(),
            styles: p.styles.clone(),
            scripts: p.scripts.clone(),
        });
    }

    // Static assets (style.css + favicon) always; supplementary files need a base URL.
    let mut assets = renderer.static_assets();
    if !base_url.is_empty() {
        if opts.generate_seo {
            assets.push((
                "sitemap.xml".to_string(),
                page::generate_sitemap(&pages, base_url).into_bytes(),
            ));
            assets.push((
                "robots.txt".to_string(),
                page::generate_robots_txt(base_url, true).into_bytes(),
            ));
        }
        if writes_feeds {
            let root = pages.iter().find(|p| p.is_root);
            let desc = root.and_then(|r| r.description.as_deref()).unwrap_or("");
            let author = root.and_then(|r| r.author.as_deref()).unwrap_or("");
            assets.push((
                "feed.xml".to_string(),
                page::generate_atom_feed(&pages, &site_title, base_url, desc, author).into_bytes(),
            ));
            assets.push((
                "rss.xml".to_string(),
                page::generate_rss_feed(&pages, &site_title, base_url, desc, author).into_bytes(),
            ));
        }
    }

    SiteRender {
        pages: out_pages,
        assets,
        template_error,
        page_shell_errors,
        #[cfg(feature = "syntax-highlighting")]
        syntax_errors: syntaxes.warnings().to_vec(),
        #[cfg(not(feature = "syntax-highlighting"))]
        syntax_errors: Vec::new(),
        body_template_errors,
    }
}

// ── Per-page reconstruction ─────────────────────────────────────────────────

fn build_page(
    s: &SourceDoc,
    opts: &SiteOptions,
    path_to_filename: &HashMap<PathBuf, String>,
    title_map: &HashMap<PathBuf, String>,
    collected: &Collected,
    #[cfg(feature = "syntax-highlighting")] syntaxes: &crate::syntax::Syntaxes,
    reports: &mut Vec<String>,
) -> PublishedPage {
    let audience = opts.audience.as_deref();
    let parsed = frontmatter::parse_or_empty(&s.markdown).unwrap_or(frontmatter::ParsedFile {
        frontmatter: IndexMap::new(),
        body: s.markdown.clone(),
    });
    let fm = &parsed.frontmatter;

    let current_path = PathBuf::from(&s.path);
    let dest_filename = path_to_filename
        .get(&PathBuf::from(links::sanitize_rel_path(&s.path)))
        .cloned()
        .unwrap_or_else(|| dest_for(&s.path, s.is_root, fm));

    let title = frontmatter::get_string(fm, "title")
        .map(String::from)
        .unwrap_or_else(|| {
            Path::new(&s.path)
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

    // Resolve only against THIS audience's rendered set: a `contents`/`part_of`
    // link whose target was excluded for this audience is dropped, so it never
    // surfaces as a dead nav/breadcrumb entry that 404s. The render set is the
    // manifest — no separate per-audience list is needed.
    let contents_links: Vec<NavLink> = frontmatter::get_string_array(fm, "contents")
        .into_iter()
        .filter_map(|child| resolve_link(&child, &current_path, path_to_filename, title_map))
        .collect();

    let parent_link = frontmatter::get_string(fm, "part_of")
        .and_then(|p| resolve_link(p, &current_path, path_to_filename, title_map));

    let layout = PageLayout::parse(frontmatter::get_string(fm, "layout"));

    // The stored body is already visibility-filtered; template expansion still
    // needs to run (sources are stored pre-template). `template::render*`
    // re-applies visibility (a no-op now) and then expands the directives.
    //
    // A `verbatim` page skips it, as it skips everything else: a hand-authored
    // HTML file is a document someone designed, and rewriting anything inside it
    // is exactly the kind of help it asked not to be given.
    //
    // A template that will not expand still publishes its own source — there is
    // no better body to publish — but it no longer does so *quietly*. The page
    // names itself in `reports`, which `render_site` carries out as
    // `SiteRender::body_template_errors`, on the principle the shell templates
    // already hold to: silently serving the wrong thing is how a broken theme
    // survives a release.
    let file_path = Path::new(&s.path);
    let format = ContentFormat::from_extension(file_path).unwrap_or(ContentFormat::Markdown);
    let rendered_body = if layout.is_verbatim() {
        parsed.body.clone()
    } else {
        let values = page_context_values(
            s,
            fm,
            collected,
            &contents_links,
            parent_link.as_ref(),
            audience,
        );
        let context = template::Context::new(&collected.context, &values);
        let mut warnings = Vec::new();
        let rendered = match audience {
            Some(a) => {
                template::render_for_audiences(&parsed.body, format, context, &[a], &mut warnings)
            }
            None => template::render(&parsed.body, format, context, &mut warnings),
        };
        reports.extend(
            warnings
                .into_iter()
                .map(|w| format!("{}: {w}", current_path.display())),
        );
        match rendered {
            Ok(body) => body,
            Err(err) => {
                reports.push(format!(
                    "{}: {err} — the page is published as its own source",
                    current_path.display()
                ));
                parsed.body.clone()
            }
        }
    };

    // Body → HTML in the document's own grammar, then rewrite internal document
    // links. The empty workspace dir means canonical paths are used directly as
    // `path_to_filename` keys.
    //
    // The format is the *document's*, read off its extension, not the vault's
    // `content_format`: one site can hold a `.md` transcription beside the
    // `.html` artifact it transcribes, and each has to be parsed as what it is.
    // A path with no recognized extension falls back to Markdown, which is what
    // every document in every vault written before this was.
    //
    // A `verbatim` page takes neither step. Parsing and reserializing a file
    // someone designed returns a document that means the same and *is* not the
    // same, and rewriting links inside it would edit bytes it asked to have
    // copied — so its body passes through as written, and the rest of the
    // pipeline treats it as already-rendered HTML.
    let final_html = if layout.is_verbatim() {
        rendered_body.clone()
    } else {
        // The site's grammars, not the built-in set that plain `render_body`
        // reaches for: a site that declared one of its own declared it to be
        // used here.
        #[cfg(feature = "syntax-highlighting")]
        let converted = body::render_body_with(&rendered_body, format, syntaxes);
        #[cfg(not(feature = "syntax-highlighting"))]
        let converted = body::render_body(&rendered_body, format);
        links::transform_links(
            &converted,
            file_path,
            path_to_filename,
            Path::new(""),
            &dest_filename,
        )
    };

    let nav_order = fm.get("nav_order").and_then(|v| match v {
        YamlValue::Int(i) => Some(*i as i32),
        YamlValue::Float(f) => Some(*f as i32),
        YamlValue::String(st) => st.parse::<i32>().ok(),
        _ => None,
    });

    let created = frontmatter::get_string(fm, "created").map(String::from);
    let updated = frontmatter::get_string(fm, "updated").map(String::from);
    let date_of_document = frontmatter::get_string(fm, "date_of_document").map(String::from);
    // One line, because the chain, the grain and the multi-valued case are all
    // the view spec's to answer now — including the two spellings a field
    // permits (`people: Grandpa` and `people: [Grandpa, Nan]`), which prov
    // reads the same way.
    let group_keys = match &opts.arrangement {
        Arrangement::Containment => Vec::new(),
        Arrangement::Grouped(grouping) => grouping.keys_of(&YamlValue::Mapping(fm.clone())),
    };

    let styles = resolve_asset_paths(fm, "styles", &current_path);
    let scripts = resolve_asset_paths(fm, "scripts", &current_path);

    PublishedPage {
        source_path: current_path,
        dest_filename,
        title,
        rendered_body: final_html,
        markdown_body: rendered_body,
        contents_links,
        parent_link,
        is_root: s.is_root,
        description: frontmatter::get_string(fm, "description").map(String::from),
        author: frontmatter::get_string(fm, "author").map(String::from),
        created,
        updated,
        date_of_document,
        group_keys,
        attachments: frontmatter::get_string_array(fm, "attachments"),
        styles,
        scripts,
        layout,
        // Only for a page that wears a shell at all: `bare` and `verbatim` are
        // statements that this page carries its own frame, and recording a
        // request they cannot act on would report a missing template for a page
        // that was never going to use one.
        shell: match layout {
            PageLayout::Site => frontmatter::get_string(fm, "shell")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
            PageLayout::Bare | PageLayout::Verbatim => None,
        },
        nav_title: frontmatter::get_string(fm, "nav_title").map(String::from),
        nav_order,
        hide_from_nav: fm
            .get("hide_from_nav")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hide_from_feed: fm
            .get("hide_from_feed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        id: frontmatter::get_string(fm, "id").map(String::from),
        source_markdown: s.markdown.clone(),
    }
}

/// Resolve a frontmatter list of asset references (`styles`, `scripts`) into
/// paths below the site root.
///
/// A vault names a file either from its own root (`/assets/theme.css`, prov's
/// `path_style: root`) or relative to the document holding the reference
/// (`../assets/theme.css`); both spellings mean one file, and both arrive here.
/// Resolving them the way [`resolve_link`] resolves a `contents:` entry — and
/// the way [`crate::links::transform_links`] rebases an `<img src>` in the body
/// — is what makes a stylesheet named from a nested entry point at the same
/// object as one named from the front page.
///
/// The paths are **not** sanitized, matching how an attachment's own `src`
/// survives the body: a file keeps the name it has on disk, and the caller
/// copying it there is the same caller that copies attachments.
fn resolve_asset_paths(fm: &prov::Mapping, key: &str, current_relative: &Path) -> Vec<String> {
    frontmatter::get_string_array(fm, key)
        .iter()
        .filter_map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            let link = prov::Link::parse_path_only(trimmed);
            Some(
                prov::link::resolve(current_relative, &link.target)
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

// ── Generated index ─────────────────────────────────────────────────────────

/// The heading a page with nothing to group by is filed under.
const UNGROUPED: &str = "Other";

/// What a site is called when nobody said — no [`SiteOptions::site_title`], and
/// no authored root page to take a title from.
///
/// It is deliberately a word about the *thing* rather than about its front page.
/// The generated index answers to it too, so a site with no name reads
/// "Site" everywhere instead of disagreeing with itself; and a caller that
/// knows better — the CLI has the site's label, the server has its name —
/// should pass one rather than land here.
const DEFAULT_SITE_TITLE: &str = "Site";

/// Build a front page for a site whose render set contains none.
///
/// The page is a real [`PublishedPage`] with `is_root` set, so everything
/// downstream — nav, breadcrumbs, SEO, sitemap, feeds — treats it exactly like
/// an authored index and needs no special case. Its `contents_links` are the
/// entries in arrangement order, which is what makes
/// [`build_site_nav_tree`] hang the forest
/// underneath it.
///
/// Under [`Arrangement::Containment`] it lists the forest roots — the pages
/// whose parents audience filtering removed — and lets containment show the
/// rest. Under [`Arrangement::Grouped`] it lists every entry under its group's
/// heading, because a site that declared an arrangement asked to be read that
/// way rather than by hierarchy.
pub fn synthesize_index(pages: &[PublishedPage], opts: &SiteOptions) -> PublishedPage {
    // The site's title, not a title of its own: this page *is* the site's front
    // door, and naming it separately is what made a rootless site call itself
    // "Index" in its `<title>`, its `og:site_name` and its feeds.
    let title = opts
        .site_title
        .clone()
        .unwrap_or_else(|| DEFAULT_SITE_TITLE.to_string());

    let (body, links) = match &opts.arrangement {
        Arrangement::Containment => {
            let roots: Vec<&PublishedPage> = pages
                .iter()
                .filter(|p| !p.hide_from_nav && is_forest_root(p, pages))
                .collect();
            (render_entry_list(&roots), nav_links(&roots))
        }
        Arrangement::Grouped(grouping) => {
            let groups = group_entries(pages, grouping);
            let ordered: Vec<&PublishedPage> = groups
                .iter()
                .flat_map(|(_, ps)| ps.iter().copied())
                .collect();
            (render_groups(&groups), nav_links(&ordered))
        }
    };

    PublishedPage {
        source_path: PathBuf::from("index.md"),
        dest_filename: "index.html".to_string(),
        title,
        rendered_body: body,
        markdown_body: String::new(),
        contents_links: links,
        parent_link: None,
        is_root: true,
        description: None,
        author: None,
        created: None,
        updated: None,
        date_of_document: None,
        group_keys: Vec::new(),
        attachments: Vec::new(),
        styles: Vec::new(),
        scripts: Vec::new(),
        layout: PageLayout::default(),
        shell: None,
        nav_title: None,
        nav_order: None,
        hide_from_nav: false,
        // A generated index is a listing of entries that are themselves in the
        // feed; syndicating it too would put a duplicate of the whole site at
        // the top of every reader.
        hide_from_feed: true,
        // No ARK: nothing in the vault corresponds to this page, so there is no
        // document identity to mint one against.
        id: None,
        source_markdown: String::new(),
    }
}

/// Whether a page starts its own subtree — it names no parent, or names one
/// that audience filtering left out of this render set.
fn is_forest_root(page: &PublishedPage, pages: &[PublishedPage]) -> bool {
    match &page.parent_link {
        Some(link) => !pages.iter().any(|p| p.dest_filename == link.href),
        None => true,
    }
}

/// Group pages for a grouped arrangement. Date groups come back newest first,
/// field groups alphabetically; the ungrouped bucket is always last so a page
/// missing its grouping value is still reachable rather than dropped.
fn group_entries<'p>(
    pages: &'p [PublishedPage],
    grouping: &Grouping,
) -> Vec<(String, Vec<&'p PublishedPage>)> {
    let mut groups: BTreeMap<String, Vec<&PublishedPage>> = BTreeMap::new();
    let mut ungrouped: Vec<&PublishedPage> = Vec::new();

    for page in pages {
        if page.hide_from_nav {
            continue;
        }
        if page.group_keys.is_empty() {
            ungrouped.push(page);
            continue;
        }
        // A page with several values for the grouping field appears under each,
        // which is what a field lens means: filing under `people` puts a story
        // about two people in both their groups.
        for key in &page.group_keys {
            groups.entry(key.clone()).or_default().push(page);
        }
    }

    // A calendar reads newest-first, an A–Z index reads A-first. That used to
    // fall out of matching the `Date` variant; with the variant gone it is a
    // question about the *grain*, which is the more honest place for it — a view
    // over `taken_on` by month is just as chronological as one over `created`,
    // and the field name was never what made it so.
    let descending = matches!(grouping.by, Some(Grain::Year | Grain::Month | Grain::Day));
    let mut out: Vec<(String, Vec<&PublishedPage>)> = groups.into_iter().collect();
    if descending {
        out.reverse();
    }
    for (_, entries) in &mut out {
        sort_entries(entries, descending);
    }
    if !ungrouped.is_empty() {
        sort_entries(&mut ungrouped, descending);
        out.push((UNGROUPED.to_string(), ungrouped));
    }
    out
}

/// Sort entries within a group: by date when the arrangement is dated (newest
/// first, undated last), else by title.
///
/// The dated order is [`page::newest_first`] — the comparator the feeds use —
/// rather than a second implementation of it. Two orderings of one set of
/// entries is one ordering that will drift, and the drift shows up as a site
/// whose front page and whose feed disagree about what came first.
fn sort_entries(entries: &mut [&PublishedPage], by_date: bool) {
    if by_date {
        entries.sort_by(|a, b| page::newest_first(a, b));
    } else {
        entries.sort_by(|a, b| a.title.cmp(&b.title));
    }
}

// `cut_date` and `field_values` used to live here. `Grain::cut` and
// `Grouping::keys_of` are both, and are the ones the vault already uses — the
// date cut with its validation (`banana` at year grain is not the group `bana`)
// and the two field spellings permitted (`people: Grandpa` and
// `people: [Grandpa, Nan]`) included.

/// `<ul>` of links to entries.
fn render_entry_list(entries: &[&PublishedPage]) -> String {
    let mut out = String::from("<ul class=\"entry-list\">\n");
    for page in entries {
        out.push_str(&format!(
            "<li><a href=\"{}\">{}</a>{}</li>\n",
            page::html_escape(&page.dest_filename),
            page::html_escape(page.nav_title.as_deref().unwrap_or(&page.title)),
            match page.description.as_deref() {
                Some(d) if !d.is_empty() => format!(
                    " <span class=\"entry-description\">{}</span>",
                    page::html_escape(d)
                ),
                _ => String::new(),
            }
        ));
    }
    out.push_str("</ul>\n");
    out
}

/// A `<section>` per group, each with a heading and its entry list.
fn render_groups(groups: &[(String, Vec<&PublishedPage>)]) -> String {
    let mut out = String::new();
    for (label, entries) in groups {
        out.push_str(&format!(
            "<section class=\"entry-group\">\n<h2>{}</h2>\n",
            page::html_escape(label)
        ));
        out.push_str(&render_entry_list(entries));
        out.push_str("</section>\n");
    }
    out
}

/// Nav links to entries, in the order given.
fn nav_links(entries: &[&PublishedPage]) -> Vec<NavLink> {
    entries
        .iter()
        .map(|p| NavLink {
            href: p.dest_filename.clone(),
            title: p.nav_title.clone().unwrap_or_else(|| p.title.clone()),
        })
        .collect()
}

/// Resolve a `contents`/`part_of` link string to a [`NavLink`] whose href is the
/// target's output `.html` filename and whose title comes from the target's
/// frontmatter or the link text.
///
/// Returns `None` when the target is not part of the current render set (e.g.
/// excluded by audience visibility). Dropping it keeps nav/breadcrumbs limited
/// to pages that actually exist for this audience — without any separate
/// manifest, since `path_to_filename` already is the rendered-page set.
fn resolve_link(
    link_str: &str,
    current_relative: &Path,
    path_to_filename: &HashMap<PathBuf, String>,
    title_map: &HashMap<PathBuf, String>,
) -> Option<NavLink> {
    let link = prov::Link::parse_path_only(link_str.trim());
    let canonical = prov::link::resolve(current_relative, &link.target)
        .to_string_lossy()
        .into_owned();
    // Sanitize so links carrying unsanitized characters resolve against the
    // sanitized source-path keys.
    let key = PathBuf::from(links::sanitize_rel_path(&canonical));

    let href = path_to_filename.get(&key)?.clone();

    let title = title_map
        .get(&key)
        .cloned()
        .or_else(|| link.label.clone())
        .unwrap_or_else(|| filename_to_title(&canonical));

    Some(NavLink { href, title })
}

// ── Filename helpers (ported from the publish plugin) ────────────────────────

/// Convert a canonical `.md` path to its sanitized `.html` output filename.
///
/// Public because a caller that must know where a source's HTML lands *before*
/// rendering it has no other way to ask: `build_pages` applies this same rule
/// internally, and re-deriving it elsewhere is how the two drift apart.
pub fn output_filename(canonical_md: &str) -> String {
    let with_ext = Path::new(canonical_md).with_extension("html");
    let sanitized: PathBuf = with_ext
        .components()
        .map(|c| match c {
            std::path::Component::Normal(s) => {
                std::ffi::OsString::from(links::sanitize_path_component(&s.to_string_lossy()))
            }
            other => other.as_os_str().to_owned(),
        })
        .collect();
    sanitized.to_string_lossy().into_owned()
}

/// Where one source publishes: `index.html` for the site's front page, the
/// destination its frontmatter `serve_at:` claims, else [`output_filename`].
///
/// The single rule, so a caller that must know a page's destination before the
/// render — the server, naming the object it will write; the publish client,
/// naming the key it uploads against — asks rather than re-derives. Two
/// derivations of one filename is one filename that will drift, and the drift
/// is a page whose ARK resolves to an object nothing wrote.
pub fn dest_of(source: &SourceDoc) -> String {
    let fm = frontmatter::parse_or_empty(&source.markdown)
        .map(|parsed| parsed.frontmatter)
        .unwrap_or_default();
    dest_for(&source.path, source.is_root, &fm)
}

/// [`dest_of`] for a caller that has already parsed the metadata block.
fn dest_for(path: &str, is_root: bool, fm: &prov::Mapping) -> String {
    // The site's front page is its front door rather than a page with an
    // address of its own, so a `serve_at:` on it has nothing to claim.
    if is_root {
        return "index.html".to_string();
    }
    frontmatter::get_string(fm, "serve_at")
        .and_then(serve_at_dest)
        .unwrap_or_else(|| output_filename(path))
}

/// Convert a filename to a display title (snake/kebab case → Title Case).
fn filename_to_title(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    humanize_name(stem)
}

/// Turn a machine name into something to show a reader: `family-letters` →
/// `Family Letters`.
///
/// Public because a site's *name* needs the same treatment as a document's
/// filename, and the callers that hold one — the server, which knows a site
/// only by the segment it is served under — are outside this crate. A caller
/// with a real label should pass that instead.
pub fn humanize_name(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars: Vec<char> = word.chars().collect();
            if let Some(first) = chars.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            chars.into_iter().collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(path: &str, markdown: &str, is_root: bool) -> SourceDoc {
        SourceDoc {
            path: path.to_string(),
            markdown: markdown.to_string(),
            is_root,
        }
    }

    /// The date chain these tests declare for their own views, written out
    /// here because it is a declaration a vault makes rather than something
    /// this crate knows: the layer that owns a vault's vocabulary names the
    /// same three fields, above this one.
    fn date_grouping(by: Grain) -> Grouping {
        Grouping {
            keys: ["date_of_document", "created", "updated"]
                .iter()
                .map(|k| (*k).to_string())
                .collect(),
            by: Some(by),
        }
    }

    #[test]
    fn output_filename_sanitizes_and_sets_html() {
        assert_eq!(output_filename("notes/My Note!.md"), "notes/My Note.html");
        assert_eq!(output_filename("a/b/c.md"), "a/b/c.html");
    }

    /// Every content format lands on `.html`, and a source that is already
    /// `.html` keeps its name rather than gaining a second extension.
    #[test]
    fn output_filename_covers_every_content_format() {
        assert_eq!(output_filename("notes/entry.dj"), "notes/entry.html");
        assert_eq!(output_filename("notes/entry.djot"), "notes/entry.html");
        assert_eq!(
            output_filename("notes/artifact.html"),
            "notes/artifact.html"
        );
        assert_eq!(output_filename("notes/artifact.htm"), "notes/artifact.html");
    }

    /// One site, three grammars — the case a vault reaches by importing an
    /// `.html` artifact next to the `.md` note that describes it. Each body is
    /// parsed as what its extension says it is, and the links between them are
    /// rewritten regardless of which format either end is written in.
    #[test]
    fn a_site_may_mix_content_formats() {
        let index = "---\ntitle: Home\ncontents:\n  - \"/note.dj\"\n  - \"/artifact.html\"\n---\nSee [the note](/note.dj).\n";
        let note = "---\ntitle: Note\npart_of: \"/index.md\"\n---\nA _djot_ note with a ==highlight== and a [link](/artifact.html).\n";
        let artifact =
            "---\ntitle: Artifact\npart_of: \"/index.md\"\n---\n<p>Already <em>HTML</em>.</p>\n";

        let sources = vec![
            src("index.md", index, true),
            src("note.dj", note, false),
            src("artifact.html", artifact, false),
        ];
        let pages = build_pages(&sources, &SiteOptions::default());

        let note_page = pages.iter().find(|p| p.title == "Note").unwrap();
        assert!(
            note_page.rendered_body.contains("<em>djot</em>"),
            "djot emphasis is `_x_`, which Markdown would not have italicized: {}",
            note_page.rendered_body
        );
        assert!(
            note_page.rendered_body.contains("highlight-mark"),
            "Diaryx's custom syntax works in djot too: {}",
            note_page.rendered_body
        );
        assert!(
            note_page.rendered_body.contains(r#"href="artifact.html""#),
            "a djot link to an html document is rewritten: {}",
            note_page.rendered_body
        );

        let artifact_page = pages.iter().find(|p| p.title == "Artifact").unwrap();
        assert!(artifact_page.rendered_body.contains("<em>HTML</em>"));

        // The root's link to the `.dj` note resolves to the note's dest page,
        // which is the half `md_link_canonical`'s `.md` test used to miss.
        let home = pages.iter().find(|p| p.is_root).unwrap();
        assert!(
            home.rendered_body.contains(r#"href="note.html""#),
            "got {}",
            home.rendered_body
        );
        assert_eq!(home.contents_links.len(), 2);
    }

    #[test]
    fn filename_to_title_titlecases() {
        assert_eq!(filename_to_title("hello-world.md"), "Hello World");
        assert_eq!(filename_to_title("my_cool_note.md"), "My Cool Note");
    }

    #[test]
    fn build_pages_derives_graph_and_renders() {
        let index = "---\ntitle: Home\ncontents:\n  - \"[Child](/child.md)\"\n---\nWelcome to :val[title].\n";
        let child = "---\ntitle: Child Page\npart_of: \"/index.md\"\n---\nSee [home](/index.md) and a ==highlight==.\n";

        let sources = vec![src("index.md", index, true), src("child.md", child, false)];
        let pages = build_pages(&sources, &SiteOptions::default());

        let home = pages.iter().find(|p| p.is_root).unwrap();
        let kid = pages.iter().find(|p| !p.is_root).unwrap();

        // dest filenames
        assert_eq!(home.dest_filename, "index.html");
        assert_eq!(kid.dest_filename, "child.html");

        // the value directive resolved against frontmatter
        assert!(home.rendered_body.contains("Welcome to Home."));

        // contents_links resolved to child's html + frontmatter title
        assert_eq!(home.contents_links.len(), 1);
        assert_eq!(home.contents_links[0].href, "child.html");
        assert_eq!(home.contents_links[0].title, "Child Page");

        // parent_link resolves back to index
        let parent = kid.parent_link.as_ref().unwrap();
        assert_eq!(parent.href, "index.html");
        assert_eq!(parent.title, "Home");

        // internal .md link rewritten to .html, and custom syntax expanded
        assert!(kid.rendered_body.contains(r#"href="index.html""#));
        assert!(kid.rendered_body.contains("highlight-mark"));
    }

    #[test]
    fn root_by_workspace_name_and_special_chars_resolve() {
        // Option 1: sources keyed by workspace path; root keeps its real name
        // ("Welcome.md"), not "index". Child links reference workspace paths,
        // including a special character that the dest sanitizer strips.
        let root = "---\ntitle: Home\ncontents:\n  - \"/My Note!.md\"\n---\nHi.\n";
        let note = "---\ntitle: My Note\npart_of: \"/Welcome.md\"\n---\nBody.\n";

        let sources = vec![
            src("Welcome.md", root, true),
            src("My Note.md", note, false), // stored under sanitized workspace path
        ];
        let pages = build_pages(&sources, &SiteOptions::default());

        let home = pages.iter().find(|p| p.is_root).unwrap();
        let note_page = pages.iter().find(|p| !p.is_root).unwrap();

        // Root renders to index.html despite its workspace name.
        assert_eq!(home.dest_filename, "index.html");
        // Child's contents link (with "!") resolves to the sanitized dest + title.
        assert_eq!(home.contents_links.len(), 1);
        assert_eq!(home.contents_links[0].href, "My Note.html");
        assert_eq!(home.contents_links[0].title, "My Note");
        // Child's part_of points at the root by its workspace name → index.html.
        let parent = note_page.parent_link.as_ref().unwrap();
        assert_eq!(parent.href, "index.html");
        assert_eq!(parent.title, "Home");
    }

    #[test]
    fn contents_link_to_excluded_page_is_dropped() {
        // The root lists two children, but only one is in the render set (the
        // other was excluded for this audience). Nav/contents must not link to
        // the missing page — that's the source-side cause of the 404 sidebar
        // entries.
        let index = "---\ntitle: Home\ncontents:\n  - \"/public-child.md\"\n  - \"/private-child.md\"\n---\nHi.\n";
        let public_child = "---\ntitle: Public Child\npart_of: \"/index.md\"\n---\nBody.\n";

        let sources = vec![
            src("index.md", index, true),
            src("public-child.md", public_child, false),
        ];
        let pages = build_pages(&sources, &SiteOptions::default());

        let home = pages.iter().find(|p| p.is_root).unwrap();
        assert_eq!(home.contents_links.len(), 1, "excluded child dropped");
        assert_eq!(home.contents_links[0].href, "public-child.html");

        // And the rendered nav reflects only the included child.
        let out = render_site(&sources, &SiteOptions::default());
        let home_html = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home_html.html.contains("public-child.html"));
        assert!(!home_html.html.contains("private-child.html"));
    }

    #[test]
    fn parent_link_to_excluded_page_is_dropped() {
        // A page whose parent was excluded for this audience must not carry a
        // dead breadcrumb/parent link.
        let index = "---\ntitle: Home\n---\nHi.\n";
        let orphan = "---\ntitle: Orphan\npart_of: \"/excluded.md\"\n---\nBody.\n";

        let sources = vec![
            src("index.md", index, true),
            src("orphan.md", orphan, false),
        ];
        let pages = build_pages(&sources, &SiteOptions::default());

        let orphan_page = pages
            .iter()
            .find(|p| p.dest_filename == "orphan.html")
            .unwrap();
        assert!(orphan_page.parent_link.is_none());
    }

    // ── generated index ─────────────────────────────────────────────────────

    fn entry(title: &str, date: &str) -> String {
        format!("---\ntitle: {title}\ndate_of_document: {date}\n---\nBody of {title}.\n")
    }

    /// A grammar the site declared reaches the pages it publishes — the whole
    /// point of [`SiteOptions::syntaxes`], and the one step that is neither
    /// `syntax`'s nor `body`'s to test.
    #[cfg(feature = "syntax-highlighting")]
    #[test]
    fn a_declared_grammar_colours_the_sites_code() {
        let note = "---\ntitle: Note\n---\n```wat\n;; a note\n```\n";
        let mut opts = SiteOptions::default();
        opts.syntaxes.insert(
            ".config/sites/blog/wat.sublime-syntax".to_string(),
            "name: Wat\nfile_extensions: [wat]\nscope: source.wat\ncontexts:\n  main:\n    \
             - match: ';;.*$'\n      scope: comment.line.wat\n"
                .to_string(),
        );

        let out = render_site(&[src("index.md", note, true)], &opts);
        assert!(out.syntax_errors.is_empty(), "{:?}", out.syntax_errors);
        assert!(
            out.pages[0].html.contains("plates-comment"),
            "the site's own grammar did not reach the page: {}",
            out.pages[0].html
        );
    }

    /// And one that will not parse costs the site some colour rather than its
    /// publication — the bargain a broken shell template gets.
    #[cfg(feature = "syntax-highlighting")]
    #[test]
    fn a_broken_declared_grammar_is_reported_not_fatal() {
        let note = "---\ntitle: Note\n---\n```rust\nlet x = 1;\n```\n";
        let mut opts = SiteOptions::default();
        opts.syntaxes.insert(
            ".config/sites/blog/broken.sublime-syntax".to_string(),
            "this: is: not: a grammar".to_string(),
        );

        let out = render_site(&[src("index.md", note, true)], &opts);
        assert_eq!(out.syntax_errors.len(), 1, "{:?}", out.syntax_errors);
        assert!(
            out.syntax_errors[0].contains("broken.sublime-syntax"),
            "names the file: {:?}",
            out.syntax_errors
        );
        assert!(
            out.pages[0].html.contains("plates-storage"),
            "rust still highlights: {}",
            out.pages[0].html
        );
    }

    /// The case per-file audiences create: three entries tagged for a site,
    /// none of them the vault's (private) root. There is no page to promote, so
    /// the render synthesizes one rather than crowning whichever entry sorted
    /// first.
    #[test]
    fn a_rootless_set_gets_a_generated_index() {
        let sources = vec![
            src("mon.md", &entry("Monday", "2026-07-27"), false),
            src("tue.md", &entry("Tuesday", "2026-07-28"), false),
        ];

        let out = render_site(&sources, &SiteOptions::default());

        let index = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .expect("a synthesized index");
        assert!(index.html.contains("mon.html"));
        assert!(index.html.contains("tue.html"));
        assert!(index.id.is_none(), "nothing in the vault to identify");
        assert_eq!(out.pages.len(), 3, "the index plus both entries");
    }

    /// …but not when the caller is supplying the front page itself. A site
    /// fronted by a covered directory publishes an authored `index.html` that
    /// never passes through this crate, so from here it looks exactly like a
    /// rootless set — and generating one anyway writes it straight over the
    /// page the site was fronted with.
    #[test]
    fn a_supplied_front_page_is_not_generated_over() {
        let sources = vec![
            src("mon.md", &entry("Monday", "2026-07-27"), false),
            src("tue.md", &entry("Tuesday", "2026-07-28"), false),
        ];

        let out = render_site(
            &sources,
            &SiteOptions {
                site_title: Some("Diaryx".to_string()),
                front_page_supplied: true,
                ..SiteOptions::default()
            },
        );

        assert!(
            !out.pages.iter().any(|p| p.dest_filename == "index.html"),
            "the render must leave the site's root key alone"
        );
        assert_eq!(out.pages.len(), 2, "the entries, and nothing invented");
        // The entries still render, still know the site's name, and still link
        // to each other — a supplied front page removes a page, not a site.
        let mon = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "mon.html")
            .expect("the entries still render");
        assert!(mon.html.contains("<title>Monday - Diaryx<"));
    }

    /// A feed is absolute URLs, so a render with no base URL writes none —
    /// and, since these two were separate conditions, used to put a
    /// `<link rel="alternate">` on every page pointing at the `feed.xml` it
    /// had just skipped. Every published site was in that state, because the
    /// base URL came from a parameter no client sent.
    #[test]
    fn a_site_with_no_base_url_advertises_no_feed() {
        let sources = vec![src("mon.md", &entry("Monday", "2026-07-27"), false)];
        let out = render_site(&sources, &SiteOptions::default());

        assert!(
            !out.assets
                .iter()
                .any(|(n, _)| n == "feed.xml" || n == "rss.xml"),
            "no absolute URL to write them against"
        );
        for page in &out.pages {
            assert!(
                !page.html.contains("rel=\"alternate\""),
                "nor anything to advertise: {}",
                page.dest_filename
            );
        }
    }

    /// A site with no authored root used to be named after the index
    /// synthesized for it, so every page announced the site as "Index" — in its
    /// `<title>`, its `og:site_name` and both feeds.
    #[test]
    fn a_rootless_site_is_not_named_after_its_generated_index() {
        let sources = vec![src("mon.md", &entry("Monday", "2026-07-27"), false)];
        let opts = SiteOptions {
            site_title: Some("Family Letters".to_string()),
            base_url: Some("https://example.test".to_string()),
            ..SiteOptions::default()
        };

        let out = render_site(&sources, &opts);
        let entry_page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "mon.html")
            .unwrap();
        assert!(entry_page.html.contains("<title>Monday - Family Letters<"));
        assert!(
            entry_page
                .html
                .contains(r#"og:site_name" content="Family Letters""#)
        );
        assert!(!entry_page.html.contains("Index"));

        // The generated front page answers to the site's name rather than
        // inventing one, so it does not disagree with the pages under it.
        let index = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(index.html.contains("<title>Family Letters</title>"));

        let feed = out
            .assets
            .iter()
            .find(|(n, _)| n == "feed.xml")
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .unwrap();
        assert!(feed.contains("<title>Family Letters</title>"));
    }

    /// Told nothing, a site is called something about itself rather than about
    /// its front page — and still agrees with its own index.
    #[test]
    fn an_unnamed_rootless_site_falls_back_to_one_word_everywhere() {
        let sources = vec![src("mon.md", &entry("Monday", "2026-07-27"), false)];
        let out = render_site(&sources, &SiteOptions::default());

        let index = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(index.html.contains("<title>Site</title>"));
        let entry_page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "mon.html")
            .unwrap();
        assert!(entry_page.html.contains("<title>Monday - Site<"));
    }

    /// An authored root still names the site — the fix is about where the name
    /// comes from when there is no such page, not about overriding one.
    #[test]
    fn an_authored_root_still_names_the_site() {
        let root = "---\ntitle: Home\n---\nHand written.\n";
        let sources = vec![
            src("index.md", root, true),
            src("mon.md", &entry("Monday", "2026-07-27"), false),
        ];

        let out = render_site(&sources, &SiteOptions::default());
        let entry_page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "mon.html")
            .unwrap();
        assert!(entry_page.html.contains("<title>Monday - Home<"));
    }

    #[test]
    fn humanize_name_title_cases_a_machine_name() {
        assert_eq!(humanize_name("family-letters"), "Family Letters");
        assert_eq!(humanize_name("blog"), "Blog");
    }

    /// An authored index is left alone — synthesis is the fallback, not the rule.
    #[test]
    fn an_authored_index_is_not_replaced() {
        let root = "---\ntitle: Home\n---\nHand written.\n";
        let sources = vec![
            src("index.md", root, true),
            src("mon.md", &entry("Monday", "2026-07-27"), false),
        ];

        let out = render_site(&sources, &SiteOptions::default());
        let index = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(index.html.contains("Hand written."));
        assert_eq!(out.pages.len(), 2);
    }

    /// A dated arrangement groups by the grain and puts the newest group first —
    /// the ordering a journal wants when hierarchy is not what organizes it.
    #[test]
    fn a_dated_arrangement_groups_newest_first() {
        let sources = vec![
            src("old.md", &entry("Old", "2024-01-02"), false),
            src("new.md", &entry("New", "2026-07-27"), false),
            src("mid.md", &entry("Mid", "2025-05-05"), false),
        ];
        let opts = SiteOptions {
            arrangement: Arrangement::Grouped(date_grouping(Grain::Year)),
            ..SiteOptions::default()
        };

        let pages = build_pages(&sources, &opts);
        let index = synthesize_index(&pages, &opts);

        let order: Vec<&str> = index
            .contents_links
            .iter()
            .map(|l| l.href.as_str())
            .collect();
        assert_eq!(order, ["new.html", "mid.html", "old.html"]);

        let y26 = index.rendered_body.find("2026").expect("a 2026 heading");
        let y25 = index.rendered_body.find("2025").expect("a 2025 heading");
        let y24 = index.rendered_body.find("2024").expect("a 2024 heading");
        assert!(y26 < y25 && y25 < y24, "groups run newest to oldest");
    }

    /// Month grain cuts the same ISO prefix the app's lens does.
    #[test]
    fn a_month_grain_cuts_to_the_month() {
        let sources = vec![
            src("a.md", &entry("A", "2026-07-27"), false),
            src("b.md", &entry("B", "2026-08-01"), false),
        ];
        let opts = SiteOptions {
            arrangement: Arrangement::Grouped(date_grouping(Grain::Month)),
            ..SiteOptions::default()
        };
        let index = synthesize_index(&build_pages(&sources, &opts), &opts);
        assert!(index.rendered_body.contains("2026-08"));
        assert!(index.rendered_body.contains("2026-07"));
    }

    /// A field arrangement groups by the field's values, accepting both the
    /// scalar and the list spelling, and files a document under each value it
    /// carries.
    #[test]
    fn a_field_arrangement_groups_by_value() {
        let scalar = "---\ntitle: Lunch\npeople: Nan\n---\nBody.\n";
        let list = "---\ntitle: Trip\npeople:\n  - Nan\n  - Grandpa\n---\nBody.\n";
        let sources = vec![src("lunch.md", scalar, false), src("trip.md", list, false)];
        let opts = SiteOptions {
            arrangement: Arrangement::Grouped(Grouping::field("people")),
            ..SiteOptions::default()
        };

        let pages = build_pages(&sources, &opts);
        assert_eq!(
            pages
                .iter()
                .find(|p| p.title == "Lunch")
                .unwrap()
                .group_keys,
            vec!["Nan".to_string()],
            "a scalar field value groups like a one-element list"
        );

        let index = synthesize_index(&pages, &opts);
        assert!(index.rendered_body.contains("<h2>Grandpa</h2>"));
        assert!(index.rendered_body.contains("<h2>Nan</h2>"));
        // The trip is filed under both people it names.
        let trips = index.rendered_body.matches("trip.html").count();
        assert_eq!(trips, 2, "one entry per group it belongs to");
    }

    /// A page carrying nothing to group by lands in a bucket rather than being
    /// dropped: an entry missing its date must still be reachable.
    #[test]
    fn an_entry_with_no_grouping_value_is_still_listed() {
        let sources = vec![
            src("dated.md", &entry("Dated", "2026-07-27"), false),
            src("undated.md", "---\ntitle: Undated\n---\nBody.\n", false),
        ];
        let opts = SiteOptions {
            arrangement: Arrangement::Grouped(date_grouping(Grain::Year)),
            ..SiteOptions::default()
        };
        let index = synthesize_index(&build_pages(&sources, &opts), &opts);
        assert!(index.rendered_body.contains("undated.html"));
        assert!(index.rendered_body.contains(UNGROUPED));
    }

    /// The generated index lists the entries; syndicating it as well would put
    /// a copy of the whole site at the top of every reader.
    #[test]
    fn a_generated_index_stays_out_of_the_feed() {
        let sources = vec![src("mon.md", &entry("Monday", "2026-07-27"), false)];
        let opts = SiteOptions {
            base_url: Some("https://example.test".to_string()),
            ..SiteOptions::default()
        };
        let index = synthesize_index(&build_pages(&sources, &opts), &opts);
        assert!(index.hide_from_feed);

        let out = render_site(&sources, &opts);
        let feed = out
            .assets
            .iter()
            .find(|(n, _)| n == "feed.xml")
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .expect("a feed");
        assert!(feed.contains("mon.html"));
        assert!(!feed.contains("index.html"), "the index is not an entry");
    }

    /// Under a containment arrangement the generated index lists the forest
    /// roots and lets hierarchy show the rest — it does not flatten a vault
    /// that still has a shape.
    #[test]
    fn a_containment_index_lists_the_forest_roots() {
        let parent_doc = "---\ntitle: Daily\ncontents:\n  - \"/mon.md\"\n---\nBody.\n";
        let child = "---\ntitle: Monday\npart_of: \"/daily.md\"\n---\nBody.\n";
        let loose = "---\ntitle: Loose\n---\nBody.\n";
        let sources = vec![
            src("daily.md", parent_doc, false),
            src("mon.md", child, false),
            src("loose.md", loose, false),
        ];

        let opts = SiteOptions::default();
        let index = synthesize_index(&build_pages(&sources, &opts), &opts);

        let listed: Vec<&str> = index
            .contents_links
            .iter()
            .map(|l| l.href.as_str())
            .collect();
        assert_eq!(
            listed,
            ["daily.html", "loose.html"],
            "the nested child is reached through its parent, not listed twice"
        );

        // And the rendered nav nests the child under its parent.
        let out = render_site(&sources, &opts);
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home.html.contains("mon.html"), "still reachable in nav");
    }

    // ── Shell template, layout, per-page assets ─────────────────────────────

    /// A shell template replaces the built-in document. The slots it fills are
    /// the ones the built-in one fills, so nothing about the page is invisible
    /// to it — including the nav, which the entries still appear in.
    #[test]
    fn a_template_replaces_the_built_in_shell() {
        let index = "---\ntitle: Home\ncontents:\n  - \"/child.md\"\n---\nHi.\n";
        let child = "---\ntitle: Child\npart_of: \"/index.md\"\n---\nKid.\n";
        let sources = vec![src("index.md", index, true), src("child.md", child, false)];

        let out = render_site(
            &sources,
            &SiteOptions {
                template: Some(
                    "<!DOCTYPE html>\n<html lang=\"{{lang}}\"><head><title>{{document_title}}</title>{{{head}}}</head>\
                     <body class=\"{{body_class}}\">{{{site_nav}}}<article>{{{content}}}</article>{{{scripts}}}</body></html>"
                        .to_string(),
                ),
                lang: "cy".to_string(),
                ..SiteOptions::default()
            },
        );

        assert!(out.template_error.is_none(), "{:?}", out.template_error);
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home.html.contains(r#"<html lang="cy">"#));
        assert!(
            home.html.contains("<title>Home</title>"),
            "got {}",
            home.html
        );
        assert!(home.html.contains(r#"<body class="has-site-nav">"#));
        assert!(home.html.contains("child.html"), "the nav is still there");
        assert!(
            !home.html.contains(r#"<div class="site-content">"#),
            "and the built-in furniture the template did not ask for is not"
        );
    }

    /// A theme that will not compile costs the site its design, not its
    /// publication — and says why.
    #[test]
    fn a_broken_template_falls_back_and_reports_itself() {
        let sources = vec![src("index.md", "---\ntitle: Home\n---\nHi.\n", true)];
        let out = render_site(
            &sources,
            &SiteOptions {
                template: Some("<html>{{contnet}}</html>".to_string()),
                ..SiteOptions::default()
            },
        );

        let error = out.template_error.expect("the reason it was ignored");
        assert!(error.contains("unknown shell slot `contnet`"), "{error}");
        assert!(
            out.pages[0].html.contains(r#"<div class="site-content">"#),
            "the built-in shell"
        );
    }

    /// `layout: bare` is a page that carries its own design. It still belongs to
    /// the site — nav, sitemap and feeds all know it — it just is not wearing
    /// the site's frame.
    #[test]
    fn a_bare_page_keeps_its_place_in_the_site() {
        let index = "---\ntitle: Home\ncontents:\n  - \"/poster.md\"\n---\nHi.\n";
        let poster = "---\ntitle: Poster\npart_of: \"/index.md\"\nlayout: bare\nstyles:\n  - \"/assets/poster.css\"\n---\nArt.\n";
        let sources = vec![
            src("index.md", index, true),
            src("poster.md", poster, false),
        ];

        let out = render_site(&sources, &SiteOptions::default());
        let bare = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "poster.html")
            .unwrap();
        assert!(bare.html.starts_with("<!DOCTYPE html>"));
        assert!(bare.html.contains(r#"href="assets/poster.css""#));
        assert!(!bare.html.contains("site-nav"), "no frame: {}", bare.html);
        assert!(!bare.html.contains("style.css"), "no site stylesheet");

        // …and the site still lists it.
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home.html.contains("poster.html"));
    }

    /// `layout: verbatim` publishes the body unread. Asserted as an equality
    /// over a whole document rather than a handful of `contains`, because the
    /// promise is about *bytes*: everything twig would change on the way through
    /// — attribute order, void-tag spelling, entity normalization, the exact
    /// whitespace of an inline `<script>` — is a difference this test exists to
    /// catch and no substring assertion can see.
    #[test]
    fn a_verbatim_page_is_published_byte_for_byte() {
        // Every shape the pipeline would otherwise touch: a `{{handlebars}}`
        // expression, a `.md` link, a vault-root-absolute asset path, an
        // `==highlight==`, an `![embed](x.html)`, a script full of braces and
        // angle brackets, an unclosed void tag, and single-quoted attributes.
        let mut body = String::from(
            "<!doctype html>\n\
             <html lang=\"en\" data-theme='dark'>\n\
             <head>\n\
             <meta charset=utf-8>\n\
             <title>Diaryx — {{ not a template }}</title>\n\
             <style>.a{color:red}.b{color:blue}</style>\n\
             </head>\n\
             <body>\n\
             <img src=\"/img/hero.png\" alt=\"a ==highlight== and ![an](embed.html)\">\n\
             <a href=\"/about.md\">about</a>\n\
             <script>if (a<b && c>d) { f({x: 1}); }</script>\n",
        );
        for i in 0..400 {
            body.push_str(&format!(
                "<p class='row' data-i={i}>Line {i} &amp; more<br>\n"
            ));
        }
        body.push_str("</body>\n</html>\n");

        let source = format!("---\ntitle: Front\nlayout: verbatim\n---\n{body}");
        let sources = vec![src("index.md", &source, true)];
        let out = render_site(&sources, &SiteOptions::default());

        let page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert_eq!(page.html, body, "the body is the file");
    }

    /// A verbatim page is still a page: the site knows it, links to it, and
    /// syndicates it exactly as it would any other. `verbatim` is a statement
    /// about the bytes, not about membership.
    #[test]
    fn a_verbatim_page_keeps_its_place_in_the_site() {
        let index = "---\ntitle: Home\ncontents:\n  - \"/landing.md\"\n---\nHi.\n";
        let landing =
            "---\ntitle: Landing\npart_of: \"/index.md\"\nlayout: verbatim\n---\n<h1>Hi</h1>\n";
        let sources = vec![
            src("index.md", index, true),
            src("landing.md", landing, false),
        ];

        let opts = SiteOptions {
            base_url: Some("https://example.test".to_string()),
            ..SiteOptions::default()
        };
        let out = render_site(&sources, &opts);

        let landing_page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "landing.html")
            .unwrap();
        assert_eq!(landing_page.html, "<h1>Hi</h1>\n");

        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home.html.contains("landing.html"), "listed in the nav");

        let sitemap = out
            .assets
            .iter()
            .find(|(n, _)| n == "sitemap.xml")
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .expect("a sitemap");
        assert!(sitemap.contains("landing.html"));
    }

    /// `styles:`/`scripts:` are asset references like any other: written from
    /// the vault root or relative to the document, and resolved to one path
    /// below the site root — then rebased to the depth of each page that names
    /// them, so a nested entry and the front page point at the same file.
    #[test]
    fn page_assets_resolve_and_rebase_like_attachments() {
        let front = "---\ntitle: Home\nstyles:\n  - \"/assets/site.css\"\nscripts:\n  - \"assets/site.js\"\n---\nHi.\n";
        let deep = "---\ntitle: Deep\nstyles:\n  - \"../assets/site.css\"\n---\nBody.\n";
        let sources = vec![
            src("index.md", front, true),
            src("notes/deep.md", deep, false),
        ];

        let out = render_site(&sources, &SiteOptions::default());

        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert_eq!(home.styles, vec!["assets/site.css".to_string()]);
        assert_eq!(home.scripts, vec!["assets/site.js".to_string()]);
        assert!(
            home.html
                .contains(r#"<link rel="stylesheet" href="assets/site.css">"#)
        );
        assert!(
            home.html
                .contains(r#"<script defer src="assets/site.js"></script>"#)
        );

        let deep_page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "notes/deep.html")
            .unwrap();
        assert_eq!(
            deep_page.styles,
            vec!["assets/site.css".to_string()],
            "one file, however the document spelled its way to it"
        );
        assert!(
            deep_page
                .html
                .contains(r#"<link rel="stylesheet" href="../assets/site.css">"#),
            "rebased to the page's own depth: {}",
            deep_page.html
        );
    }

    // ── `serve_at` ──────────────────────────────────────────────────────────

    /// The normalizations, one by one: a leading `/` is required, `.html` is
    /// implied, components are sanitized, and nothing reaches above the site
    /// root.
    #[test]
    fn serve_at_normalizes_a_site_root_claim() {
        assert_eq!(serve_at_dest("/privacy"), Some("privacy.html".to_string()));
        assert_eq!(
            serve_at_dest("/privacy.html"),
            Some("privacy.html".to_string()),
            "the two spellings are one claim"
        );
        assert_eq!(
            serve_at_dest("  /legal/privacy  "),
            Some("legal/privacy.html".to_string())
        );
        assert_eq!(
            serve_at_dest("/My Page!"),
            Some("My Page.html".to_string()),
            "sanitized like every other published path"
        );
        assert_eq!(
            serve_at_dest("/../../etc/passwd"),
            Some("etc/passwd.html".to_string()),
            "there is nothing above a site's root to reach"
        );
        // Not a claim at all.
        assert_eq!(serve_at_dest("privacy.html"), None, "must be site-absolute");
        assert_eq!(serve_at_dest("/"), None);
        assert_eq!(serve_at_dest(""), None);
    }

    /// A document declaring `serve_at:` publishes where it says, and everything
    /// downstream follows it: the nav, the body links that point at it, the
    /// sitemap.
    #[test]
    fn a_serve_at_page_publishes_where_it_claims() {
        let index = "---\ntitle: Home\ncontents:\n  - \"/docs/privacy.md\"\n---\nSee [the policy](/docs/privacy.md).\n";
        let privacy =
            "---\ntitle: Privacy\npart_of: \"/index.md\"\nserve_at: /privacy\n---\nThe policy.\n";
        let sources = vec![
            src("index.md", index, true),
            src("docs/privacy.md", privacy, false),
        ];

        let opts = SiteOptions {
            base_url: Some("https://example.test".to_string()),
            ..SiteOptions::default()
        };
        let pages = build_pages(&sources, &opts);
        let page = pages.iter().find(|p| p.title == "Privacy").unwrap();
        assert_eq!(page.dest_filename, "privacy.html");

        let home = pages.iter().find(|p| p.is_root).unwrap();
        assert_eq!(home.contents_links[0].href, "privacy.html");
        assert!(
            home.rendered_body.contains(r#"href="privacy.html""#),
            "a body link follows the claim: {}",
            home.rendered_body
        );

        let out = render_site(&sources, &opts);
        assert!(
            out.pages.iter().any(|p| p.dest_filename == "privacy.html"),
            "the rendered page is written at the claimed key"
        );
        let sitemap = out
            .assets
            .iter()
            .find(|(n, _)| n == "sitemap.xml")
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .unwrap();
        assert!(sitemap.contains("privacy.html"));
        assert!(!sitemap.contains("docs/privacy.html"));
    }

    /// A claim is site-root-absolute, so a page at depth linking to it gets a
    /// path back up to the root — not one relative to where the target's
    /// *source* sits.
    #[test]
    fn a_link_from_depth_to_a_serve_at_page_is_rebased() {
        let index = "---\ntitle: Home\n---\nHi.\n";
        let about =
            "---\ntitle: About\npart_of: \"/index.md\"\n---\nSee [privacy](../docs/privacy.md).\n";
        let privacy = "---\ntitle: Privacy\nserve_at: /privacy.html\n---\nThe policy.\n";
        let sources = vec![
            src("index.md", index, true),
            src("about/index.md", about, false),
            src("docs/privacy.md", privacy, false),
        ];

        let pages = build_pages(&sources, &SiteOptions::default());
        let about_page = pages.iter().find(|p| p.title == "About").unwrap();
        assert_eq!(about_page.dest_filename, "about/index.html");
        assert!(
            about_page
                .rendered_body
                .contains(r#"href="../privacy.html""#),
            "got {}",
            about_page.rendered_body
        );
    }

    /// The site's index is `index.html` by definition — it is the front door,
    /// not a page with an address — so a `serve_at:` on it claims nothing.
    #[test]
    fn the_site_index_ignores_serve_at() {
        let index = "---\ntitle: Home\nserve_at: /home.html\n---\nHi.\n";
        let sources = vec![src("index.md", index, true)];

        let pages = build_pages(&sources, &SiteOptions::default());
        assert_eq!(pages[0].dest_filename, "index.html");
        assert_eq!(dest_of(&sources[0]), "index.html");
    }

    /// [`dest_of`] is the rule the render applies, asked before the render —
    /// which is the only reason it is public.
    #[test]
    fn dest_of_answers_for_a_source_the_way_the_render_will() {
        let claimed = src(
            "docs/privacy.md",
            "---\ntitle: Privacy\nserve_at: /privacy\n---\nBody.\n",
            false,
        );
        let plain = src("docs/note.md", "---\ntitle: Note\n---\nBody.\n", false);
        assert_eq!(dest_of(&claimed), "privacy.html");
        assert_eq!(dest_of(&plain), "docs/note.html");

        let pages = build_pages(&[claimed, plain], &SiteOptions::default());
        assert_eq!(pages[0].dest_filename, "privacy.html");
        assert_eq!(pages[1].dest_filename, "docs/note.html");
    }

    // ── per-page `shell:` ───────────────────────────────────────────────────

    fn templates(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    const POSTER: &str = "<!DOCTYPE html><html lang=\"{{lang}}\"><head><title>{{document_title}}</title>\
         {{{head}}}</head><body class=\"poster\">{{{content}}}</body></html>";

    /// A page naming a shell wears it; the rest of the site keeps its own.
    #[test]
    fn a_page_may_name_its_own_shell() {
        let index = "---\ntitle: Home\ncontents:\n  - \"/poster.md\"\n---\nHi.\n";
        let poster =
            "---\ntitle: Poster\npart_of: \"/index.md\"\nshell: themes/poster.html\n---\nArt.\n";
        let sources = vec![
            src("index.md", index, true),
            src("poster.md", poster, false),
        ];

        let out = render_site(
            &sources,
            &SiteOptions {
                template: Some(
                    "<!DOCTYPE html><html><body class=\"site\">{{{content}}}</body></html>"
                        .to_string(),
                ),
                templates: templates(&[("themes/poster.html", POSTER)]),
                ..SiteOptions::default()
            },
        );

        assert!(out.template_error.is_none());
        assert!(
            out.page_shell_errors.is_empty(),
            "{:?}",
            out.page_shell_errors
        );

        let page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "poster.html")
            .unwrap();
        assert!(
            page.html.contains(r#"<body class="poster">"#),
            "{}",
            page.html
        );
        assert!(page.html.contains("<title>Poster - Home</title>"));

        // …and the site's own shell is untouched by the page that opted out.
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home.html.contains(r#"<body class="site">"#));
    }

    /// A page naming a shell the site does not carry falls back to the site's
    /// and says so — the bargain a broken site template already gets.
    #[test]
    fn a_missing_page_shell_falls_back_and_reports_itself() {
        let poster = "---\ntitle: Poster\nshell: themes/gone.html\n---\nArt.\n";
        let sources = vec![src("poster.md", poster, false)];

        let out = render_site(
            &sources,
            &SiteOptions {
                template: Some(
                    "<!DOCTYPE html><html><body class=\"site\">{{{content}}}</body></html>"
                        .to_string(),
                ),
                ..SiteOptions::default()
            },
        );

        assert_eq!(out.page_shell_errors.len(), 1);
        let report = &out.page_shell_errors[0];
        assert!(report.contains("poster.md"), "{report}");
        assert!(report.contains("themes/gone.html"), "{report}");
        let page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "poster.html")
            .unwrap();
        assert!(page.html.contains(r#"<body class="site">"#));
    }

    /// A page shell that will not compile costs that page its design, not the
    /// site's publication — and is reported once however many pages named it.
    #[test]
    fn a_broken_page_shell_is_reported_once_for_the_shell() {
        let one = "---\ntitle: One\nshell: themes/poster.html\n---\nA.\n";
        let two = "---\ntitle: Two\nshell: themes/poster.html\n---\nB.\n";
        let sources = vec![src("one.md", one, false), src("two.md", two, false)];

        let out = render_site(
            &sources,
            &SiteOptions {
                templates: templates(&[("themes/poster.html", "<html>{{contnet}}</html>")]),
                ..SiteOptions::default()
            },
        );

        assert_eq!(
            out.page_shell_errors.len(),
            1,
            "one broken template is one report: {:?}",
            out.page_shell_errors
        );
        assert!(
            out.page_shell_errors[0].contains("unknown shell slot `contnet`"),
            "{:?}",
            out.page_shell_errors
        );
        assert!(out.template_error.is_none(), "the site's shell is fine");
        for page in &out.pages {
            assert!(
                page.html.contains(r#"<div class="site-content">"#),
                "the built-in shell"
            );
        }
    }

    /// The site's own shell keeps its own error channel, and a page's failure
    /// does not appear in it.
    #[test]
    fn a_page_shell_does_not_disturb_the_site_shells_report() {
        let sources = vec![src(
            "poster.md",
            "---\ntitle: Poster\nshell: themes/gone.html\n---\nArt.\n",
            false,
        )];
        let out = render_site(
            &sources,
            &SiteOptions {
                template: Some("<html>{{contnet}}</html>".to_string()),
                ..SiteOptions::default()
            },
        );
        assert!(
            out.template_error
                .as_deref()
                .is_some_and(|e| e.contains("unknown shell slot")),
            "{:?}",
            out.template_error
        );
        assert_eq!(out.page_shell_errors.len(), 1);
    }

    /// `bare` and `verbatim` carry their own frame, so a `shell:` on one of
    /// them applies to nothing — and is not reported as missing either.
    #[test]
    fn a_bare_or_verbatim_page_takes_no_page_shell() {
        let bare = "---\ntitle: Bare\nlayout: bare\nshell: themes/gone.html\n---\nArt.\n";
        let verbatim =
            "---\ntitle: Verbatim\nlayout: verbatim\nshell: themes/poster.html\n---\n<h1>Hi</h1>\n";
        let sources = vec![
            src("bare.md", bare, false),
            src("verbatim.md", verbatim, false),
        ];

        let out = render_site(
            &sources,
            &SiteOptions {
                templates: templates(&[("themes/poster.html", POSTER)]),
                ..SiteOptions::default()
            },
        );

        assert!(
            out.page_shell_errors.is_empty(),
            "nothing was going to wear it: {:?}",
            out.page_shell_errors
        );
        let verbatim_page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "verbatim.html")
            .unwrap();
        assert_eq!(verbatim_page.html, "<h1>Hi</h1>\n");
        let bare_page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "bare.html")
            .unwrap();
        assert!(!bare_page.html.contains("class=\"poster\""));
    }

    /// A page shell and a `serve_at:` on the same document are independent
    /// claims, and both hold.
    #[test]
    fn a_page_may_claim_a_shell_and_a_destination_at_once() {
        let poster =
            "---\ntitle: Poster\nserve_at: /poster\nshell: themes/poster.html\n---\nArt.\n";
        let sources = vec![src("deep/nested/poster.md", poster, false)];

        let out = render_site(
            &sources,
            &SiteOptions {
                templates: templates(&[("themes/poster.html", POSTER)]),
                ..SiteOptions::default()
            },
        );

        let page = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "poster.html")
            .expect("the claimed destination");
        assert!(page.html.contains(r#"<body class="poster">"#));
    }

    #[test]
    fn render_site_produces_pages_nav_and_assets() {
        let index = "---\ntitle: Home\ncontents:\n  - \"[Child](/child.md)\"\n---\nHi.\n";
        let child = "---\ntitle: Child\npart_of: \"/index.md\"\n---\nKid.\n";
        let sources = vec![src("index.md", index, true), src("child.md", child, false)];

        let out = render_site(&sources, &SiteOptions::default());

        assert_eq!(out.pages.len(), 2);
        // index page carries the site nav with a link to the child
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home.html.contains("site-nav"));
        assert!(home.html.contains("child.html"));
        assert!(home.html.contains("<!DOCTYPE html>"));

        // assets include the stylesheet
        assert!(out.assets.iter().any(|(n, _)| n == "style.css"));
    }

    /// The whole point of widening the context: a page can list the other
    /// pages, which is the thing no amount of template *engine* could fix.
    #[test]
    fn a_page_can_list_the_sites_entries() {
        let index =
            "---\ntitle: Home\n---\n:::each{of=entries as=e}\n- [:val[e.title]]({{e.href}})\n:::\n";
        let sources = vec![
            src("index.md", index, true),
            src("a.md", "---\ntitle: Alpha\n---\nA.\n", false),
            src("b.md", "---\ntitle: Beta\n---\nB.\n", false),
        ];

        let out = render_site(&sources, &SiteOptions::default());
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();

        assert!(home.html.contains(r#"href="a.html""#), "got {}", home.html);
        assert!(home.html.contains("Alpha"), "got {}", home.html);
        assert!(home.html.contains("Beta"), "got {}", home.html);
        assert!(
            out.body_template_errors.is_empty(),
            "{:?}",
            out.body_template_errors
        );
    }

    /// `groups` comes back in prov's order — ascending by key — however the
    /// sources happened to be handed over. Source order was the old rule, and
    /// it made the same archive read two ways depending on which document the
    /// walk reached first.
    #[test]
    fn a_templates_groups_are_ordered_by_key_not_by_arrival() {
        let index = "---\ntitle: Home\n---\n:::each{of=groups as=g}\n- :val[g.key]\n:::\n";
        let sources = vec![
            src("index.md", index, true),
            src("c.md", "---\ntitle: C\npeople: Nan\n---\nC.\n", false),
            src("a.md", "---\ntitle: A\npeople: Ada\n---\nA.\n", false),
        ];
        let opts = SiteOptions {
            arrangement: Arrangement::Grouped(Grouping::field("people")),
            ..SiteOptions::default()
        };

        let out = render_site(&sources, &opts);
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        let ada = home.html.find("Ada").expect("an Ada group");
        let nan = home.html.find("Nan").expect("a Nan group");
        assert!(
            ada < nan,
            "ascending by key, not `Nan` first: {}",
            home.html
        );
    }

    /// The gate property, tested as a property of the pipeline rather than of a
    /// check: `entries` is built from the sources this render was handed, and
    /// audience exclusion happens before that — so there is no path by which a
    /// withheld document reaches a template. Remove the document, and the
    /// listing simply has nothing to say about it.
    #[test]
    fn a_template_cannot_reach_a_withheld_document() {
        let index = "---\ntitle: Home\n---\n:::each{of=entries as=e}\n- :val[e.title]\n:::\n";
        let admitted = vec![
            src("index.md", index, true),
            src("public.md", "---\ntitle: Public\n---\nP.\n", false),
        ];

        let out = render_site(&admitted, &SiteOptions::default());
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();

        assert!(home.html.contains("Public"), "got {}", home.html);
        assert!(!home.html.contains("Private"), "got {}", home.html);
    }

    /// The `{{ }}` migration: a brace outside a link destination publishes as
    /// itself and names its own page, rather than vanishing or being
    /// substituted.
    #[test]
    fn a_stray_brace_is_reported_against_the_page_that_wrote_it() {
        let index = "---\ntitle: Home\n---\nWelcome to {{ title }}.\n";
        let sources = vec![src("index.md", index, true)];

        let out = render_site(&sources, &SiteOptions::default());
        let home = &out.pages[0];

        assert!(home.html.contains("{{ title }}"), "got {}", home.html);
        assert_eq!(out.body_template_errors.len(), 1);
        assert!(
            out.body_template_errors[0].starts_with("index.md:"),
            "{:?}",
            out.body_template_errors
        );
    }

    /// A body template that will not expand publishes its own source — and
    /// says so, which is the half that used to be missing.
    #[test]
    fn a_broken_body_template_is_reported_rather_than_swallowed() {
        let index = "---\ntitle: Home\n---\n:::if{equals=title}\nX\n:::\n";
        let sources = vec![src("index.md", index, true)];

        let out = render_site(&sources, &SiteOptions::default());

        assert_eq!(out.body_template_errors.len(), 1);
        assert!(
            out.body_template_errors[0].contains("equals"),
            "{:?}",
            out.body_template_errors
        );
    }

    #[test]
    fn a_page_can_name_its_parent_children_and_trail() {
        let index = "---\ntitle: Home\ncontents:\n  - \"[Child](/child.md)\"\n---\n:::each{of=children as=c}\n- :val[c.title]\n:::\n";
        let child = "---\ntitle: Child\npart_of: \"/index.md\"\n---\nparent: :val[parent.title]\n\n:::each{of=breadcrumbs as=b}\n- :val[b.title]\n:::\n";
        let sources = vec![src("index.md", index, true), src("child.md", child, false)];

        let out = render_site(&sources, &SiteOptions::default());
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        let kid = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "child.html")
            .unwrap();

        assert!(home.html.contains("<li>Child</li>"), "got {}", home.html);
        assert!(kid.html.contains("parent: Home"), "got {}", kid.html);
        // Root first, this page last.
        let trail = kid
            .html
            .find("<li>Home</li>")
            .zip(kid.html.find("<li>Child</li>"));
        let (root_at, self_at) = trail.unwrap_or_else(|| panic!("got {}", kid.html));
        assert!(root_at < self_at, "got {}", kid.html);
    }
}
