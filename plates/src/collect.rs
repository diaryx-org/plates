//! Collection: turning a planned site into the files that serve it.
//!
//! The half of the work that needs a vault. Every document the plan admits is
//! read once, its body filtered for the gate's audience, its frontmatter
//! stripped of what must not travel and stamped with whatever the caller asks
//! for; every file those documents drag along — a referenced image, an
//! `attachments:` entry, a page's own stylesheet, a covered directory's assets —
//! is found, weighed, and named at the address it will be served from.
//!
//! No HTML is produced here. What comes out is [`CollectedSite`], and a
//! renderer, a publisher and a content diff all start from the same one.
//!
//! # The anchor
//!
//! The rule this module exists for. A site's front page's *directory* becomes
//! the site's root, and every published coordinate — a source key, a page's
//! destination, an attachment — is written relative to it. See [`anchor_of`].
//!
//! It is also the one place two correct answers disagree: a leading `/` means
//! the *vault* root to [`prov::link::resolve`] and the *site* root to
//! `plates_render`. `push_canonical_ref` is where they are reconciled, and a
//! vault-rooted site anchors at `""`, where the whole question is a no-op.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use prov::{IdIndex, Storage, Workspace};

use plates_render::visibility::{Audience, filter_body};

use crate::digest::{self, DigestMemo};
use crate::error::{Error, Result};
use crate::source::{Attachment, CollectedSite, SourceFile};
use crate::spec::{IndexDirectory, SitePlan};

/// What a caller wants stamped into a collected document's frontmatter, beyond
/// the keys [`CollectOptions::strip_keys`] removes.
///
/// The hook exists because the two callers disagree, and both are right. A
/// local build wants the document as the vault wrote it. A publish wants its
/// durable public identity written in, because the id lives in the registry
/// rather than the body — so without a stamp the uploaded bytes of a
/// (re)published document are identical to the last ones, the content diff
/// calls it unchanged, and nothing re-registers it.
///
/// The document on disk is never touched either way. This shapes the *collected
/// copy* only.
pub trait SourceStamp {
    /// Add to `fm` for the document at `path`, whose identity is `id`.
    fn stamp(&self, path: &Path, id: Option<&str>, fm: &mut prov::meta::Mapping);
}

/// A stamp that adds nothing — the collected frontmatter is the document's own,
/// less whatever was stripped.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStamp;

impl SourceStamp for NoStamp {
    fn stamp(&self, _path: &Path, _id: Option<&str>, _fm: &mut prov::meta::Mapping) {}
}

/// Everything a collection needs that the plan does not carry.
pub struct CollectOptions<'a> {
    /// The audience whose `:vis[...]` regions survive in each collected body.
    ///
    /// The plan already decided *which documents* leave; this decides which
    /// parts of each one does. Both halves read the same name, and they have to:
    /// a body filtered for one audience inside a site gated for another is a
    /// disclosure with no one to notice it.
    pub audience: &'a str,
    /// Top-level frontmatter keys removed from the collected source.
    ///
    /// A collected document is routinely served publicly, so internal
    /// configuration must not ride along in it. Author-facing metadata — title,
    /// description, dates — is not this list's business.
    pub strip_keys: &'a [&'a str],
    /// What else goes into the collected frontmatter. See [`SourceStamp`].
    pub stamp: &'a dyn SourceStamp,
    /// Each document's identity by path — prov's registry ids, built once by
    /// the caller and shared across every site in a run.
    ///
    /// A document not in the map falls back to its own frontmatter `id`, which
    /// is where the authoritative one lives under `id_storage: both`/
    /// `frontmatter`, or when an imported vault's registry is stale.
    pub id_by_path: &'a HashMap<PathBuf, String>,
    /// The archive's links, inverted: every document to the documents that link
    /// to it, workspace-relative. [`prov::Workspace::backlinks`] is where one
    /// comes from.
    ///
    /// The caller's for the reason [`id_by_path`](Self::id_by_path) is: taking
    /// it means a census of every document in the archive, and a run that
    /// publishes four sites should pay for that once rather than four times.
    /// It is the *vault's* map, deliberately — narrowing it to what a site may
    /// name is this module's job, because only the plan knows which set that
    /// is, and doing it here is what keeps a withheld document out of
    /// [`SourceFile::backlinks`].
    ///
    /// An empty map is a legitimate answer, and means no page learns who links
    /// to it.
    pub backlinks: &'a BTreeMap<PathBuf, Vec<prov::Backlink>>,
    /// What each attachment hashed to last time. See [`crate::digest`].
    pub digests: &'a dyn DigestMemo,
    /// How an attachment's bytes are digested when the memo does not recognize
    /// it.
    ///
    /// The caller's, not this crate's: the digest is a *protocol* — it is
    /// compared against what some other system reports — so the algorithm
    /// belongs to whoever will do the comparing. A caller that never reads
    /// [`Attachment::hash`] can pass anything.
    pub digest: fn(&[u8]) -> String,
}

