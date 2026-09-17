//! Mounting a peer: a site that carries the sites of the workspaces it names.
//!
//! An archive's published page draws a foreign spanning edge —
//! `[fig](id:fig/b9j9zgk)` — into a workspace the device knows where to find.
//! This module opens that workspace, plans the export there that answers to the
//! same gate, collects it exactly as `plates build` inside the peer would, and
//! **mounts** the result under the peer's name: `/fig/`. The origin site is the
//! union; each peer's own build is unchanged. `docs/proposals/mounting-a-peer.md`
//! argues the shape; what follows is the rule it lands on.
//!
//! # The rule
//!
//! A followed foreign edge *from a page the site publishes* mounts the peer's
//! export *for the same audience* at `/<name>/`. Each clause is load-bearing:
//!
//! - **Followed.** prov's [`descend`] decides, under the trust the caller set.
//!   A refused edge — unknown, unconfirmed, mismatched, a URL, too deep — is a
//!   warning here and stays what an unfollowed edge already is to the render: a
//!   link to a page this site does not publish.
//! - **From a published page.** A private page's foreign edge mounts nothing,
//!   for the reason its path links publish nothing. The origin's plan decides
//!   which pages those are, and a mounted peer's plan decides for the edges
//!   its own pages draw, so a mount nests to the depth the descent allowed.
//! - **The same audience.** The export in the peer's config whose gate field
//!   and value equal the origin site's — one named like the origin site first,
//!   then the only one. The audience name is the whole contract between the
//!   two workspaces, which is the contract prov's gate already makes.
//! - **At `/<name>/`.** The workspace's declared name, which is what the
//!   reference was spelled with, so `id:fig/…` in a source and `/fig/` in the
//!   address bar are one fact seen twice.
//!
//! # What is and is not carried
//!
//! Sources, attachments, both directions of link edge, and the outline, every
//! coordinate prefixed by the mount — that is [`CollectOptions::mount`]'s job,
//! and this module only sets it. A `serve_at:` claim is written from the site
//! root and is not prefixed. The peer's theme is not mounted: one site, one
//! frame, and the origin's shell surrounds every page.
//!
//! Nothing is written across a boundary, no registry is read as if it were the
//! origin's, and a mounted document's identity stays the peer's.
//! [`CollectedSite`] gains no field: a mounted source is a [`SourceFile`] whose
//! path happens to start with the peer's name.
//!
//! [`SourceFile`]: crate::source::SourceFile

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use prov::crossing::Node;
use prov::{Boundary, Descent, IdIndex, PeerResolver, Refusal, Storage, Workspace, descend};

use crate::collect::{
    CollectOptions, RegistryLinks, anchor_of, collect_site, collected_source_path,
};
use crate::error::{Error, Result};
use crate::plan::plan_site;
use crate::source::{Attachment, CollectedSite};
use crate::spec::{SitePlan, SiteSpec};
use crate::term::read_term_config;

/// How far a mount reaches, and on whose say-so.
#[derive(Clone, Copy)]
pub struct MountOptions<'a> {
    /// Where the other workspaces are — this device's map, as the host loads
    /// it. `prov::PeerFile` is the CLI's; a host with its own map implements
    /// the same port.
    pub peers: &'a dyn PeerResolver,
    /// prov's descent: how many boundaries may be crossed, and how much doubt
    /// about a peer's identity is accepted. The default follows only confirmed
    /// peers, eight crossings deep.
    pub descent: Descent,
}

/// One peer a mount reached, and what it contributed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountReport {
    /// The peer's name — the last segment of [`prefix`](Self::prefix).
    pub name: String,
    /// The directory below the site root its pages landed in, with the trailing
    /// slash: `fig/`, or `fig/schema/` for a mount a mounted peer made.
    pub prefix: String,
    /// The peer's root directory, as the resolver gave it.
    pub root_dir: PathBuf,
    /// The name of the export mounted, under the peer's own `exports:`.
    pub export: String,
    /// How many pages it contributed.
    pub pages: usize,
}

