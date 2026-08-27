//! Pure value types describing a site to be rendered.
//!
//! Nothing here opens, resolves or allocates anything on disk. These are the
//! *description* a caller hands the renderer, which is why the same description
//! can be assembled by a CLI, by a server, or by an edge worker.
//!
//! Appearance types (colors, typography, favicon, theme) live in
//! [`crate::appearance`].

use std::path::PathBuf;

/// Options for publishing.
#[derive(Debug, Clone)]
pub struct PublishOptions {
    /// Output as a single HTML file instead of multiple files
    pub single_file: bool,
    /// Site title (defaults to workspace title)
    pub title: Option<String>,
    /// Include audience filtering
    pub audience: Option<String>,
    /// Overwrite existing destination
    pub force: bool,
    /// Copy referenced attachment files to the output directory
    pub copy_attachments: bool,
    /// Base URL for sitemap, canonical URLs, og tags, and feeds.
    pub base_url: Option<String>,
    /// Generate sitemap.xml, robots.txt, and SEO meta tags (default true).
    pub generate_seo: bool,
    /// Generate feed.xml (Atom) and rss.xml (RSS) feeds (default true).
    pub generate_feeds: bool,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            single_file: false,
            title: None,
            audience: None,
            force: false,
            copy_attachments: true,
            base_url: None,
            generate_seo: true,
            generate_feeds: true,
        }
    }
}

/// Which shell a page is wrapped in, from its frontmatter `layout:`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PageLayout {
    /// The site shell: nav, breadcrumbs, footer, site stylesheet, built-in
    /// interactivity script — or the caller's template in place of all of it.
    /// What a page with no `layout:` gets.
    #[default]
    Site,
    /// A complete document with none of the site's frame: no nav, no
    /// breadcrumbs, no footer, no site stylesheet and no built-in script — only
    /// the page's own `styles:`/`scripts:` around its rendered body.
    ///
    /// For a page that *is* a design of its own (a landing page, a poster, a
    /// visualization) rather than an entry in a site's furniture. It still
    /// appears in the nav, the sitemap and the feeds like any other page: bare
    /// is about what the page looks like, not about whether the site knows it.
    ///
    /// A supplied shell template does not apply to it — `bare` is a statement
    /// that this page carries its own frame, and wrapping it in someone else's
    /// would be the thing it asked not to happen.
    Bare,
    /// The body *is* the file. Everything after the metadata block is written
    /// out byte for byte: no wrapper, no head, no head links, no chrome — and,
    /// unlike every other layout, no parse either.
    ///
    /// A `bare` page is still rendered: its body goes through templating, twig,
    /// and link rewriting, and comes back as twig's serialization of it. That is
    /// right for prose and wrong for a hand-authored page, where a reserialized
    /// document is a *different* document — attribute order moves, void tags are
    /// respelled, an inline `<script>` survives or does not depending on how the
    /// parser felt about it. A designed landing page is a file someone wrote,
    /// not a document someone described, and the only faithful thing to do with
    /// it is copy it.
    ///
    /// So `verbatim` is for a self-contained HTML file that carries frontmatter
    /// only so the vault can see it: the metadata makes it a document the site
    /// knows — it appears in the nav, the sitemap and the feeds like any other
    /// page — while the bytes below the metadata are published unread.
    ///
    /// The cost is that nothing is done *for* it. Its links are not rewritten,
    /// so a `.md` href in it stays a `.md` href and a vault-root-absolute path
    /// stays absolute; its `styles:`/`scripts:` are not emitted, since there is
    /// no head to emit them into. A verbatim page is responsible for itself,
    /// which is the point of asking for one.
    Verbatim,
}

impl PageLayout {
    /// Read a frontmatter `layout:` value. Anything unrecognized — including
    /// the absent case — is [`PageLayout::Site`], because a site whose theme
    /// spells a layout this version does not know should still publish.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("bare") => Self::Bare,
            Some("verbatim") => Self::Verbatim,
            _ => Self::Site,
        }
    }

    /// Whether the body is published unread — no templating, no parse, no link
    /// rewriting. True only for [`PageLayout::Verbatim`].
    pub fn is_verbatim(self) -> bool {
        matches!(self, Self::Verbatim)
    }
}

/// A navigation link.
#[derive(Debug, Clone)]
pub struct NavLink {
    /// Link href (relative path or anchor)
    pub href: String,
    /// Display title
    pub title: String,
}