/// Collect one planned site's sources + attachments.
///
/// The plan is the caller's — [`crate::plan_site`] makes one, and a caller that
/// selects documents some other way can hand `collect_documents` its own list
/// instead. What this adds on top of the plan is the anchor, the front page's
/// two shapes, and the walk.
///
/// When the site declares no `index:`, **no source is flagged as the index**,
/// and the render layer synthesizes a front page from the entries. The
/// alternative — promoting whichever document sorted first — is what made a
/// site's front page, and through [`SourceFile::is_index`] its published
/// identity, depend on traversal order.
pub async fn collect_site<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    plan: &SitePlan,
    opts: &CollectOptions<'_>,
) -> Result<CollectedSite> {
    let mut docs: Vec<(PathBuf, bool)> = plan
        .entries
        .iter()
        .map(|doc| {
            let is_index = plan.index.as_deref() == Some(doc.path.as_path());
            (doc.path.clone(), is_index)
        })
        .collect();

    // Where the site's own root is, on disk — see [`anchor_of`].
    let anchor = anchor_of(plan);

    // A front page that is a *directory* is not collected as a document at all.
    // A manifest node has no body — it is a claim over a set of files — so
    // rendering it would publish an empty page at the site's own root, and the
    // authored `index.html` sitting inside the directory it covers would never
    // be uploaded. The directory ships as bytes instead, below.
    match plan.index_directory.as_ref() {
        Some(dir) => {
            // Belt and braces: the node is the site's frame either way, and a
            // vault that also has it in the gate's set (an export with no view
            // narrowing it) would otherwise render it as an ordinary entry.
            if let Some(index) = plan.index.as_deref() {
                docs.retain(|(path, _)| path != index);
            }
            let mut collected = collect_documents(ws, &docs, &anchor, opts).await?;
            collected.attachments =
                cover(ws, opts, dir, std::mem::take(&mut collected.attachments)).await;
            collected.verbatim_front_page = true;
            Ok(collected)
        }
        None => {
            // The front page is the site's *frame*, not one of its entries, so
            // it is usually not in the set above: a site's index is typically
            // its view's anchor, and prov scopes a view to the subtree below the
            // anchor. Ship it anyway — a site whose front page was never
            // uploaded serves a 404 at its own root.
            //
            // `site_plan` has already confirmed the gate admits it, which is the
            // check that matters; appending here cannot widen what leaves.
            if let Some(index) = plan.index.as_deref()
                && !docs.iter().any(|(path, _)| path == index)
            {
                docs.push((index.to_path_buf(), true));
            }
            collect_documents(ws, &docs, &anchor, opts).await
        }
    }
}

/// A site's **anchor**: the directory its front page sits in, and therefore the
/// directory every published path is written relative to.
///
/// A vault whose index is its root document anchors at `""`, which rebases
/// nothing — which is the ordinary case, and why anchoring changes no such
/// site's URLs. A site fronted by `www/index.html` anchors at `www`,
/// and its sibling documents publish at `/about/index.html` rather than
/// `/www/about/index.html`: the front page's own links were written from inside
/// that directory, so serving it anywhere else breaks every one of them.
///
/// A site fronted by a covered directory anchors at the directory itself, which
/// is the rebasing `cover` was already doing for the files the manifest
/// claims. Naming it once here is what makes the documents and the assets agree.
pub fn anchor_of(plan: &SitePlan) -> PathBuf {
    match plan.index_directory.as_ref() {
        Some(dir) => dir.root.clone(),
        None => plan
            .index
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    }
}

/// Strip the site's anchor from a workspace-relative path.
///
/// A path outside the anchor is left alone rather than forced under it: it has
/// no site-relative name to be given, and inventing one (`../notes/x.html`)
/// would be a path no object store can hold. It publishes where it always did.
pub fn rebase(path: &Path, anchor: &Path) -> PathBuf {
    if anchor.as_os_str().is_empty() {
        return path.to_path_buf();
    }
    path.strip_prefix(anchor)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

/// The attachments a site fronted by a covered directory ships: the directory's
/// front page, every file its manifest covers, and then whatever the site's
/// remaining pages referenced.
///
/// **The directory is rebased onto the site root.** A row reading `logo.png`
/// under `root: www/` is published at `logo.png`, not `www/logo.png`, because
/// the authored `index.html` being served at the site root asked for it by that
/// name. Rebasing is the whole reason a manifest node makes a usable front page:
/// the directory was written to be served as a site, and it becomes one without
/// a single link inside it being rewritten.
///
/// `referenced` — the ordinary body/frontmatter attachments — is appended after,
/// and a key the directory already claims wins. Losing an authored asset to a
/// collision would break the page a reader lands on; losing a referenced one
/// breaks an image somewhere further in.
async fn cover<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    opts: &CollectOptions<'_>,
    dir: &IndexDirectory,
    referenced: Vec<Attachment>,
) -> Vec<Attachment> {
    let mut out: Vec<Attachment> = Vec::with_capacity(dir.files.len() + referenced.len() + 1);
    let mut claimed: HashSet<String> = HashSet::new();

    // The front page first, so it is the one thing a truncated or failed
    // collection is least likely to be missing.
    for rel in std::iter::once(&dir.front_page).chain(dir.files.iter()) {
        let dest_rel = rel.to_string_lossy().replace('\\', "/");
        if !claimed.insert(dest_rel.clone()) {
            continue;
        }
        // Best-effort, exactly as a referenced attachment is: a row whose file
        // has vanished is drift for `prov manifest` to report, and refusing to
        // publish the site over it would take the whole front page down for one
        // missing photograph.
        if let Some(att) = weigh(ws, opts, dest_rel, dir.root.join(rel)).await {
            out.push(att);
        }
    }

    for att in referenced {
        if claimed.insert(att.dest_rel.clone()) {
            out.push(att);
        }
    }
    out
}