/// A site with its mounts collected in: the union, and an account of how it
/// was made.
#[derive(Debug, Clone, Default)]
pub struct Mounted {
    /// The origin's collected site with every mounted peer's pages, attachments,
    /// edges and outline folded in under their prefixes. A caller renders this
    /// exactly as it renders a site with no mounts.
    pub site: CollectedSite,
    /// Each peer mounted, in the order the walk reached them.
    pub mounts: Vec<MountReport>,
    /// What could not be mounted and why — a refused edge, a peer with no
    /// export for this audience, a page-level shell a mount does not honour.
    /// Never fatal: a site whose neighbour is missing is still a site.
    pub warnings: Vec<String>,
}

/// One workspace whose pages share the site: the origin, or a peer that was
/// mounted. The walk over the federated tree keys everything on prov's
/// workspace index so a peer reached twice is one realm.
struct Realm {
    /// The site-relative directory this workspace's pages land in.
    prefix: String,
    /// Its anchor, in its own coordinates.
    anchor: PathBuf,
    /// The pages it publishes, in its own coordinates.
    published: HashSet<PathBuf>,
}

/// Collect a site and every peer it mounts.
///
/// `spec`, `plan` and `root_doc` are the origin site's, as [`collect_site`]
/// would take them; `opts` is the origin's collection options, and what this
/// function changes about them for each collection is written on
/// [`CollectOptions::mount`] and [`CollectOptions::id_links`] — the rest travel
/// as given, so the audience, the stamp and the digest memo are one policy
/// across the whole site. [`CollectOptions::spanning_root`] is taken over: the
/// outline of a mounted site is the federated tree, built here, and the inner
/// collections walk none of their own.
///
/// A peer that opens but cannot be planned is [`Error::Mount`], naming it,
/// because the fix is a declaration in the other repository and building the
/// site without it would publish a hole where a neighbour was expected.
pub async fn collect_mounted<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    spec: &SiteSpec,
    plan: &SitePlan,
    root_doc: &Path,
    opts: &CollectOptions<'_>,
    mount: &MountOptions<'_>,
) -> Result<Mounted> {
    let federation = descend(ws, root_doc, mount.peers, &mount.descent)
        .await
        .map_err(|e| Error::Descent(e.to_string()))?;

    let mut warnings = Vec::new();
    let mut realms: HashMap<usize, Realm> = HashMap::new();
    realms.insert(
        0,
        Realm {
            prefix: String::new(),
            anchor: anchor_of(plan),
            published: published_set(plan),
        },
    );

    // Every peer's registry, in site coordinates, before any body is collected:
    // a page in the origin resolving `id:fig/…` needs fig's table, and a page
    // in fig resolving `id:org/…` needs the origin's.
    let mut foreign: HashMap<(String, String), String> = HashMap::new();
    if !ws.workspace_id().is_empty() {
        let realm = &realms[&0];
        for (path, id) in opts.id_by_path {
            foreign.insert(
                (ws.workspace_id().to_string(), id.clone()),
                format!("/{}", collected_source_path(path, &realm.anchor)),
            );
        }
    }

    // The peers, opened and planned. Breadth-first over the federated tree from
    // the origin, so a nested mount's prefix is known before it is planned and
    // a peer reached from two pages is planned once, under the first.
    struct Opened<FS> {
        index: usize,
        name: String,
        prefix: String,
        peer: prov::Peer<FS>,
        spec: SiteSpec,
        plan: SitePlan,
        id_by_path: HashMap<PathBuf, String>,
    }
    let mut opened: Vec<Opened<FS>> = Vec::new();
    let mut queue: Vec<(usize, &Node)> = vec![(0, &federation.tree)];
    while !queue.is_empty() {
        let mut next = Vec::new();
        for (realm_index, subtree) in queue.drain(..) {
            let parent_prefix = realms[&realm_index].prefix.clone();
            for (parent, edge) in foreign_edges(subtree, realm_index) {
                if !realms[&realm_index].published.contains(parent) {
                    continue;
                }
                let into = match &edge.boundary {
                    Some(Boundary::Followed { into }) => *into,
                    Some(Boundary::Refused(refusal)) => {
                        warnings.push(format!(
                            "{}: {} was not followed — {}",
                            parent.display(),
                            edge.path.display(),
                            describe(refusal)
                        ));
                        continue;
                    }
                    None => continue,
                };
                if realms.contains_key(&into) {
                    // Reached again, by another page or a deeper path. One
                    // mount per peer, at the first address it was given.
                    continue;
                }
                let reached = &federation.workspaces[into];
                let name = reached.name.clone();
                let prefix = format!("{parent_prefix}{name}/");

                // Re-opened rather than handed back by the descent, which
                // memoizes its peers privately; one open per peer either way.
                let peer = match prov::open_peer(ws.fs(), mount.peers, &name, mount.descent.trust)
                    .await
                    .map_err(|e| Error::Descent(e.to_string()))?
                {
                    prov::Crossing::Opened(peer) => peer,
                    prov::Crossing::Refused(refusal) => {
                        warnings.push(format!(
                            "{}: {name} was not followed — {}",
                            parent.display(),
                            describe(&refusal)
                        ));
                        continue;
                    }
                };

                let Some(export) = matching_export(&peer.discovered.config.exports, spec) else {
                    warnings.push(format!(
                        "{name} declares no export gated on {}: {:?}, so nothing of it is \
                         mounted at /{prefix}",
                        spec.gate_field(),
                        spec.audience
                    ));
                    continue;
                };
                let peer_root_doc = peer.discovered.root_doc.clone();
                let (peer_spec, term_warnings) = SiteSpec::from_export(export).with_term_config(
                    read_term_config(
                        &peer.workspace,
                        &peer_root_doc,
                        &peer.discovered.config,
                        spec.gate_field(),
                        spec.audience.trim(),
                    )
                    .await,
                );
                warnings.extend(term_warnings.into_iter().map(|w| format!("{name}: {w}")));

                let census = peer
                    .workspace
                    .census(&peer_root_doc)
                    .await
                    .unwrap_or_default();
                let peer_plan = plan_site(
                    &peer.workspace,
                    &peer_spec,
                    &peer.discovered.config.views,
                    &peer_root_doc,
                    &census,
                )
                .await
                .map_err(|e| Error::Mount {
                    workspace: name.clone(),
                    reason: e.to_string(),
                })?;
                for diagnostic in &peer_plan.link_diagnostics {
                    warnings.push(format!("{name}: {diagnostic}"));
                }

                let anchor = anchor_of(&peer_plan);
                let id_by_path: HashMap<PathBuf, String> = peer
                    .workspace
                    .index()
                    .iter()
                    .map(|(id, path)| (path.clone(), id.as_str().to_string()))
                    .collect();
                for (path, id) in &id_by_path {
                    foreign.insert(
                        (name.clone(), id.clone()),
                        format!("/{prefix}{}", collected_source_path(path, &anchor)),
                    );
                }
                realms.insert(
                    into,
                    Realm {
                        prefix: prefix.clone(),
                        anchor,
                        published: published_set(&peer_plan),
                    },
                );
                next.push((into, edge));
                opened.push(Opened {
                    index: into,
                    name,
                    prefix,
                    peer,
                    spec: peer_spec,
                    plan: peer_plan,
                    id_by_path,
                });
            }
        }
        queue = next;
    }

    // The origin, in its own coordinates, with the whole table to hand.
    let origin_links = RegistryLinks::new(
        ws.workspace_id(),
        opts.id_by_path,
        &realms[&0].anchor,
        "",
        &foreign,
    );
    let mut site = collect_site(ws, plan, &with(opts, &origin_links, "")).await?;
    let mut claimed: HashMap<String, String> = site
        .sources
        .iter()
        .map(|s| (s.dest_path.clone(), s.source_rel_path.clone()))
        .collect();
    let mut attachment_keys: HashSet<String> = site
        .attachments
        .iter()
        .map(|a| a.dest_rel.clone())
        .collect();

    let mut mounts = Vec::new();
    for o in &opened {
        let realm = &realms[&o.index];
        let backlinks = o
            .peer
            .workspace
            .backlinks(&o.peer.discovered.root_doc)
            .await
            .unwrap_or_default();
        let census = o
            .peer
            .workspace
            .census(&o.peer.discovered.root_doc)
            .await
            .unwrap_or_default();
        let links = RegistryLinks::new(
            o.peer.declares(),
            &o.id_by_path,
            &realm.anchor,
            &o.prefix,
            &foreign,
        );
        let inner = CollectOptions {
            audience: opts.audience,
            gate_field: opts.gate_field,
            strip_keys: opts.strip_keys,
            stamp: opts.stamp,
            id_by_path: &o.id_by_path,
            backlinks: &backlinks,
            census: &census,
            spanning_root: None,
            digests: opts.digests,
            digest: opts.digest,
            id_links: &links,
            mount: &o.prefix,
        };
        let collected = collect_site(&o.peer.workspace, &o.plan, &inner)
            .await
            .map_err(|e| Error::Mount {
                workspace: o.name.clone(),
                reason: e.to_string(),
            })?;

        let pages = collected.sources.len();
        for source in collected.sources {
            if let Some(first) = claimed.get(&source.dest_path) {
                return Err(Error::DestinationClaimedTwice {
                    dest: source.dest_path.clone(),
                    first: PathBuf::from(first),
                    second: PathBuf::from(&source.source_rel_path),
                });
            }
            claimed.insert(source.dest_path.clone(), source.source_rel_path.clone());
            site.sources.push(source);
        }
        for attachment in collected.attachments {
            // A key the site already ships wins, as it does within one site.
            if !attachment_keys.insert(attachment.dest_rel.clone()) {
                continue;
            }
            site.attachments.push(Attachment {
                source_path: relocate(
                    &o.peer.discovered.root_dir,
                    ws.root(),
                    &attachment.source_path,
                ),
                ..attachment
            });
        }
        // A peer fronted by a covered directory ships its front page as bytes
        // under the mount, which the loop above already carried; the
        // `verbatim_front_page` flag is the *site's* and stays the origin's.
        mounts.push(MountReport {
            name: o.name.clone(),
            prefix: o.prefix.clone(),
            root_dir: o.peer.discovered.root_dir.clone(),
            export: o.spec.name.clone(),
            pages,
        });
    }

    // One outline: the federated tree, each node in the coordinates of the
    // realm it belongs to. A node in a workspace that was reached but not
    // mounted keeps the reference as written and prunes on arrival, exactly as
    // an unfollowed foreign leaf always has.
    site.outline = vec![outline(&federation.tree, &realms)];

    Ok(Mounted {
        site,
        mounts,
        warnings,
    })
}

