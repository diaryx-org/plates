//! Turning an archive into a website: where a vault's documents land, and what
//! ships alongside them.
//!
//! Two crates make a website out of a [`prov`] archive, and this is the outer
//! one. prov says *which documents* a site holds — the gate, the view, and the
//! one-way valve between them. [`plates_render`] says *what HTML a document
//! becomes*, reading nothing and resolving nothing. Between them sits the layer
//! this crate is: **where everything lands and what ships with it**.
//!
//! ```text
//!   prov            plates                   plates-render
//!   ─────           ──────                   ─────────────
//!   which           where it lands,          what it looks
//!   documents  ──▶  what ships with it  ──▶  like
//! ```
//!
//! # What that means concretely
//!
//! - **The anchor.** A site's front page's directory becomes the site's root,
//!   and every published coordinate is written relative to it. See
//!   [`collect::anchor_of`], and [`collect`]'s module docs for the one place two
//!   correct answers about a leading `/` have to be reconciled.
//! - **The reference scan.** Which files a page drags along: link targets,
//!   `src`/`href`/`srcset` attributes, and the frontmatter `attachments:`,
//!   `styles:` and `scripts:` lists. Grammar-blind, so Markdown, Djot and HTML
//!   all work through one scanner.
//! - **Destinations.** Path-to-URL shaping, `serve_at:` claims, and refusing two
//!   documents that claim one address rather than letting one quietly overwrite
//!   the other.
//! - **The front page.** [`SitePlan`] on top of prov's export plan: an index
//!   resolved through the spanning relation so it survives renames, the rule
//!   that an index need not be among the entries but must be admitted by the
//!   gate, and [`IndexDirectory`] — a manifest node fronting a site with a whole
//!   covered directory, rebased onto the site root.
//! - **The theme.** Reading a declaration's shell and stylesheet into *text*,
//!   because the renderer cannot open a file.
//!
//! # The dependency list is the design
//!
//! prov, `plates-render` and `thiserror`. That is the whole list, and it is
//! meant to stay that way: a site is planned and collected from a
//! [`prov::Workspace`] and nothing else, and `plates-render` is taken *without*
//! its `templating` feature, so no template engine is linked here — running the
//! render pipeline is the caller's job, not this crate's.
//!
//! What that buys is that a caller's several commands cannot drift. Building to
//! a directory, serving a live preview and uploading to a host are one collector
//! with different options bolted to it ([`collect::CollectOptions`]), rather
//! than three walks that agree until one of them is fixed.
//!
//! # What a caller still owns
//!
//! - **The config vocabulary.** Which block a vault declares its sites and front
//!   pages in is not read here, deliberately — that is a vault format's dialect,
//!   and a [`SiteSpec`] arrives already built. Two applications with different
//!   config formats can compose the same `SiteSpec` and get the same site.
//! - **The gate field.** [`plan::AUDIENCE_FIELD`] is fixed to `audience`. A
//!   vault that names its visibility field something else cannot say so yet;
//!   this wants to become a [`SiteSpec`] field.
//! - **Running the renderer.** This crate hands back sources and attachments.
//!   Turning them into pages is `plates-render`'s, with the caller choosing when
//!   and where.

pub mod collect;
pub mod digest;
pub mod error;
pub mod plan;
pub mod source;
pub mod spec;
pub mod theme;

pub use collect::{
    CollectOptions, NoStamp, SourceStamp, anchor_of, collect_documents, collect_site,
    declared_dest, mime_type_from_ext, rebase, sanitize_component, sanitize_rel_path,
};
pub use digest::{DigestMemo, NoDigests, mtime_ms};
pub use error::{Error, Result};
pub use plan::{AUDIENCE_FIELD, plan_site, to_export};
pub use source::{Attachment, CollectedSite, SourceFile};
pub use spec::{
    FRONT_PAGE, IndexDirectory, SITE_ASSETS_DIR, SitePlan, SiteSpec, VisibleDoc, case_drift,
    finish, humanize,
};
pub use theme::{DEFAULT_LANG, SiteTheme, arrangement_for, read_page_shells, read_theme};

/// Re-export both layers below, so a downstream caller names one `prov` and one
/// `plates_render` rather than tracking their versions separately.
pub use plates_render;
pub use prov;