/// Collect an explicit list of documents: read each one, filter its body for
/// the gate's audience, strip the keys that must not travel, stamp whatever the
/// caller asks for, and gather the files they reference.
///
/// The collection body [`collect_site`] runs, exposed because a caller that
/// chooses its documents some other way — a whole-audience walk, a hand-picked
/// set — wants the same treatment of them and must not grow a second copy of
/// it.
///
/// `docs` pairs each document's workspace-relative path with whether it is the
/// site's index page. At most one entry should be flagged; none is legitimate
/// and means the renderer synthesizes a front page.
///
/// `anchor` is the site's [anchor directory](anchor_of): every published path —
/// a source key, a dest page, an attachment — is written relative to it. The
/// *source* paths are rebased too, deliberately: the server hands
/// `plates_render` the object key with the site prefix removed, so a nav link,
/// a `transform_links` in-set test and an emitted href only agree with each
/// other if all three see the same coordinates.
pub async fn collect_documents<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    docs: &[(PathBuf, bool)],
    anchor: &Path,
    opts: &CollectOptions<'_>,
) -> Result<CollectedSite> {
    let mut sources = Vec::with_capacity(docs.len());
    let mut attachments = Vec::new();
    let mut seen_attachments: HashSet<String> = HashSet::new();
    // Which document claimed each published destination, so a second claim on
    // one destination is refused rather than silently overwritten — see
    // [`claim_dest`].
    let mut claimed: HashMap<String, PathBuf> = HashMap::new();
    // What this site publishes as *pages*, so an explicitly listed attachment
    // that is also one of them is not shipped a second time as opaque bytes.
    let page_paths: HashSet<&Path> = docs.iter().map(|(path, _)| path.as_path()).collect();

    for (path, is_root) in docs {
        let is_root = *is_root;
        // Through the graph, not `read_document`: inside the read scope
        // `collect_all` holds, this document was already read and parsed by the
        // walk that decided it belongs here, and prov's memo remembers the
        // *parsed* form. Reading it again by hand — which is what this used to
        // do — bought a second `Document::parse` of every published document,
        // and a third, fourth, … for every site after the first.
        let parsed = ws
            .graph()
            .document(path)
            .await
            .map_err(|e| Error::Document {
                path: path.clone(),
                reason: e.to_string(),
            })?;

        // An attachment sidecar is a document, but it is not a *page*: its body is
        // a JPEG. Rendering one would publish a broken HTML page per attachment,
        // and its payload already publishes on its own as an attachment, reached
        // by the body reference in whatever entry embeds it.
        //
        // This only bites on a `*` audience, which sees every reachable document
        // regardless of what it declares — a named audience never reaches a
        // sidecar, since `attach` gives it no `audience` and visibility is
        // never inherited.
        if parsed.is_attachment() {
            continue;
        }

        // Which *parts* of this document leave. The plan already decided that
        // the document does; this reads the same audience name against the
        // regions inside it, in the document's own grammar.
        //
        // A failure here is fatal to the collection rather than skipped: the
        // filter refuses only when it could not account for a region, and the
        // body it could not account for is the one carrying somebody's private
        // paragraph. Publishing the rest of the site without it would be the
        // one outcome worse than not publishing.
        let format =
            prov::ContentFormat::from_extension(path).unwrap_or(prov::ContentFormat::Markdown);
        let filtered_body = filter_body(&parsed.body, format, Audience::Only(&[opts.audience]))
            .map_err(|reason| Error::Visibility {
                path: path.clone(),
                reason: reason.to_string(),
            })?;

        // The registry id when present, else the document's own frontmatter
        // `id` (authoritative under `id_storage: both`/`frontmatter`, or when an
        // imported vault's registry is stale).
        let id = opts.id_by_path.get(path).cloned().or_else(|| {
            parsed
                .meta
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

        // Strip the keys that must not travel — a collected source is
        // routinely served publicly — and then let the caller add its own. The
        // document on disk is never touched; this shapes the collected copy
        // only. Re-fenced as YAML regardless of the source document's own
        // carrier archetype, because that is the one carrier every consumer of a
        // collected source can read.
        let mut source_fm = parsed
            .meta
            .as_mapping()
            .cloned()
            .unwrap_or_else(prov::meta::Mapping::new);
        for key in opts.strip_keys {
            source_fm.shift_remove(*key);
        }
        opts.stamp.stamp(path, id.as_deref(), &mut source_fm);
        let source_markdown = if source_fm.is_empty() {
            filtered_body.clone()
        } else {
            let yaml =
                prov::meta::serialize_mapping(&source_fm, prov::Format::Yaml).map_err(|e| {
                    Error::Document {
                        path: path.clone(),
                        reason: format!("frontmatter cannot be serialized: {e}"),
                    }
                })?;
            format!("---\n{yaml}---\n{filtered_body}")
        };

        let rebased = rebase(path, anchor);
        let source_rel_path = collected_source_path(path, anchor);
        // The same destination rule `plates_render` renders by, applied here
        // because a caller needs the address before there is a render to read it
        // off. A `serve_at:` claim is **not** rebased onto
        // the site's anchor: it is written from the site's root already, which
        // is the coordinate rebasing produces.
        let dest_path = if is_root {
            "index.html".to_string()
        } else {
            declared_dest(&parsed.meta).unwrap_or_else(|| sanitize_rel_path(&rebased, "html"))
        };
        claim_dest(&mut claimed, &dest_path, path)?;

        // Resolve attachment references: local file refs in the (filtered)
        // body + the frontmatter `attachments` list, canonicalized against
        // this document's path via `prov::link`, non-`.md`.
        let mut refs: Vec<String> = Vec::new();
        for raw in extract_local_file_refs(path, &filtered_body) {
            push_canonical_ref(path, &raw, false, anchor, &page_paths, &mut refs);
        }
        // A frontmatter listing is a *statement of intent*: these bytes ship.
        // So it overrides the grammar filter a body scan is subject to — see
        // [`push_canonical_ref`] — which is what makes an `.html` island
        // shippable as an asset at all.
        if let Some(att) = parsed.meta.get("attachments") {
            for raw in att.link_strings() {
                push_canonical_ref(path, &raw, true, anchor, &page_paths, &mut refs);
            }
        }
        // A page's own `styles:`/`scripts:` are files the site must serve, on
        // exactly an attachment's terms: `plates_render` emits the tags and
        // says so, and copying the file is the caller's — see
        // `plates_render::site::RenderedPage::styles`.
        for key in ["styles", "scripts"] {
            if let Some(listed) = parsed.meta.get(key) {
                for raw in listed.link_strings() {
                    push_canonical_ref(path, &raw, true, anchor, &page_paths, &mut refs);
                }
            }
        }

        for canonical in refs {
            if !seen_attachments.insert(canonical.clone()) {
                continue;
            }
            if let Some(attachment) = weigh_attachment(ws, opts, &canonical, anchor).await {
                attachments.push(attachment);
            }
            // Missing/unreadable attachment: skip. Best-effort on purpose — a
            // broken reference should not fail a whole build.
        }

        sources.push(SourceFile {
            source_markdown,
            source_rel_path,
            dest_path,
            id,
            is_index: is_root,
            backlinks: site_backlinks(
                opts.backlinks.get(path).map_or(&[][..], Vec::as_slice),
                &page_paths,
                anchor,
            ),
        });
    }

    Ok(CollectedSite {
        sources,
        attachments,
        // Set by `collect_site` when the front page turned out to be a covered
        // directory; nothing collected here can supply one.
        verbatim_front_page: false,
    })
}

/// Where a document's collected *source* publishes, in site coordinates:
/// rebased onto the anchor, sanitized, and keeping its own grammar's extension
/// normalized to the canonical spelling (`.markdown` → `.md`, `.djot` → `.dj`,
/// `.htm` → `.html`).
///
/// The extension is load-bearing rather than cosmetic: `plates_render` reads a
/// body's format off it — so flattening every source to `.md`, as this did when
/// markdown was the only grammar, would hand the renderer a Djot body and tell
/// it to parse it as Markdown.
///
/// Named once because two things depend on giving one document one name: the
/// source it is collected as, and every [`SourceFile::backlinks`] entry that
/// points at it. A second spelling of this rule is a link that resolves in one
/// place and not the other.
fn collected_source_path(path: &Path, anchor: &Path) -> String {
    let ext = prov::ContentFormat::from_extension(path)
        .unwrap_or(prov::ContentFormat::Markdown)
        .extension();
    sanitize_rel_path(&rebase(path, anchor), ext)
}

/// The inbound links one document may publish: the archive's, narrowed to the
/// documents this site admits, in this site's coordinates.
///
/// **`admitted` is the disclosure control.** The map handed in is the vault's
/// whole inverted census, so a private note linking to a public one is in it,
/// and a page that listed its inbound links unfiltered would name that note by
/// path and — once the render resolves it — by title. Intersecting with the
/// plan's own set is what stops it, and it is done here rather than downstream
/// because this is the last layer that knows which documents the gate refused.
///
/// prov counts *link sites* — a document that names this one in `related:` and
/// again in a sentence appears twice — and a reader wants the document once, so
/// the result is deduplicated. Sorted, because a rendered page is a build
/// artifact and two builds of one archive have to agree.
fn site_backlinks(
    inbound: &[prov::Backlink],
    admitted: &HashSet<&Path>,
    anchor: &Path,
) -> Vec<String> {
    let mut out: Vec<String> = inbound
        .iter()
        .filter(|link| admitted.contains(link.source.as_path()))
        .map(|link| collected_source_path(&link.source, anchor))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The destination a document's frontmatter `serve_at:` claims, or `None` when
/// it declares none.
///
/// Public so a caller answering the same question answers it the same way — a
/// preview quoting a path a build would not write is a review of a different
/// build.
pub fn declared_dest(meta: &prov::meta::Value) -> Option<String> {
    plates_render::serve_at_dest(meta.get("serve_at")?.as_str()?)
}

/// Record that `path` publishes at `dest`, refusing a destination another
/// document in this site has already claimed.
///
/// Loudly, and naming both documents, because the alternative is what used to
/// happen: two documents resolving to one object key, one of them silently
/// overwriting the other in a published namespace and in a local build alike,
/// with nothing to say which one won. A `serve_at:` makes that reachable on
/// purpose rather than by an accident of sanitization, so it is now an error at
/// the point the claim is made — before anything is uploaded.
fn claim_dest(claimed: &mut HashMap<String, PathBuf>, dest: &str, path: &Path) -> Result<()> {
    match claimed.get(dest) {
        Some(first) => Err(Error::DestinationClaimedTwice {
            dest: dest.to_string(),
            first: first.clone(),
            second: path.to_path_buf(),
        }),
        None => {
            claimed.insert(dest.to_string(), path.to_path_buf());
            Ok(())
        }
    }
}

/// Describe the attachment at workspace-relative `canonical` for the diff,
/// published under that same path.
///
/// The two coordinates coincide for a site anchored at the vault root: a body
/// pointing at `img/scan.jpg` means the site should serve it at `img/scan.jpg`.
/// They part company under an anchor — `www/logo.png` is served at `logo.png` —
/// which is the same rebasing a covered directory has always had. See [`weigh`],
/// which this defers to.
async fn weigh_attachment<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    opts: &CollectOptions<'_>,
    canonical: &str,
    anchor: &Path,
) -> Option<Attachment> {
    let source = PathBuf::from(canonical);
    let dest_rel = rebase(&source, anchor).to_string_lossy().replace('\\', "/");
    weigh(ws, opts, dest_rel, source).await
}

/// Describe the file at workspace-relative `source`, to be published at
/// `dest_rel`: its size, its digest, and its bytes **only if they had to be
/// read**.
///
/// The stat is unavoidable — it is what says the file is there at all, and what
/// a remembered digest is validated against. The read is what this avoids: when
/// the memo recognizes the file at that stat, a diff has everything it needs
/// and the payload stays on disk. That is the whole optimization, and on an
/// archive whose attachments outweigh its prose by two orders of magnitude it
/// is most of what a preview costs.
///
/// The digest is remembered against `source`, not `dest_rel`: the memo is about
/// a file on disk, and the same file published under two names must not be
/// hashed twice.
///
/// `None` for a file that cannot be stat'ed or read — the caller skips it, as
/// it always has.
async fn weigh<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    opts: &CollectOptions<'_>,
    dest_rel: String,
    source: PathBuf,
) -> Option<Attachment> {
    let abs = ws.fs_path(&source);
    let meta = ws.fs().metadata(&abs).await.ok()?;
    let mime_type = mime_type_from_ext(&source);
    let mtime = digest::mtime_ms(&meta);

    if let Some(hash) = opts.digests.recall(&source, meta.len(), mtime) {
        return Some(Attachment {
            dest_rel,
            source_path: source,
            hash,
            len: meta.len(),
            bytes: None,
            mime_type,
        });
    }

    // Nothing remembered: read it, hash it, and say so — so the next preview of
    // this same unchanged file costs a stat.
    let bytes = ws.fs().read(&abs).await.ok()?;
    let hash = (opts.digest)(&bytes);
    opts.digests.remember(&source, meta.len(), mtime, &hash);
    Some(Attachment {
        dest_rel,
        source_path: source,
        hash,
        // The file's own length, not the stat's: a file that grew between the
        // two is described by what was actually read and hashed.
        len: bytes.len() as u64,
        bytes: Some(bytes),
        mime_type,
    })
}

/// Parse `raw` as a link, resolve it against `doc` (workspace-relative
/// coordinates) via `prov::link::resolve`, and push the canonical path onto
/// `out` when it is not itself a link to another *document* (i.e. it's an
/// attachment, not another page).
///
/// "Document" is [`prov::ContentFormat`]'s judgement rather than a `.md` test,
/// so a link to a `.dj` or `.html` page is recognized as a page. Getting this
/// wrong is not cosmetic: a sibling `.dj` note would be collected as an
/// *attachment* and uploaded a second time as an opaque blob.
///
/// `explicit` lifts that filter, for a reference the author **listed** rather
/// than one a body scan found. The two are different claims. A body scan reads
/// every `(path)` in the prose, so treating a link to a sibling page as an asset
/// would republish that page as bytes; a frontmatter `attachments:` entry names
/// one file on purpose, and the file whose extension the filter objects to is
/// usually the whole point — a hand-authored `.html` island, loaded in an
/// `<iframe>`, which has to ship verbatim or not at all.
///
/// `pages` is what the site publishes as rendered documents, and it bounds the
/// lift: a listed file that is *also* one of this site's pages stays a page.
/// Nothing sensible comes of publishing one document twice, once rendered and
/// once as opaque bytes at a key the render is about to write over.
fn push_canonical_ref(
    doc: &Path,
    raw: &str,
    explicit: bool,
    anchor: &Path,
    pages: &HashSet<&Path>,
    out: &mut Vec<String>,
) {
    let link = prov::link::Link::parse(raw);
    let mut canonical = prov::link::resolve(doc, &link.target);
    // A leading `/` means *the site's root*, which under an anchor is not the
    // vault's. `prov::link::resolve` reads it as vault-relative, correctly for
    // prov's own purposes, and `plates_render` reads it as site-relative,
    // correctly for a page that will be served — so the anchor is put back here
    // rather than letting the two disagree about which `/logo.png` is meant.
    // A vault-rooted site anchors at `""`, where this is a no-op.
    if link.target.starts_with('/') && !anchor.as_os_str().is_empty() {
        canonical = anchor.join(canonical);
    }
    if pages.contains(canonical.as_path()) {
        return;
    }
    if explicit || prov::ContentFormat::from_extension(&canonical).is_none() {
        out.push(canonical.to_string_lossy().into_owned());
    }
}

/// Sanitize each component of a relative path and set its extension. Mirrors
/// the publish dest-filename sanitization (keep alphanumerics, spaces, `-`,
/// `_`, `.`). Public so a caller shaping a destination of its own does not
/// duplicate the rule.
pub fn sanitize_rel_path(rel: &Path, ext: &str) -> String {
    let with_ext = rel.with_extension(ext);
    let sanitized: PathBuf = with_ext
        .components()
        .map(|c| match c {
            std::path::Component::Normal(s) => {
                std::ffi::OsString::from(sanitize_component(&s.to_string_lossy()))
            }
            other => other.as_os_str().to_owned(),
        })
        .collect();
    sanitized.to_string_lossy().into_owned()
}

pub fn sanitize_component(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Whether a reference points at a local file (not external URL/anchor/scheme)
/// and has a file extension.
fn is_local_file_ref(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with('#')
        || path.starts_with("mailto:")
        || path.starts_with("data:")
        || path.starts_with("javascript:")
    {
        return false;
    }
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename.contains('.')
}

/// Push `path` if it names a local file and is not already listed. Deduplicating
/// here is what lets the three scans below overlap freely — a `[a](x.png)` is
/// seen by both the parser and the paren scan, and one attachment is meant.
fn push_if_local(paths: &mut Vec<String>, path: &str) {
    if is_local_file_ref(path) && !paths.iter().any(|seen| seen == path) {
        paths.push(path.to_string());
    }
}

/// The runs of `body` outside `spans` (sorted, not necessarily disjoint), in
/// source order. Offsets are dropped: every caller here wants the *text* to
/// scan, never a position in the original.
fn runs_outside<'a>(body: &'a str, spans: &[std::ops::Range<usize>]) -> Vec<&'a str> {
    let mut runs = Vec::new();
    let mut cursor = 0;
    for span in spans {
        if cursor < span.start {
            runs.push(&body[cursor..span.start]);
        }
        cursor = cursor.max(span.end);
    }
    if cursor < body.len() {
        runs.push(&body[cursor..]);
    }
    runs
}

