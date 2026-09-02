//! What can go wrong between a declaration and a directory of HTML.
//!
//! Deliberately small. Everything here is a *site declaration* problem — a front
//! page that resolves to nothing, an index nobody may read, two documents
//! claiming one address — plus a passthrough for prov's own. Collection failures
//! that are not the site's fault are handled where they happen: an attachment
//! whose file has vanished is skipped, not raised, because refusing to build the
//! site over one missing photograph is the wrong trade for every caller.

use std::path::PathBuf;

/// A site-shaped failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A document's audience regions could not all be resolved, so the
    /// collection refused it.
    ///
    /// Fatal rather than skipped, and fatal rather than published: the filter
    /// answers only "yes, filtered" or "I could not account for a region", and
    /// the region it could not account for is the one somebody marked private.
    /// A site that published everything except the document it could not scope
    /// would be the failure this error exists to prevent, arriving quietly.
    #[error("{path}: {reason}")]
    Visibility {
        /// The document, workspace-relative.
        path: std::path::PathBuf,
        /// What the filter could not do, in its own words.
        reason: String,
    },
    /// A site names a `view:` the workspace does not declare. Reported rather
    /// than ignored, because falling back to the unscoped arrangement would
    /// publish every document in the gate's set under a site built to narrow it.
    #[error("site {site:?} is arranged by view {view:?}, which is not declared")]
    UnresolvedSiteView {
        /// The site's name, as declared under `exports`.
        site: String,
        /// The view name the site named.
        view: String,
    },

    /// A site names an `index:` that does not resolve to a document in this
    /// workspace — a retired id, a deleted page, or a link to nowhere.
    #[error("site {site:?} is fronted by {target:?}, which resolves to nothing")]
    UnresolvedSiteIndex {
        /// The site's name, as declared under `exports`.
        site: String,
        /// The `index:` link exactly as written.
        target: String,
    },

    /// A site's `index:` resolves to a document the site does not publish —
    /// either the gate cannot see it (the usual cause: a front page nobody
    /// tagged) or the site's view scopes it out.
    #[error("site {site:?} is fronted by {path}, which audience {audience:?} does not publish")]
    SiteIndexNotVisible {
        /// The site's name, as declared under `exports`.
        site: String,
        /// The audience the site publishes to.
        audience: String,
        /// The resolved path of the declared index.
        path: PathBuf,
    },

    /// A site's `index:` resolves to a document its own hold is keeping back:
    /// the front page is a draft.
    ///
    /// Separate from [`SiteIndexNotVisible`](Self::SiteIndexNotVisible)
    /// because nothing is mistagged and no audience is wrong — the gate
    /// admitted the page, and its author said `draft: true` about it. The two
    /// send someone to different files, so they are two errors.
    ///
    /// Still an error rather than a synthesized index, for the reason that one
    /// is: a site whose front page quietly vanished is a site that publishes
    /// looking fine. Unlike the gate's, this one clears itself — the fix is to
    /// finish the page, or to take the `hold` off the export.
    #[error("site {site:?} is fronted by {path}, which declares {field}: true and is held back")]
    SiteIndexHeld {
        /// The site's name, as declared under `exports`.
        site: String,
        /// The field the site's `hold` names.
        field: String,
        /// The resolved path of the declared index.
        path: PathBuf,
    },

    /// A site's `index:` resolves to a manifest node whose covered directory
    /// holds no `index.html`.
    ///
    /// Fronting a site with a manifest node says "serve this directory", and a
    /// directory written to be served has a front page. Synthesizing a listing
    /// instead would publish a site whose author's landing page is missing and
    /// look fine doing it — the same reasoning that makes
    /// [`SiteIndexNotVisible`](Self::SiteIndexNotVisible) an error rather than a
    /// cue to fall back.
    #[error(
        "site {site:?} is fronted by the directory {root}, which holds no {front_page} to serve"
    )]
    SiteIndexDirectoryHasNoFrontPage {
        /// The site's name, as declared under `exports`.
        site: String,
        /// The covered directory, workspace-relative.
        root: PathBuf,
        /// The file that was looked for — [`FRONT_PAGE`](crate::FRONT_PAGE).
        front_page: String,
    },

    /// Two documents in one site claim the same published destination.
    ///
    /// Loud, and naming both, because the alternative is what used to happen:
    /// two documents resolving to one object key, one silently overwriting the
    /// other in the published namespace and in a local build alike, with
    /// nothing to say which one won. A `serve_at:` makes that reachable on
    /// purpose rather than by an accident of sanitization, so it is refused at
    /// the point the claim is made — before anything is written.
    #[error("{first} and {second} both publish at {dest:?}")]
    DestinationClaimedTwice {
        /// The published destination both documents want.
        dest: String,
        /// The document that claimed it first.
        first: PathBuf,
        /// The document that claimed it second.
        second: PathBuf,
    },

    /// A document could not be read, or its frontmatter could not be written
    /// back out for the collected source.
    #[error("{path}: {reason}")]
    Document {
        /// The document, workspace-relative.
        path: PathBuf,
        /// What went wrong, in the words of whatever refused.
        reason: String,
    },

    /// prov refused to plan the export behind this site.
    ///
    /// Carried as text rather than wrapping `prov::exports::Error`, so this
    /// crate's public error does not grow a variant every time the planner
    /// gains one. The one case there is something better to say about — an
    /// unknown view — is translated into
    /// [`UnresolvedSiteView`](Self::UnresolvedSiteView) instead.
    #[error("this site cannot be planned: {0}")]
    Export(String),

    /// prov could not read the workspace.
    #[error(transparent)]
    Prov(#[from] prov::Error),

    /// The filesystem refused a read this crate makes directly — a `try_exists`
    /// behind a front page's resolution, which prov hands back unwrapped.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// This crate's result alias.
pub type Result<T> = std::result::Result<T, Error>;