/// The origin's options with the two per-collection fields replaced.
fn with<'a>(
    opts: &'a CollectOptions<'a>,
    id_links: &'a RegistryLinks<'a>,
    mount: &'a str,
) -> CollectOptions<'a> {
    CollectOptions {
        audience: opts.audience,
        gate_field: opts.gate_field,
        strip_keys: opts.strip_keys,
        stamp: opts.stamp,
        id_by_path: opts.id_by_path,
        backlinks: opts.backlinks,
        census: opts.census,
        spanning_root: None,
        digests: opts.digests,
        digest: opts.digest,
        id_links,
        mount,
    }
}

/// The pages a plan publishes: its entries, and its front page when that is a
/// document.
fn published_set(plan: &SitePlan) -> HashSet<PathBuf> {
    let mut set: HashSet<PathBuf> = plan.entries.iter().map(|d| d.path.clone()).collect();
    if let (Some(index), None) = (&plan.index, &plan.index_directory) {
        set.insert(index.clone());
    }
    set
}

/// Every foreign edge drawn inside one workspace's part of the federated tree:
/// the node that drew it, and the node it landed on. Stops at each boundary —
/// what lies past a followed edge is the peer's part, walked when the peer is
/// mounted, and never when it is not.
fn foreign_edges(node: &Node, workspace: usize) -> Vec<(&Path, &Node)> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        for child in &node.children {
            if child.boundary.is_some() {
                out.push((node.path.as_path(), child));
            } else if child.workspace == workspace {
                stack.push(child);
            }
        }
    }
    out.reverse();
    out
}