/// Whether a span [`prov::code_spans`] reported is one of `twig`'s *raw HTML*
/// nodes rather than one of its code nodes.
///
/// The two arrive under one kind set and want opposite treatment: a fence is
/// prose about markup, an island is markup, and the attribute scan below exists
/// for the island. Nothing in prov's API separates them, so they are told apart
/// by how the span opens — markdown raw HTML opens on `<`, djot raw carries an
/// `=format` marker on its fence (`` ```=html ``) or after its verbatim span
/// (`` `…`{=html} ``), and every code node is a bare fence, a bare backtick run,
/// or an indented line. A misjudgement here can only re-admit a span the scan
/// used to read anyway, never hide one.
fn is_raw_html_span(body: &str, span: &std::ops::Range<usize>) -> bool {
    let text = &body[span.clone()];
    let after_fence = text.trim_start_matches(['`', '~']);
    text.starts_with('<')
        || body[span.end..].starts_with("{=")
        || (after_fence.len() < text.len() && after_fence.trim_start_matches(' ').starts_with('='))
}

/// Extract local file reference paths from a document body: link targets and
/// HTML `src`/`href`/`srcset` attributes.
///
/// Code-aware, which is the contract that matters: a `(img/a.png)` shown inside
/// a fenced block or an inline code span is documentation, not a reference, and
/// shipping the file it names was a bug. `twig` draws the line — through
/// [`prov::link::scan_body_links`] for wikilinks and parsed markdown/djot links,
/// and [`prov::code_spans`] for the two scans prov has no equivalent of.
///
/// Those two stay lexical for reasons that are not going away. Twig calls
/// `![alt](path)` an *Image*, not a Link, so the paren scan is what sees the
/// commonest attachment there is; and no parser reports HTML attributes, so an
/// `<img src="…">` island — the whole reason an HTML body needs nothing added
/// here — is found by looking for the attribute. Both are held to the code mask
/// instead, which is where grammar-blindness was actually costing something.
///
/// An HTML body has no code mask (twig reports no code nodes for it) and no
/// parsed links, so it is scanned whole, exactly as before.
fn extract_local_file_refs(doc: &Path, body: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let code = prov::ContentFormat::from_extension(doc)
        .and_then(|format| prov::code_spans(body, format).ok())
        .unwrap_or_default();

    // Markdown and Djot both spell a link target `(path)`; an image is only ever
    // spelled that way. Run per non-code run so a `(` in a fence can never pair
    // with a `)` in the prose after it.
    for run in runs_outside(body, &code) {
        let mut remaining = run;
        while let Some(paren_pos) = remaining.find('(') {
            remaining = &remaining[paren_pos + 1..];
            if let Some(close) = remaining.find(')') {
                push_if_local(&mut paths, remaining[..close].trim());
                remaining = &remaining[close + 1..];
            } else {
                break;
            }
        }
    }

    for found in prov::link::scan_body_links(doc, body) {
        push_if_local(&mut paths, found.link.target.trim());
    }

    let attribute_text: Vec<&str> = runs_outside(
        body,
        &code
            .iter()
            .filter(|span| !is_raw_html_span(body, span))
            .cloned()
            .collect::<Vec<_>>(),
    );
    for marker in &["src=\"", "href=\""] {
        for run in &attribute_text {
            let mut remaining = *run;
            while let Some(pos) = remaining.find(marker) {
                remaining = &remaining[pos + marker.len()..];
                if let Some(end) = remaining.find('"') {
                    push_if_local(&mut paths, remaining[..end].trim());
                    remaining = &remaining[end + 1..];
                } else {
                    break;
                }
            }
        }
    }

    for run in &attribute_text {
        let mut remaining = *run;
        while let Some(pos) = remaining.find("srcset=\"") {
            remaining = &remaining[pos + "srcset=\"".len()..];
            if let Some(end) = remaining.find('"') {
                for candidate in remaining[..end].split(',') {
                    let path = candidate.split_whitespace().next().unwrap_or("");
                    push_if_local(&mut paths, path);
                }
                remaining = &remaining[end + 1..];
            } else {
                break;
            }
        }
    }

    paths
}

