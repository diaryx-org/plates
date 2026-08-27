//! What collection produces: the documents a site publishes, and the files
//! they drag along.
//!
//! Plain data on purpose. A collected site is the boundary between the half of
//! the work that needs a vault and the half that does not — a renderer, a
//! publisher and a diff all start from these and none of them opens the
//! workspace again.

use std::path::PathBuf;

/// A collected, gate-scoped source document ready to render or upload.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// The document's source: sensitive frontmatter stripped, body
    /// visibility-filtered, whatever the caller asked to be stamped stamped.
    /// Still *pre-template* and unrendered — this is the document, scoped.
    pub source_markdown: String,
    /// Sanitized workspace-relative path, keeping the body's own grammar
    /// extension (`"Welcome.md"`, `"notes/post.dj"`). Load-bearing rather than
    /// cosmetic: `plates_render` reads a body's format off its extension, and a
    /// site is not required to be all one grammar.
    pub source_rel_path: String,
    /// Sanitized destination path this document publishes at (`"index.html"`,
    /// `"notes/post.html"`) — either derived from the source path or claimed
    /// outright by a frontmatter `serve_at:`.
    pub dest_path: String,
    /// The document's own identifier, from prov's registry or its frontmatter
    /// `id`. Carried through untouched; nothing in collection reads it.
    pub id: Option<String>,
    /// Whether this is the site's front page.
    pub is_index: bool,
    /// The [`source_rel_path`](Self::source_rel_path)s of the documents that
    /// link to this one, sorted and each named once.
    ///
    /// **Narrowed to this site**, which is the field's whole discipline: the
    /// archive's inverted link map is about the vault, and a document the gate
    /// held back is in it. Publishing one of those names on the page it links
    /// to would disclose the document by its path and its title, so collection
    /// intersects the map with the set the plan admits before writing anything
    /// here. See [`CollectOptions::backlinks`](crate::CollectOptions::backlinks).
    ///
    /// Site coordinates, not the vault's: rebased onto the anchor and sanitized
    /// exactly as [`source_rel_path`](Self::source_rel_path) is, so the render
    /// layer can match a name here against the source that carries it without
    /// a second convention to keep in step.
    pub backlinks: Vec<String>,
}

/// A file a site ships verbatim: an image a body references, an `attachments:`
/// entry, a page's own stylesheet or script, or a file covered by the manifest
/// fronting the site.
///
/// Unlike a [`SourceFile`], whose bytes are *derived* (body filtered,
/// frontmatter stripped and re-stamped), an attachment ships its file as it is.
/// That is what makes [`bytes`](Self::bytes) optional: a caller that only needs
/// [`hash`](Self::hash) can have it recalled for an unchanged file rather than
/// computed from a read of it — which on an archive of photographs is the
/// difference between a preview and a wait. See [`crate::digest`].
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Destination path relative to the site root.
    pub dest_rel: String,
    /// Where the payload lives, workspace-relative — the answer to "read it
    /// after all", for a caller that needs the bytes collection never read.
    pub source_path: PathBuf,
    /// The file's digest, in whatever spelling
    /// [`CollectOptions::digest`](crate::collect::CollectOptions::digest)
    /// produces.
    pub hash: String,
    /// Size of the file, from its stat. What a preview quotes before any of
    /// this has been read.
    pub len: u64,
    /// The payload, when collection read it — because no digest was remembered
    /// for this file at this stat. `None` means unread: the bytes are whatever
    /// [`source_path`](Self::source_path) holds.
    pub bytes: Option<Vec<u8>>,
    /// MIME type, guessed from the extension.
    pub mime_type: String,
}

/// A collected site: its sources and the files they reference.
#[derive(Debug, Clone, Default)]
pub struct CollectedSite {
    /// The documents, in the order the plan listed them.
    pub sources: Vec<SourceFile>,
    /// Every file the site ships alongside them, deduplicated by destination.
    pub attachments: Vec<Attachment>,
    /// Whether this site's front page is among the
    /// [`attachments`](Self::attachments) — shipped verbatim — rather than
    /// something to render.
    ///
    /// True only for a site fronted by a manifest node (see
    /// [`IndexDirectory`](crate::IndexDirectory)). It is what stops a renderer
    /// synthesizing an index over the authored one: no source claims
    /// [`SourceFile::is_index`] in that case, and "no source claims it" is
    /// otherwise exactly the signal that a front page needs generating.
    pub verbatim_front_page: bool,
}