/// The export a peer publishes to this site's audience, if it has one: the one
/// sharing the site's *name* when several answer to the gate, else the only one
/// that does.
fn matching_export<'a>(
    exports: &'a [prov::ExportSpec],
    spec: &SiteSpec,
) -> Option<&'a prov::ExportSpec> {
    let gated: Vec<&prov::ExportSpec> = exports
        .iter()
        .filter(|e| {
            e.gate.field == spec.gate_field() && e.gate.value.trim() == spec.audience.trim()
        })
        .collect();
    gated
        .iter()
        .copied()
        .find(|e| e.name == spec.name)
        .or_else(|| (gated.len() == 1).then(|| gated[0]))
}

/// A mounted attachment's `source_path`, made readable from the origin: the
/// peer's file relative to the origin root when the peer sits inside it — the
/// org-and-checkouts shape — and absolute otherwise, so a caller's
/// `root.join(source_path)` reaches the file in both cases.
fn relocate(peer_root: &Path, origin_root: &Path, source: &Path) -> PathBuf {
    let abs = peer_root.join(source);
    abs.strip_prefix(origin_root)
        .map(Path::to_path_buf)
        .unwrap_or(abs)
}

/// One federated node as the render layer's outline node, in the coordinates
/// of the realm it belongs to.
fn outline(node: &Node, realms: &HashMap<usize, Realm>) -> plates_render::OutlineNode {
    let refused = matches!(node.boundary, Some(Boundary::Refused(_)));
    let path = match realms.get(&node.workspace) {
        // Reached but not mounted, or refused: the reference as written, which
        // names no page — sanitizing it would only make it look like one.
        None => node.path.to_string_lossy().into_owned(),
        Some(_) if refused => node.path.to_string_lossy().into_owned(),
        Some(realm) => format!(
            "{}{}",
            realm.prefix,
            collected_source_path(&node.path, &realm.anchor)
        ),
    };
    plates_render::OutlineNode {
        path,
        label: node.label.clone(),
        children: node.children.iter().map(|c| outline(c, realms)).collect(),
    }
}