/// Guess a content type from a file extension.
pub fn mime_type_from_ext(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("html" | "htm") => "text/html",
        // A page's own `styles:`/`scripts:` publish as attachments, and a
        // browser will not apply a stylesheet served as `application/octet-
        // stream`. The three text types a site is actually built out of are
        // named here for that reason.
        Some("css") => "text/css",
        Some("js" | "mjs") => "text/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("ico") => "image/x-icon",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rel_path_swaps_ext_and_strips_unsafe() {
        assert_eq!(
            sanitize_rel_path(Path::new("notes/My Note!.md"), "html"),
            "notes/My Note.html"
        );
        assert_eq!(
            sanitize_rel_path(Path::new("notes/My Note!.md"), "md"),
            "notes/My Note.md"
        );
        assert_eq!(
            sanitize_rel_path(Path::new("Welcome.md"), "html"),
            "Welcome.html"
        );
    }

    #[test]
    fn local_file_refs_filter_external_and_extensionless() {
        assert!(is_local_file_ref("img/a.png"));
        assert!(!is_local_file_ref("https://x.com/a.png"));
        assert!(!is_local_file_ref("#anchor"));
        assert!(!is_local_file_ref("just-text"));
    }

    #[test]
    fn extract_refs_from_markdown_and_html() {
        let md = "![a](img/a.png) and <img src=\"b.jpg\"> and [doc](notes/x.md)";
        let refs = extract_local_file_refs(Path::new("a.md"), md);
        assert!(refs.contains(&"img/a.png".to_string()));
        assert!(refs.contains(&"b.jpg".to_string()));
        assert!(refs.contains(&"notes/x.md".to_string())); // .md filtered later in caller
    }

    /// The bug this scan was code-blind to: a document *about* markup names
    /// files it does not reference, and publishing them shipped bytes nobody
    /// asked for. The reference beside the fence still has to survive, or the
    /// fix would be a worse bug than the one it replaces.
    #[test]
    fn refs_inside_code_are_documentation_not_references() {
        let md = "```\n![x](img/fenced.png)\n<img src=\"fenced.jpg\">\n```\n\n\
                  Inline `(img/inline.png)` and `<img src=\"inline.jpg\">` too.\n\n\
                  ![real](img/real.png) beside <img src=\"real.jpg\">\n";
        let refs = extract_local_file_refs(Path::new("a.md"), md);
        assert_eq!(refs, ["img/real.png", "real.jpg"], "got {refs:?}");
    }

    /// Djot spells its links and fences the same way, so the same rule holds —
    /// and its raw-HTML island, which twig reports under the code kinds, is
    /// still read for attributes.
    #[test]
    fn djot_code_is_skipped_and_its_raw_html_island_is_not() {
        let dj = "```\n[x](img/fenced.png)\n<img src=\"fenced.jpg\">\n```\n\n\
                  `<img src=\"inline.jpg\">`{=html}\n\n\
                  ```=html\n<img src=\"block.jpg\">\n```\n\n[real](img/real.png)\n";
        let refs = extract_local_file_refs(Path::new("a.dj"), dj);
        assert!(!refs.contains(&"img/fenced.png".to_string()), "{refs:?}");
        assert!(!refs.contains(&"fenced.jpg".to_string()), "{refs:?}");
        assert!(refs.contains(&"img/real.png".to_string()), "{refs:?}");
        assert!(refs.contains(&"inline.jpg".to_string()), "{refs:?}");
        assert!(refs.contains(&"block.jpg".to_string()), "{refs:?}");
    }

    /// An HTML body has no fences to be blind to, and twig reports no code nodes
    /// for it — so the attribute scan reads the whole thing, as it always did.
    #[test]
    fn an_html_body_is_still_scanned_whole() {
        let html = "<img src=\"a.png\"><a href=\"b.pdf\">x</a>\
                    <img srcset=\"c.png 1x, d.png 2x\">";
        let refs = extract_local_file_refs(Path::new("a.html"), html);
        assert_eq!(refs, ["a.png", "b.pdf", "c.png", "d.png"], "got {refs:?}");
    }

    /// A wikilink is a reference the paren scan can never see. prov's scanner
    /// reports it, and an Obsidian-dialect vault embeds its images this way.
    #[test]
    fn a_wikilinked_file_is_a_reference() {
        let md = "see ![[img/w.png]] and `[[img/coded.png]]`\n";
        let refs = extract_local_file_refs(Path::new("a.md"), md);
        assert_eq!(refs, ["img/w.png"], "got {refs:?}");
    }

    #[test]
    fn mime_types() {
        assert_eq!(mime_type_from_ext(Path::new("a.png")), "image/png");
        assert_eq!(mime_type_from_ext(Path::new("a.html")), "text/html");
        assert_eq!(
            mime_type_from_ext(Path::new("a.bin")),
            "application/octet-stream"
        );
    }

    /// The other half of the rule: a *body* link to a sibling page is still a
    /// page. Lifting the filter for a listing must not lift it for prose, or
    /// every cross-reference in a vault would upload its target twice.
    #[test]
    fn a_body_link_to_a_sibling_page_is_not_an_attachment() {
        let pages: HashSet<&Path> = HashSet::new();
        let mut out = Vec::new();
        push_canonical_ref(
            Path::new("a.md"),
            "b.md",
            false,
            Path::new(""),
            &pages,
            &mut out,
        );
        assert!(out.is_empty(), "a scanned document link stays a document");
        push_canonical_ref(
            Path::new("a.md"),
            "b.md",
            true,
            Path::new(""),
            &pages,
            &mut out,
        );
        assert_eq!(out, ["b.md"], "a listed one is a statement of intent");
    }

    /// A listed file that is also one of the site's own pages stays a page.
    /// Publishing it twice would put opaque bytes at a key the render is about
    /// to write over.
    #[test]
    fn a_listed_file_that_is_also_a_page_is_not_double_collected() {
        let island = PathBuf::from("chart.html");
        let pages: HashSet<&Path> = [island.as_path()].into_iter().collect();
        let mut out = Vec::new();
        push_canonical_ref(
            Path::new("a.md"),
            "chart.html",
            true,
            Path::new(""),
            &pages,
            &mut out,
        );
        assert!(out.is_empty());
    }

    /// A workspace in memory, so the collector can be run against a real
    /// [`prov::Workspace`] — a census, a graph and all — without this crate
    /// growing a dev-dependency or touching a disk.
    fn vault(docs: &[(&str, &str)]) -> Workspace<prov::InMemoryFs> {
        let fs = prov::InMemoryFs::default();
        for (path, text) in docs {
            prov::block_on(fs.write_atomic(&Path::new("/vault").join(path), text.as_bytes()))
                .unwrap();
        }
        Workspace::builder(fs).root("/vault").build()
    }

    fn collected_backlinks(
        ws: &Workspace<prov::InMemoryFs>,
        admitted: &[&str],
    ) -> HashMap<String, Vec<String>> {
        let backlinks = prov::block_on(ws.backlinks("index.md")).unwrap();
        let docs: Vec<(PathBuf, bool)> = admitted
            .iter()
            .map(|p| (PathBuf::from(p), *p == "index.md"))
            .collect();
        let site = prov::block_on(collect_documents(
            ws,
            &docs,
            Path::new(""),
            &CollectOptions {
                audience: "public",
                strip_keys: &[],
                stamp: &NoStamp,
                id_by_path: &HashMap::new(),
                backlinks: &backlinks,
                digests: &crate::digest::NoDigests,
                digest: |_| String::new(),
            },
        ))
        .unwrap();
        site.sources
            .into_iter()
            .map(|s| (s.source_rel_path, s.backlinks))
            .collect()
    }

    /// Both halves of prov's census reach the collected source: a frontmatter
    /// relation and a link written in prose are two inbound references to one
    /// document, and the document is named once for them.
    #[test]
    fn a_relation_and_a_body_link_are_one_backlink() {
        let ws = vault(&[
            ("index.md", "---\ncontents:\n- a.md\n- b.md\n---\nHome.\n"),
            (
                "a.md",
                "---\ntitle: Alpha\npart_of: index.md\nrelated:\n- b.md\n---\nAnd again in [prose](b.md).\n",
            ),
            ("b.md", "---\ntitle: Beta\npart_of: index.md\n---\nB.\n"),
        ]);

        let backlinks = collected_backlinks(&ws, &["index.md", "a.md", "b.md"]);

        assert_eq!(
            backlinks["b.md"],
            ["a.md", "index.md"],
            "the relation and the prose link name a.md once; index.md's \
             `contents:` is an inbound reference like any other"
        );
    }

    /// The disclosure control. prov's map is the *vault's*, so a document the
    /// gate refused is in it and links out of it — and the page it links to
    /// must not name it, by path or by anything the render would resolve from
    /// one.
    #[test]
    fn a_linker_this_site_does_not_admit_is_not_named() {
        let ws = vault(&[
            (
                "index.md",
                "---\ncontents:\n- b.md\n- private.md\n---\nHome.\n",
            ),
            ("b.md", "---\ntitle: Beta\npart_of: index.md\n---\nB.\n"),
            (
                "private.md",
                "---\ntitle: Secret\npart_of: index.md\n---\nSee [Beta](b.md).\n",
            ),
        ]);

        // The census sees private.md — it is reachable — which is the point:
        // the filter is doing work rather than agreeing with an empty map.
        let census = prov::block_on(ws.backlinks("index.md")).unwrap();
        assert!(
            census[Path::new("b.md")]
                .iter()
                .any(|l| l.source == Path::new("private.md")),
            "{census:?}"
        );

        let backlinks = collected_backlinks(&ws, &["index.md", "b.md"]);

        assert_eq!(backlinks["b.md"], ["index.md"], "{backlinks:?}");
    }
}
