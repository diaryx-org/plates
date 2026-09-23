//! Planning a site: the set its gate admits, narrowed by its view's scope,
//! fronted by its declared index.
//!
//! The composition — and the invariant that a view can only ever narrow — is
//! [`prov::exports::plan()`]'s. What this module supplies is the two things
//! prov's planner does not have: the `index:` link, resolved through the
//! spanning relation so the front page survives a rename, a move and a retitle
//! exactly as a view's anchor does; and what that index turns out to *be*, since
//! a front page that resolves to a [manifest](prov::manifest) node fronts the
//! site with a whole directory rather than a rendered page.
//!
//! Everything here reads a [`prov::Workspace`] and nothing else. There is no
//! config parsing: `views` and `root_doc` arrive as arguments, because which
//! dialect a vault declares its views in is the caller's business and reading
//! one would tie this crate to a vocabulary.

use std::path::{Path, PathBuf};

use prov::Target;
use prov::link::Link;
use prov::{IdIndex, Storage, ViewSpec, Workspace};

use crate::error::{Error, Result};
use crate::spec::{FRONT_PAGE, IndexDirectory, SitePlan, SiteSpec, finish};

/// Plan one site against a workspace.
///
/// `views` is the pool `spec.view` is resolved against, and `root_doc` is the
/// document the spanning walk starts from — both the caller's to supply, for
/// the reason in the module docs.
///
/// `census` is [`prov::Workspace::census`]'s, and it is a parameter rather than
/// something taken here because a build plans every site the archive declares
/// against one archive: the resolutions do not differ between sites, and taking
/// the census per site would walk the whole graph once per site to learn the
/// same thing. A caller with no use for [`SitePlan::link_diagnostics`] passes
/// `&[]`.
///
/// `own_page` is the audience's own page — the term node its gate value names,
/// [`TermConfig::page`](crate::TermConfig::page) — or `None` for an archive
/// whose audiences are not documents. It is always shared with the audience it
/// describes, whatever its own `audience:` says, and it is the site's front
/// page whenever the site names no other.
pub async fn plan_site<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    spec: &SiteSpec,
    views: &[ViewSpec],
    root_doc: &Path,
    census: &[prov::CensusEntry],
    own_page: Option<&Path>,
) -> Result<SitePlan> {
    // Resolve the index first: a front page that points at nothing is a mistake
    // worth reporting before spending a whole tree walk on the plan it would
    // invalidate.
    let index = match &spec.index {
        Some(target) => Some(resolve_index(ws, &spec.name, target, root_doc).await?),
        None => None,
    };
    // …and then ask what it *is*. A front page that turns out to be a manifest
    // node fronts the site with a whole directory rather than a rendered page,
    // and every layer downstream needs to know that before it tries to render a
    // node with no body.
    let index_directory = match index.as_deref() {
        Some(path) => index_directory(ws, &spec.name, path).await?,
        None => None,
    };

    let export = prov::exports::plan(ws.graph(), &to_export(spec), views, root_doc)
        .await
        .map_err(|e| match e {
            // prov's "this export names a view nobody declared" is this layer's
            // `UnresolvedSiteView` — the same mistake, and a caller already
            // knows how to say it.
            prov::exports::Error::ViewUnknown { view, .. } => Error::UnresolvedSiteView {
                site: spec.name.clone(),
                view,
            },
            other => Error::Export(other.to_string()),
        })?;

    // Whether the own page may front the site. Not when the archive's root is
    // already shared with this audience: a vault whose root its readers can
    // see opens where it always did. And not as a draft — the own page is
    // outside the gate, admitted by what it is rather than by what it says, so
    // the hold the gate applies never saw it.
    let root_shared = export.entries.iter().any(|doc| doc.path == root_doc)
        || export.outside_view.iter().any(|path| path == root_doc);
    let fronting = match own_page {
        Some(page) if !root_shared && !holds(ws, spec, page).await => Some(page),
        _ => None,
    };

    let mut plan = finish(
        spec,
        export,
        index.as_deref(),
        index_directory,
        fronting,
        census,
    )?;
    // Recorded whether or not it fronts the site: a tagged own page published
    // as an ordinary entry still keeps its site settings home.
    plan.own_page = own_page.map(Path::to_path_buf);
    Ok(plan)
}