/// A processed file ready for publishing.
#[derive(Debug, Clone)]
pub struct PublishedPage {
    /// Original source path
    pub source_path: PathBuf,
    /// Destination filename (e.g., "index.html" or "my-entry.html")
    pub dest_filename: String,
    /// Page title
    pub title: String,
    /// Rendered content in the output format (body only, no wrapper)
    pub rendered_body: String,
    /// Original markdown body
    pub markdown_body: String,
    /// Navigation links to children (from contents property)
    pub contents_links: Vec<NavLink>,
    /// Navigation link to parent (from part_of property)
    pub parent_link: Option<NavLink>,
    /// Whether this is the root index
    pub is_root: bool,
    /// Page description (from frontmatter `description`)
    pub description: Option<String>,
    /// Page author (from frontmatter `author`)
    pub author: Option<String>,
    /// Creation date (from frontmatter `created`)
    pub created: Option<String>,
    /// Last update date (from frontmatter `updated`)
    pub updated: Option<String>,
    /// The date the document is *about*, as opposed to when its file was made
    /// (from frontmatter `date_of_document`). First link in the date chain a
    /// grouped arrangement sorts by: `date_of_document` → `created` → `updated`,
    /// the same chain a grouped view is cut by.
    pub date_of_document: Option<String>,
    /// The values this page groups under in a grouped arrangement — the date
    /// cut to the view's grain, or the grouping field's values. Empty for a
    /// containment arrangement, or for a page carrying nothing to group by
    /// (which lands it in the "ungrouped" bucket rather than dropping it).
    pub group_keys: Vec<String>,
    /// Attachment paths (from frontmatter `attachments`)
    pub attachments: Vec<String>,
    /// Stylesheets this page pulls in (from frontmatter `styles`), as paths
    /// below the site root — already resolved against the document that named
    /// them, so `../theme.css` and `/theme.css` both arrive as `theme.css`.
    ///
    /// Emitted as `<link rel="stylesheet">` after the site stylesheet, rebased
    /// to the page's own depth. The file itself is the caller's to copy, the
    /// same way an `attachments` entry is.
    pub styles: Vec<String>,
    /// Scripts this page pulls in (from frontmatter `scripts`), resolved and
    /// copied exactly like [`styles`](Self::styles) and emitted as
    /// `<script defer src="…">` after the built-in interactivity script.
    pub scripts: Vec<String>,
    /// Which shell wraps this page (from frontmatter `layout`).
    pub layout: PageLayout,
    /// The shell template this page asked for by name (from frontmatter
    /// `shell`), as the vault-relative path it was written as — the key into
    /// [`SiteOptions::templates`](crate::site::SiteOptions::templates), since
    /// the render crate reads no files.
    ///
    /// `None` for a page that takes the site's own shell, which is every page
    /// that does not name one — and every `bare`/`verbatim` page, which take no
    /// shell at all and so are never recorded as wanting one.
    pub shell: Option<String>,
    /// Override title shown in navigation (from frontmatter `nav_title`)
    pub nav_title: Option<String>,
    /// Sort order among siblings in navigation (from frontmatter `nav_order`)
    pub nav_order: Option<i32>,
    /// Whether to hide this page from the navigation tree
    pub hide_from_nav: bool,
    /// Whether to hide this page from RSS/Atom feeds
    pub hide_from_feed: bool,
    /// The source document's own identifier, read from frontmatter `id` — which
    /// is prov's registry id for the file.
    ///
    /// Carried through the render untouched: nothing here reads it. It is here
    /// because a caller that mints permalinks, builds an index, or addresses the
    /// published object by identity needs to know which page each id belongs to,
    /// and the render is the only place that pairing exists.
    pub id: Option<String>,
    /// The audience-scoped markdown source (frontmatter + visibility-filtered
    /// body) uploaded as a sibling so the server can serve `?content`/`?json`.
    pub source_markdown: String,
}

impl PublishedPage {
    /// When the entry is *of*, as its vault wrote it:
    /// `date_of_document` → `created` → `updated`.
    ///
    /// The one chain, so a site cannot disagree with itself. A grouped
    /// arrangement files and orders entries by this (it is the chain prov's
    /// `views` cuts by), and
    /// the feeds, the sitemap and `article:published_time` used to answer a
    /// different question — `updated` → `created` — so a journal of scanned
    /// letters, whose `date_of_document` is the year it was written and whose
    /// `created` is the day it was scanned, syndicated in scanning order while
    /// its own front page listed it by letter date.
    pub fn published_date(&self) -> Option<&str> {
        self.date_of_document
            .as_deref()
            .or(self.created.as_deref())
            .or(self.updated.as_deref())
            .filter(|d| !d.is_empty())
    }

    /// When the entry last changed: `updated`, else whatever
    /// [`published_date`](Self::published_date) found.
    ///
    /// What a sitemap's `lastmod` and a feed entry's `<updated>` mean, as
    /// against the `<published>` above them.
    pub fn modified_date(&self) -> Option<&str> {
        self.updated
            .as_deref()
            .filter(|d| !d.is_empty())
            .or_else(|| self.published_date())
    }
}