/// Why a boundary was not crossed, for a warning.
fn describe(refusal: &Refusal) -> String {
    match refusal {
        Refusal::Unknown => "no peer by that name on this device".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use prov::exports::{ExportSpec, Gate};
    use prov::{PeerLocation, PeerLookup};

    use crate::collect::{NoIdLinks, NoStamp};
    use crate::digest::NoDigests;

    fn export(name: &str, value: &str) -> ExportSpec {
        ExportSpec {
            name: name.into(),
            label: None,
            gate: Gate {
                field: "audience".into(),
                value: value.into(),
            },
            hold: None,
            view: None,
        }
    }

    fn site(name: &str, audience: &str) -> SiteSpec {
        SiteSpec::from_export(&export(name, audience))
    }

    #[test]
    fn the_export_sharing_the_sites_name_wins_else_the_only_match() {
        let exports = [export("docs", "public"), export("www", "public")];
        assert_eq!(
            matching_export(&exports, &site("www", "public")).map(|e| &e.name),
            Some(&"www".to_string())
        );
        // Two answer and neither shares the name: ambiguous, so none.
        assert!(matching_export(&exports, &site("site", "public")).is_none());
        let one = [export("docs", "public"), export("team", "staff")];
        assert_eq!(
            matching_export(&one, &site("www", "public")).map(|e| &e.name),
            Some(&"docs".to_string())
        );
        assert!(matching_export(&one, &site("www", "family")).is_none());
    }

    /// A resolver that answers from a table without checking — the check
    /// [`prov::open_peer`] makes on the declared name is what the test relies on.
    struct Peers(HashMap<String, PathBuf>);

    impl PeerResolver for Peers {
        fn locate(&self, workspace: &str) -> PeerLookup {
            match self.0.get(workspace) {
                Some(root) => PeerLookup::Confirmed(PeerLocation::Path(root.clone())),
                None => PeerLookup::Unknown,
            }
        }
    }

    fn write(fs: &prov::InMemoryFs, path: &str, text: &str) {
        prov::block_on(fs.write_atomic(Path::new(path), text.as_bytes())).unwrap();
    }

    /// An org at `/org` whose public front page names the peer `fig` at
    /// `/org/fig`; fig publishes a two-page site of its own, fronted through a
    /// reified `public` term, with one image and links both ways.
    fn org_and_fig() -> (prov::InMemoryFs, Peers) {
        let fs = prov::InMemoryFs::default();
        write(
            &fs,
            "/org/README.md",
            "---\ntitle: Org\nid: kv2bv2m\ncontents:\n- '[Site](/www/index.md)'\n- '[fig](id:fig/b9j9zgk)'\n---\n",
        );
        write(
            &fs,
            "/org/www/index.md",
            "---\ntitle: Diaryx\nid: x2q521g\naudience: public\npart_of: '[Org](/README.md)'\ncontents:\n- '[About](/www/about.md)'\n- '[fig](id:fig/b9j9zgk)'\n- '[twig](id:twig/9tz497d)'\n---\nFront.\n",
        );
        write(
            &fs,
            "/org/www/about.md",
            "---\ntitle: About\nid: 4sptdg2\naudience: public\npart_of: '[Diaryx](/www/index.md)'\n---\nSee [fig](id:fig/87w2xwr) and [fig's docs](id:fig/qh0d737).\n",
        );
        // Not a page the site publishes, so its edge to twig mounts nothing.
        write(
            &fs,
            "/org/notes.md",
            "---\ntitle: Notes\ncontents:\n- '[twig](id:twig/9tz497d)'\n---\n",
        );

        write(
            &fs,
            "/org/fig/prov.yaml",
            "workspace_id: fig\nroot: README.md\nid_storage: frontmatter\nexports:\n  docs:\n    gate:\n      field: audience\n      value: public\nfields:\n  audience:\n    values: closed\n    vocabulary: '[Audiences](/vocab/audiences.md)'\n    reify: true\n",
        );
        write(
            &fs,
            "/org/fig/README.md",
            "---\ntitle: fig\nid: b9j9zgk\npart_of: id:org/kv2bv2m\nconfig: prov.yaml\ncontents:\n- '[Site](/www/index.md)'\n- '[Audiences](/vocab/audiences.md)'\n---\n",
        );
        write(
            &fs,
            "/org/fig/vocab/audiences.md",
            "---\ntitle: Audiences\nid: a0d1en1\npart_of: '[fig](/README.md)'\ncontents:\n- '[Public](/vocab/public.md)'\n---\n",
        );
        write(
            &fs,
            "/org/fig/vocab/public.md",
            "---\ntitle: Public\nid: p0b1ic1\nterm: public\npart_of: '[Audiences](/vocab/audiences.md)'\nfront_page: '[fig](/www/index.md)'\n---\n",
        );
        write(
            &fs,
            "/org/fig/www/index.md",
            "---\ntitle: fig\nid: 87w2xwr\naudience: public\npart_of: '[fig](/README.md)'\ncontents:\n- '[Docs](/www/docs.md)'\n---\n![logo](logo.png) Back to [about](id:org/4sptdg2), on to [docs](id:qh0d737).\n",
        );
        write(
            &fs,
            "/org/fig/www/docs.md",
            "---\ntitle: Docs\nid: qh0d737\naudience: public\npart_of: '[fig](/www/index.md)'\n---\nDocs.\n",
        );
        write(&fs, "/org/fig/www/logo.png", "\u{89}PNG");

        let peers = Peers(
            [("fig".to_string(), PathBuf::from("/org/fig"))]
                .into_iter()
                .collect(),
        );
        (fs, peers)
    }

    #[test]
    fn a_published_edge_mounts_the_peers_export_under_its_name() {
        let (fs, peers) = org_and_fig();
        let ws: Workspace<prov::InMemoryFs> = Workspace::builder(fs)
            .root("/org")
            .workspace_id("org")
            .build();
        let spec = SiteSpec {
            index: Some("[Site](/www/index.md)".into()),
            ..site("www", "public")
        };
        let root = Path::new("README.md");
        let census = prov::block_on(ws.census(root)).unwrap();
        let backlinks = prov::block_on(ws.backlinks(root)).unwrap();
        let plan = prov::block_on(plan_site(&ws, &spec, &[], root, &census)).unwrap();
        let id_by_path: HashMap<PathBuf, String> = [
            ("README.md", "kv2bv2m"),
            ("www/index.md", "x2q521g"),
            ("www/about.md", "4sptdg2"),
        ]
        .into_iter()
        .map(|(p, id)| (PathBuf::from(p), id.to_string()))
        .collect();
        let opts = CollectOptions {
            audience: "public",
            gate_field: "audience",
            strip_keys: &[],
            stamp: &NoStamp,
            id_by_path: &id_by_path,
            backlinks: &backlinks,
            census: &census,
            spanning_root: Some(root),
            digests: &NoDigests,
            digest: |_| String::new(),
            id_links: &NoIdLinks,
            mount: "",
        };

        let mounted = prov::block_on(collect_mounted(
            &ws,
            &spec,
            &plan,
            root,
            &opts,
            &MountOptions {
                peers: &peers,
                descent: Descent::default(),
            },
        ))
        .unwrap();

        // fig was mounted once, under its name, from its `docs` export.
        assert_eq!(mounted.mounts.len(), 1, "{:?}", mounted.warnings);
        let fig = &mounted.mounts[0];
        assert_eq!((fig.name.as_str(), fig.prefix.as_str()), ("fig", "fig/"));
        assert_eq!(fig.export, "docs");
        assert_eq!(fig.pages, 2);

        // twig was named by a published page and is not on this device: said,
        // not silent. Named by a private page too, which says nothing.
        assert_eq!(mounted.warnings.len(), 1, "{:?}", mounted.warnings);
        assert!(
            mounted.warnings[0].contains("twig"),
            "{:?}",
            mounted.warnings
        );

        let by_path: BTreeMap<&str, &crate::source::SourceFile> = mounted
            .site
            .sources
            .iter()
            .map(|s| (s.source_rel_path.as_str(), s))
            .collect();
        assert_eq!(
            by_path.keys().copied().collect::<Vec<_>>(),
            vec!["about.md", "fig/docs.md", "fig/index.md", "index.md"]
        );
        assert_eq!(by_path["fig/index.md"].dest_path, "fig/index.html");
        assert!(by_path["index.md"].is_index && !by_path["fig/index.md"].is_index);

        // Links across the boundary, both ways, in site coordinates.
        assert!(
            by_path["about.md"]
                .source_markdown
                .contains("[fig](/fig/index.md) and [fig's docs](/fig/docs.md)"),
            "{}",
            by_path["about.md"].source_markdown
        );
        let fig_index = &by_path["fig/index.md"].source_markdown;
        assert!(fig_index.contains("[about](/about.md)"), "{fig_index}");
        assert!(fig_index.contains("[docs](/fig/docs.md)"), "{fig_index}");

        // The peer's attachment, keyed under the mount and readable from the
        // origin root.
        let logo = &mounted.site.attachments[0];
        assert_eq!(logo.dest_rel, "fig/logo.png");
        assert_eq!(logo.source_path, PathBuf::from("fig/www/logo.png"));

        // One outline: the org's tree with fig's hung where the edge was drawn.
        let org = &mounted.site.outline[0];
        assert_eq!(org.path, "README.md");
        let front = &org.children[0];
        assert_eq!(front.path, "index.md");
        assert_eq!(front.children[0].path, "about.md");
        let fig_node = &front.children[1];
        assert_eq!(fig_node.path, "fig/README.md");
        assert_eq!(fig_node.children[0].path, "fig/index.md");
        assert_eq!(fig_node.children[0].children[0].path, "fig/docs.md");
        // The refused edge stays as written and names no page.
        assert_eq!(front.children[2].path, "id:twig/9tz497d");
    }

    #[test]
    fn a_peer_inside_the_origin_relocates_relative_and_one_outside_absolute() {
        assert_eq!(
            relocate(
                Path::new("/org/fig"),
                Path::new("/org"),
                Path::new("www/logo.png")
            ),
            PathBuf::from("fig/www/logo.png")
        );
        assert_eq!(
            relocate(
                Path::new("/elsewhere/fig"),
                Path::new("/org"),
                Path::new("www/logo.png")
            ),
            PathBuf::from("/elsewhere/fig/www/logo.png")
        );
    }
}