/// Whether the site's hold field keeps `page` back: `draft: true`, or the
/// string `"true"`, on the page itself.
async fn holds<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    spec: &SiteSpec,
    page: &Path,
) -> bool {
    let Some(field) = spec.hold.as_deref() else {
        return false;
    };
    let Ok(doc) = ws.graph().document(page).await else {
        return false;
    };
    match doc.meta.get(field) {
        Some(prov::meta::Value::Bool(held)) => *held,
        Some(prov::meta::Value::String(text)) => text.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// The field a site's gate is judged on when it names none.
///
/// `audience` is the default because that is the field the product treats as a
/// disclosure control — closed vocabulary, private by default, surfaced in the
/// editor as a permission rather than a tag. It is only a default: prov's gate
/// names its own field, so [`SiteSpec::gate_field`] carries a vault that spells
/// its disclosure control something else, and nothing below this line assumes
/// the name.
pub const AUDIENCE_FIELD: &str = "audience";

/// The export half of a site's declaration — what prov stores and plans.
pub fn to_export(spec: &SiteSpec) -> prov::exports::ExportSpec {
    prov::exports::ExportSpec {
        name: spec.name.clone(),
        label: spec.label.clone(),
        gate: prov::exports::Gate {
            field: spec.gate_field().to_string(),
            value: spec.audience.trim().to_string(),
        },
        hold: spec.hold.clone(),
        view: spec.view.clone(),
    }
}

/// Resolve a site's `index:` link to a workspace-relative path, erroring when it
/// points at nothing.
///
/// Split out so an unresolvable link and a link to a document the site does not
/// publish stay distinguishable — they have different causes and different
/// fixes.
async fn resolve_index<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    site: &str,
    target: &str,
    root_doc: &Path,
) -> Result<PathBuf> {
    let unresolved = || Error::UnresolvedSiteIndex {
        site: site.to_string(),
        target: target.to_string(),
    };
    let link = Link::parse(target);
    let Target::Path(path) = ws.resolve_link(root_doc, &link) else {
        return Err(unresolved());
    };
    if !ws.fs().try_exists(&ws.fs_path(&path)).await? {
        return Err(unresolved());
    }
    Ok(path)
}

/// The directory a site's front page covers, when the front page is a
/// [manifest](prov::manifest) node — `None` when it is an ordinary document,
/// which is the usual case.
///
/// The manifest's `root` is written relative to the manifest document, so it is
/// resolved back to workspace coordinates here; the rows are already relative to
/// that root and stay as they are, because the root is exactly the frame the
/// authored front page's own links were written against.
///
/// A covered directory with no [`FRONT_PAGE`] is refused rather than fronted
/// with a synthesized listing — see
/// [`Error::SiteIndexDirectoryHasNoFrontPage`].
async fn index_directory<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    site: &str,
    index: &Path,
) -> Result<Option<IndexDirectory>> {
    let Some((manifest_doc, manifest)) = ws.manifest_of(index).await? else {
        return Ok(None);
    };
    let root = prov::link::resolve(&manifest_doc, &manifest.root);
    let front_page = PathBuf::from(FRONT_PAGE);
    if !ws
        .fs()
        .try_exists(&ws.fs_path(root.join(&front_page)))
        .await?
    {
        return Err(Error::SiteIndexDirectoryHasNoFrontPage {
            site: site.to_string(),
            root,
            front_page: FRONT_PAGE.to_string(),
        });
    }
    Ok(Some(IndexDirectory {
        root,
        front_page,
        files: manifest.files.into_iter().map(|row| row.path).collect(),
    }))
}