/// One node of a site's **spanning outline**: the archive's own containment
/// hierarchy, materialized by whoever holds the workspace.
///
/// A vault's spine is configured, not spelled: prov's `spanning:` names the
/// relation whose links contain, and `contents:`/`part_of:` is one vault
/// dialect's spelling of it. This crate cannot read a workspace's configuration
/// — it reads nothing — so the layer that can walks the tree and hands the
/// result down as plain data. See [`SiteOptions::outline`](crate::site::SiteOptions::outline).
///
/// [`path`](Self::path) is the source path in the coordinates
/// [`SourceDoc::path`](crate::site::SourceDoc::path) is written in: rebased onto
/// the site's anchor, sanitized, carrying the body's own extension. That is what
/// lets a node be matched to the page it became without either side re-deriving
/// the other's naming rule.
///
/// A node naming a document this site does not publish is not an error and not a
/// nav entry — it is pruned, and its published descendants hoist to the nearest
/// ancestor that *is* published. Under explicit-only visibility that is the
/// ordinary shape, not the edge case.
#[derive(Debug, Clone, Default)]
pub struct OutlineNode {
    /// The source path this node names, spelled as
    /// [`SourceDoc::path`](crate::site::SourceDoc::path) spells it.
    pub path: String,
    /// The label the containing document's link carried (`[Label](path)`), when
    /// it carried one. A fallback only: a page's own `nav_title`/`title` wins.
    pub label: Option<String>,
    /// Contained nodes, in the order the containing document declared them.
    pub children: Vec<OutlineNode>,
}

/// A node in the full site navigation tree.
#[derive(Debug, Clone)]
pub struct SiteNavNode {
    /// Node title
    pub title: String,
    /// Node href
    pub href: String,
    /// Whether this is the current page
    pub is_current: bool,
    /// Whether this node is an ancestor of the current page
    pub is_ancestor_of_current: bool,
    /// Child nodes
    pub children: Vec<SiteNavNode>,
}

/// Full site navigation context for a specific page.
#[derive(Debug, Clone)]
pub struct SiteNavigation {
    /// Full nav tree with current-page marking
    pub tree: Vec<SiteNavNode>,
    /// Breadcrumb trail from root to current page
    pub breadcrumbs: Vec<NavLink>,
}

/// Result of a publishing operation.
#[derive(Debug)]
pub struct PublishResult {
    /// Pages that were published
    pub pages: Vec<PublishedPage>,
    /// Total files processed
    pub files_processed: usize,
    /// Number of attachment files copied to the output directory
    pub attachments_copied: usize,
}

/// What a grouped arrangement sorts entries into groups by.
///
/// prov's own, not a mirror of it. This used to be a redeclaration — the crate
/// sits below the workspace layer and must stay portable to
/// `wasm32-unknown-unknown`, so it kept its own `DateGrain` and `Grouping` with
/// the spellings and prefix lengths copied across, on the reasoning that a site
/// grouped "by year" must cut dates the same way the app's lens does or the
/// published archive reads differently from the vault it came from.
///
/// Since prov 0.5 the grouping engine is `prov-views`, which reaches nothing
/// that can write and is already in this crate's dependency graph. So the way to
/// keep the two identical is to stop having two: the published site now groups
/// through the same [`Grouping::keys_of`] the vault does, and "identical" is a
/// fact rather than a promise two copies make to each other.
pub use prov::views::{Grain, Grouping};

/// How a site is arranged — the render-side half of a site's `view:`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Arrangement {
    /// Nav follows containment where audience filtering left it intact, and
    /// pages the walk cannot reach become roots of their own. What a
    /// hierarchical vault wants, and the behaviour when a site declares no view.
    #[default]
    Containment,
    /// Entries are gathered into groups. The generated index shows the groups;
    /// the nav lists entries in group order rather than by containment, because
    /// a site that declared an arrangement asked for one.
    Grouped(Grouping),
}

/// Normalize a frontmatter `serve_at:` value into a path below the site root,
/// or `None` when it claims nothing this crate can serve.
///
/// The value is **site-root-absolute** and must start with `/`. That is what
/// makes it a claim on the site's own layout rather than on the directory the
/// document happens to sit in — and why, unlike a derived destination, it is
/// never rebased onto a site's anchor: it is already written in the
/// coordinates a rebasing would produce.
///
/// `/privacy` and `/privacy.html` are the same claim: a value that does not
/// already end in `.html` gains it, because what is being named is a page and a
/// page is an HTML file. Components are sanitized the way every other published
/// path is, and `.`/`..` are dropped rather than resolved — a destination is a
/// name *inside* the site, and there is nothing above the site root to reach.
pub fn serve_at_dest(value: &str) -> Option<String> {
    let rest = value.trim().strip_prefix('/')?;
    let mut parts: Vec<String> = Vec::new();
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        let cleaned = crate::links::sanitize_path_component(part);
        if !cleaned.is_empty() {
            parts.push(cleaned);
        }
    }
    if parts.is_empty() {
        return None;
    }
    let mut dest = parts.join("/");
    if !dest.ends_with(".html") {
        dest.push_str(".html");
    }
    Some(dest)
}
