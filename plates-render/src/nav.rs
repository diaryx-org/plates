//! Site navigation tree construction.
//!
//! [`build_site_nav_tree`] builds the whole-site tree from the archive's
//! spanning outline; [`nav_for_page`] specializes that tree for a single page
//! (marking current/ancestor nodes and computing breadcrumbs). Both are pure
//! functions over [`PublishedPage`] and [`OutlineNode`].
//!
//! # Where containment comes from
//!
//! A vault's spine is *configured*: prov's `spanning:` names the relation whose
//! links contain, and `contents:`/`part_of:` is one dialect's spelling of it.
//! This crate reads no configuration, so the layer holding the workspace walks
//! that relation and hands the result down as an [`OutlineNode`] forest — the
//! same tree prov's own outline view is built from, so a site and the archive it
//! came from cannot disagree about what contains what.
//!
//! With no outline supplied the nav falls back to each page's
//! `contents_links`/`parent_link`, which is the diaryx spelling resolved at
//! parse time. That is a *fallback*, not the second half of a policy: it keeps a
//! caller that has only sources — a preview, a test, an edge worker handed
//! nothing else — rendering the nav it always did.
//!
//! # Why the site is a forest, not a tree
//!
//! This module used to root the nav at the one page with `is_root` and descend
//! only `contents_links`, which was sound while audience visibility was
//! inherited: the visible set was then always a connected subtree containing the
//! workspace root.
//!
//! Visibility is now explicit-only — a document is visible to exactly the
//! audiences it declares, and an undeclared one is private — the gate is the
//! caller's to declare. A render set is therefore an *arbitrary
//! subset* of the containment tree: a published entry whose parent is private is
//! the normal case, not the edge case. Descending from a single root left every
//! such entry with a URL, a sitemap row and a feed item, but no place in the
//! sidebar.
//!
//! So containment survives where it survives — a visible parent still nests its
//! visible children — and every page the walk cannot reach becomes a root of its
//! own. The invariant is that **every page in the render set appears exactly
//! once in the nav**, pinned by `every_page_appears_exactly_once` below.

use std::collections::{HashMap, HashSet};

use crate::links::sanitize_rel_path;
use crate::types::{NavLink, OutlineNode, PublishedPage, SiteNavNode, SiteNavigation};

/// How deep the nav renders before it stops descending.
///
/// Not a safety bound — a page is placed at most once (tracked in `placed`), so
/// a containment cycle cannot loop this walk. It is a readability bound
/// on a sidebar, raised from the 3 the single-rooted tree used because a forest
/// hangs its roots one level lower.
const MAX_NAV_DEPTH: usize = 6;

/// Build a site navigation tree from all published pages.
///
/// `outline` is the archive's spanning tree, pruned here to the pages this site
/// publishes; an empty one falls back to each page's own
/// `contents_links`/`parent_link` (see the module docs). Containment that
/// survived audience filtering nests; every page the walk from the index cannot
/// reach becomes its own root, so nothing in the render set is missing from the
/// nav. Filters out `hide_from_nav` pages and sorts siblings by `nav_order` (if
/// present), then by the order the containing document declared them in.
///
/// With no page marked `is_root` the forest is returned bare — a caller that
/// synthesizes a front page (`site::synthesize_index`) hands it back here as a
/// page with `is_root` set, and the forest nests under it on the next call.
pub fn build_site_nav_tree(pages: &[PublishedPage], outline: &[OutlineNode]) -> Vec<SiteNavNode> {
    let containment = Containment::of(pages, outline);

    // Every page placed so far, so no page is listed twice and a containment
    // cycle terminates.
    let mut placed: HashSet<&str> = HashSet::new();

    let root = pages.iter().find(|p| p.is_root);
    if let Some(r) = root {
        placed.insert(r.dest_filename.as_str());
    }

    // 1. Everything reachable from the index through surviving containment.
    let mut top: Vec<SiteNavNode> = match root {
        Some(r) => build_children(r, &containment, &mut placed, 0),
        None => Vec::new(),
    };

    // 2. Forest roots: a page whose container is not in this render set
    //    (audience filtering dropped it) is the root of its own subtree. Source
    //    order, `nav_order` overriding.
    let mut orphans: Vec<(i32, SiteNavNode)> = Vec::new();
    for (idx, page) in pages.iter().enumerate() {
        if page.hide_from_nav || placed.contains(page.dest_filename.as_str()) {
            continue;
        }
        if !containment.is_forest_root(page) {
            continue;
        }
        placed.insert(page.dest_filename.as_str());
        let children = build_children(page, &containment, &mut placed, 1);
        orphans.push((
            page.nav_order.unwrap_or(idx as i32),
            node_for(page, None, children),
        ));
    }
    orphans.sort_by_key(|(key, _)| *key);
    top.extend(orphans.into_iter().map(|(_, node)| node));

    // 3. Anything still unplaced — a page whose container *is* in the set but
    //    never listed it (a one-sided link), a page below a `hide_from_nav`
    //    container, or a member of a cycle the walk entered elsewhere. Listing
    //    it flat is worse than nesting it and better than dropping it: a
    //    published page missing from its own site's nav is the failure this
    //    rewrite exists to end.
    for page in pages {
        if page.hide_from_nav || placed.contains(page.dest_filename.as_str()) {
            continue;
        }
        placed.insert(page.dest_filename.as_str());
        top.push(node_for(page, None, Vec::new()));
    }

    match root {
        Some(r) => vec![node_for(r, None, top)],
        None => top,
    }
}

/// The pages that start subtrees of their own: they have no container, or a
/// container this site does not publish. In source order, `hide_from_nav`
/// excluded.
///
/// The nav's own answer, exposed because the generated front page lists exactly
/// this set (`site::synthesize_index`). Two derivations of "what is top level
/// here" is one that will drift, and the drift is a front page listing entries
/// the sidebar nests three levels down.
pub fn forest_roots<'p>(
    pages: &'p [PublishedPage],
    outline: &[OutlineNode],
) -> Vec<&'p PublishedPage> {
    let containment = Containment::of(pages, outline);
    pages
        .iter()
        .filter(|p| !p.hide_from_nav && containment.is_forest_root(p))
        .collect()
}

/// One containment edge an outline leaves after pruning, in the source-path
/// coordinates an [`OutlineNode`] names.
pub struct OutlineEdge<'o> {
    /// The containing page.
    pub container: String,
    /// The page it contains.
    pub contained: String,
    /// The label the container's link carried (`[Label](path)`), when any.
    pub label: Option<&'o str>,
}

/// The containment edges an outline leaves once the documents a site does not
/// publish are pruned out of it, in declaration order.
///
/// `admits` answers whether this site publishes the page at a sanitized source
/// path. A node it refuses is no edge and no nav entry — the gate held it back,
/// or the view scoped it out — so what hung below it hoists to the nearest
/// ancestor the site *does* publish, and to nothing at all when there is none.
/// That is the placement a dropped `part_of` used to produce, arrived at from
/// the archive's side instead of the document's.
///
/// Public, and shared, because a site answers "what contains what" twice from
/// one outline: the nav tree below, and the `parent`/`children`/`breadcrumbs` a
/// template reads (`site::collect_context`). Two walks of one outline is one
/// walk that will drift, and the drift is a trail that contradicts the sidebar
/// printed beside it.
pub fn pruned_edges<'o>(
    outline: &'o [OutlineNode],
    admits: &dyn Fn(&str) -> bool,
) -> Vec<OutlineEdge<'o>> {
    fn walk<'o>(
        node: &'o OutlineNode,
        under: Option<&str>,
        admits: &dyn Fn(&str) -> bool,
        out: &mut Vec<OutlineEdge<'o>>,
    ) {
        let path = sanitize_rel_path(&node.path);
        let mine = admits(&path).then_some(path);
        if let Some(path) = &mine
            && let Some(container) = under
        {
            out.push(OutlineEdge {
                container: container.to_string(),
                contained: path.clone(),
                label: node.label.as_deref(),
            });
        }
        let under = mine.as_deref().or(under);
        for child in &node.children {
            walk(child, under, admits, out);
        }
    }

    let mut out = Vec::new();
    for node in outline {
        walk(node, None, admits, &mut out);
    }
    out
}

/// Which page contains which, in declaration order.
///
/// The two spellings of that question — the archive's spanning outline, and each
/// page's own resolved `contents`/`part_of` — are both reduced to this, so the
/// placement above reads one shape and never learns a vocabulary.
#[derive(Default)]
struct Containment<'p> {
    /// Each page's children, keyed by destination.
    children: HashMap<&'p str, Vec<Child<'p>>>,
    /// Each contained page's container, keyed by destination. A page missing
    /// from this map starts a subtree of its own.
    parent: HashMap<&'p str, &'p str>,
}

/// One contained page, with what the container's link called it.
struct Child<'p> {
    page: &'p PublishedPage,
    /// The link's own label, used only when the page carries no `nav_title` —
    /// and before its `title`, because a container that troubled to name its
    /// child meant that name to be read here.
    label: Option<&'p str>,
}

impl<'p> Containment<'p> {
    /// Read containment from the outline when there is one, and from the pages'
    /// own links when there is not.
    fn of(pages: &'p [PublishedPage], outline: &'p [OutlineNode]) -> Self {
        match outline.is_empty() {
            true => Self::from_links(pages),
            false => Self::from_outline(pages, outline),
        }
    }

    /// Containment as each page resolved it out of its own frontmatter — the
    /// fallback for a caller that supplies no outline.
    fn from_links(pages: &'p [PublishedPage]) -> Self {
        let by_dest: HashMap<&str, &PublishedPage> = pages
            .iter()
            .map(|p| (p.dest_filename.as_str(), p))
            .collect();

        let mut out = Self::default();
        for page in pages {
            // A link whose target is not in the render set is **dropped**, not
            // rendered from the link text: it was excluded by audience
            // filtering, and emitting a nav entry for it would both leak the
            // excluded page's title and 404 on click.
            let children: Vec<Child> = page
                .contents_links
                .iter()
                .filter_map(|link| {
                    by_dest.get(link.href.as_str()).map(|child| Child {
                        page: child,
                        label: Some(link.title.as_str()),
                    })
                })
                .collect();
            if !children.is_empty() {
                out.children.insert(page.dest_filename.as_str(), children);
            }
            if let Some(parent) = &page.parent_link
                && let Some(container) = by_dest.get(parent.href.as_str())
            {
                out.parent.insert(
                    page.dest_filename.as_str(),
                    container.dest_filename.as_str(),
                );
            }
        }
        out
    }

    /// Containment as the archive declared it, pruned to what this site
    /// publishes.
    ///
    /// An outline node naming a document the site does not publish is not a nav
    /// entry — it was held back by the gate, or scoped out by the view — so its
    /// published descendants hoist to the nearest ancestor that *is* published,
    /// and to the forest when there is none. That is the same placement a
    /// dropped `part_of` used to produce, arrived at from the archive's side
    /// instead of the document's.
    fn from_outline(pages: &'p [PublishedPage], outline: &'p [OutlineNode]) -> Self {
        // Keyed by source path, since that is what an outline node names. First
        // claim wins: a synthesized front page carries a source path it never
        // read, and must not take a real document's place in the tree.
        let mut by_source: HashMap<String, &PublishedPage> = HashMap::new();
        for page in pages {
            by_source
                .entry(sanitize_rel_path(&page.source_path.to_string_lossy()))
                .or_insert(page);
        }

        let edges = pruned_edges(outline, &|path| by_source.contains_key(path));

        let mut out = Self::default();
        for edge in edges {
            let (Some(container), Some(page)) = (
                by_source.get(&edge.container).copied(),
                by_source.get(&edge.contained).copied(),
            ) else {
                continue;
            };
            out.children
                .entry(container.dest_filename.as_str())
                .or_default()
                .push(Child {
                    page,
                    label: edge.label,
                });
            // A document reachable by two spanning paths is materialized twice;
            // the first branch that reaches it is where it lives, which is what
            // prov's own tree shows.
            out.parent
                .entry(page.dest_filename.as_str())
                .or_insert(container.dest_filename.as_str());
        }
        out
    }

    /// Whether a page starts its own subtree: nothing in this render set
    /// contains it.
    fn is_forest_root(&self, page: &PublishedPage) -> bool {
        !self.parent.contains_key(page.dest_filename.as_str())
    }

    fn children_of(&self, page: &PublishedPage) -> &[Child<'p>] {
        self.children
            .get(page.dest_filename.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

/// A nav node for `page`. `label` supplies the display title when the page was
/// reached through a link that carried its own; otherwise the page's
/// `nav_title`/`title` is used.
fn node_for(page: &PublishedPage, label: Option<&str>, children: Vec<SiteNavNode>) -> SiteNavNode {
    let title = page
        .nav_title
        .clone()
        .or_else(|| label.map(str::to_string))
        .unwrap_or_else(|| page.title.clone());
    SiteNavNode {
        title,
        href: page.dest_filename.clone(),
        is_current: false,
        is_ancestor_of_current: false,
        children,
    }
}

/// Build `page`'s children from the containment, recursively.
fn build_children<'p>(
    page: &'p PublishedPage,
    containment: &Containment<'p>,
    placed: &mut HashSet<&'p str>,
    depth: usize,
) -> Vec<SiteNavNode> {
    if depth >= MAX_NAV_DEPTH {
        return Vec::new();
    }

    let mut children: Vec<(i32, SiteNavNode)> = Vec::new();

    for (idx, Child { page: child, label }) in containment.children_of(page).iter().enumerate() {
        if child.hide_from_nav || placed.contains(child.dest_filename.as_str()) {
            continue;
        }
        placed.insert(child.dest_filename.as_str());

        let sub_children = build_children(child, containment, placed, depth + 1);
        let sort_key = child.nav_order.unwrap_or(idx as i32);
        children.push((sort_key, node_for(child, *label, sub_children)));
    }

    // Sort by nav_order (encoded in sort_key), stable for equal keys.
    children.sort_by_key(|(key, _)| *key);
    children.into_iter().map(|(_, node)| node).collect()
}

/// Build navigation context (tree with current-page marking + breadcrumbs) for a specific page.
///
/// The breadcrumbs are the current page's ancestors **in the tree**, not a
/// second walk of its `part_of` chain: a site whose spine is spelled some other
/// way has no such chain to walk, and a trail that disagreed with the sidebar
/// beside it would be wrong even where it exists. A page the tree does not hold
/// — `hide_from_nav` — still gets the chain, since it is the only answer left.
pub fn nav_for_page(
    tree: &[SiteNavNode],
    current_dest: &str,
    pages: &[PublishedPage],
) -> SiteNavigation {
    // Deep-clone and mark current + ancestors
    fn mark_current(nodes: &[SiteNavNode], target: &str) -> (Vec<SiteNavNode>, bool) {
        let mut result = Vec::with_capacity(nodes.len());
        let mut found = false;

        for node in nodes {
            let (children, child_found) = mark_current(&node.children, target);
            let is_current = node.href == target;
            let is_ancestor = child_found;

            if is_current || is_ancestor {
                found = true;
            }

            result.push(SiteNavNode {
                title: node.title.clone(),
                href: node.href.clone(),
                is_current,
                is_ancestor_of_current: is_ancestor,
                children,
            });
        }

        (result, found)
    }

    let (marked_tree, _) = mark_current(tree, current_dest);

    // The trail down to the page, read off the tree the sidebar is showing.
    let mut breadcrumbs = Vec::new();
    if trail_to(tree, current_dest, &mut breadcrumbs) {
        return SiteNavigation {
            tree: marked_tree,
            breadcrumbs,
        };
    }

    // …and, for a page the tree does not hold, its own `part_of` chain. Only
    // `hide_from_nav` lands here: every other published page is in the nav
    // exactly once, which is the invariant `every_page_appears_exactly_once`
    // pins.
    let page_map: HashMap<&str, &PublishedPage> = pages
        .iter()
        .map(|p| (p.dest_filename.as_str(), p))
        .collect();

    if let Some(current_page) = page_map.get(current_dest) {
        // Walk up parent chain
        let mut chain = vec![NavLink {
            href: current_page.dest_filename.clone(),
            title: current_page
                .nav_title
                .clone()
                .unwrap_or_else(|| current_page.title.clone()),
        }];

        let mut visited = HashSet::new();
        visited.insert(current_dest.to_string());

        let mut cursor = current_page.parent_link.as_ref();
        while let Some(parent) = cursor {
            if !visited.insert(parent.href.clone()) {
                break; // cycle guard
            }
            chain.push(NavLink {
                href: parent.href.clone(),
                title: page_map
                    .get(parent.href.as_str())
                    .and_then(|p| p.nav_title.clone())
                    .unwrap_or_else(|| parent.title.clone()),
            });
            cursor = page_map
                .get(parent.href.as_str())
                .and_then(|p| p.parent_link.as_ref());
        }

        chain.reverse();
        breadcrumbs = chain;
    }

    SiteNavigation {
        tree: marked_tree,
        breadcrumbs,
    }
}

/// Push the trail from a tree root down to `target` onto `into`, target last.
/// `false` — and `into` untouched — when no root reaches it.
fn trail_to(nodes: &[SiteNavNode], target: &str, into: &mut Vec<NavLink>) -> bool {
    for node in nodes {
        into.push(NavLink {
            href: node.href.clone(),
            title: node.title.clone(),
        });
        if node.href == target || trail_to(&node.children, target, into) {
            return true;
        }
        into.pop();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PageLayout;
    use std::path::PathBuf;

    fn make_page(
        dest: &str,
        title: &str,
        is_root: bool,
        contents: Vec<NavLink>,
        parent: Option<NavLink>,
    ) -> PublishedPage {
        PublishedPage {
            source_path: PathBuf::from(format!("/workspace/{}", dest.replace(".html", ".md"))),
            dest_filename: dest.to_string(),
            title: title.to_string(),
            rendered_body: String::new(),
            markdown_body: String::new(),
            contents_links: contents,
            parent_link: parent,
            is_root,
            description: None,
            author: None,
            created: None,
            updated: None,
            date_of_document: None,
            group_keys: vec![],
            attachments: vec![],
            styles: vec![],
            scripts: vec![],
            layout: PageLayout::default(),
            shell: None,
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
            id: None,
            source_markdown: String::new(),
        }
    }

    #[test]
    fn test_nav_tree_flat_workspace() {
        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "a.html".into(),
                        title: "A".into(),
                    },
                    NavLink {
                        href: "b.html".into(),
                        title: "B".into(),
                    },
                ],
                None,
            ),
            make_page(
                "a.html",
                "A",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
            make_page(
                "b.html",
                "B",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].title, "Home");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].title, "A");
        assert_eq!(tree[0].children[1].title, "B");
    }

    #[test]
    fn test_nav_tree_deep_hierarchy() {
        let pages = vec![
            make_page(
                "index.html",
                "Root",
                true,
                vec![NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }],
                None,
            ),
            make_page(
                "parent.html",
                "Parent",
                false,
                vec![NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Root".into(),
                }),
            ),
            make_page(
                "child.html",
                "Child",
                false,
                vec![NavLink {
                    href: "grandchild.html".into(),
                    title: "Grandchild".into(),
                }],
                Some(NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }),
            ),
            make_page(
                "grandchild.html",
                "Grandchild",
                false,
                vec![],
                Some(NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        assert_eq!(tree[0].children.len(), 1); // Parent
        assert_eq!(tree[0].children[0].children.len(), 1); // Child
        assert_eq!(tree[0].children[0].children[0].children.len(), 1); // Grandchild
        // Depth 3: grandchild's children are empty (max depth reached)
        assert_eq!(
            tree[0].children[0].children[0].children[0].children.len(),
            0
        );
    }

    #[test]
    fn test_nav_tree_hide_from_nav() {
        let mut hidden_page = make_page(
            "hidden.html",
            "Hidden",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        hidden_page.hide_from_nav = true;

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "visible.html".into(),
                        title: "Visible".into(),
                    },
                    NavLink {
                        href: "hidden.html".into(),
                        title: "Hidden".into(),
                    },
                ],
                None,
            ),
            make_page(
                "visible.html",
                "Visible",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
            hidden_page,
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].title, "Visible");
    }

    #[test]
    fn test_nav_tree_nav_order() {
        let mut page_b = make_page(
            "b.html",
            "B",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_b.nav_order = Some(1);

        let mut page_a = make_page(
            "a.html",
            "A",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_a.nav_order = Some(2);

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "a.html".into(),
                        title: "A".into(),
                    },
                    NavLink {
                        href: "b.html".into(),
                        title: "B".into(),
                    },
                ],
                None,
            ),
            page_a,
            page_b,
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        // B has nav_order 1, A has 2 — B should come first
        assert_eq!(tree[0].children[0].title, "B");
        assert_eq!(tree[0].children[1].title, "A");
    }

    #[test]
    fn test_nav_tree_nav_title() {
        let mut page_a = make_page(
            "a.html",
            "Full Title of A",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_a.nav_title = Some("Short A".to_string());

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![NavLink {
                    href: "a.html".into(),
                    title: "Full Title of A".into(),
                }],
                None,
            ),
            page_a,
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        assert_eq!(tree[0].children[0].title, "Short A");
    }

    /// Collect every href in a nav tree, depth-first.
    fn hrefs(nodes: &[SiteNavNode]) -> Vec<String> {
        let mut out = Vec::new();
        for n in nodes {
            out.push(n.href.clone());
            out.extend(hrefs(&n.children));
        }
        out
    }

    fn parent(href: &str) -> Option<NavLink> {
        Some(NavLink {
            href: href.into(),
            title: href.replace(".html", ""),
        })
    }

    fn link(href: &str) -> NavLink {
        NavLink {
            href: href.into(),
            title: href.replace(".html", ""),
        }
    }

    /// A page whose parent was excluded by audience filtering (so `parent_link`
    /// is `None`) becomes a root of its own instead of vanishing from the nav.
    /// This is the ordinary shape under explicit-only visibility.
    #[test]
    fn an_orphaned_page_becomes_its_own_root() {
        let pages = vec![
            make_page("index.html", "Home", true, vec![link("a.html")], None),
            make_page("a.html", "A", false, vec![], parent("index.html")),
            // Published, but its `part_of` target is private — dropped upstream.
            make_page("orphan.html", "Orphan", false, vec![], None),
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        assert_eq!(tree.len(), 1, "one site root");
        let top: Vec<&str> = tree[0].children.iter().map(|n| n.href.as_str()).collect();
        assert_eq!(top, ["a.html", "orphan.html"]);
    }

    /// An orphan carries its own subtree with it — hierarchy that survived
    /// filtering still nests, it just hangs from a new root.
    #[test]
    fn an_orphan_keeps_the_children_that_survived() {
        let pages = vec![
            make_page("index.html", "Home", true, vec![], None),
            make_page("daily.html", "Daily", false, vec![link("mon.html")], None),
            make_page("mon.html", "Monday", false, vec![], parent("daily.html")),
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        let daily = &tree[0].children[0];
        assert_eq!(daily.href, "daily.html");
        assert_eq!(daily.children.len(), 1);
        assert_eq!(daily.children[0].href, "mon.html");
    }

    /// **The invariant.** Every page in the render set is in the nav, once.
    /// Covers all three placement passes: reachable by containment, orphaned,
    /// and reachable-in-principle but never listed by its parent.
    #[test]
    fn every_page_appears_exactly_once() {
        let pages = vec![
            make_page("index.html", "Home", true, vec![link("a.html")], None),
            make_page(
                "a.html",
                "A",
                false,
                vec![link("a-kid.html")],
                parent("index.html"),
            ),
            make_page("a-kid.html", "A Kid", false, vec![], parent("a.html")),
            // Orphan — parent filtered out.
            make_page("orphan.html", "Orphan", false, vec![], None),
            // Parent is in the set but never listed this page in `contents`.
            make_page("unlisted.html", "Unlisted", false, vec![], parent("a.html")),
        ];

        let found = hrefs(&build_site_nav_tree(&pages, &[]));
        let unique: HashSet<&String> = found.iter().collect();
        assert_eq!(found.len(), unique.len(), "no page listed twice: {found:?}");
        for page in &pages {
            assert!(
                unique.contains(&page.dest_filename),
                "{} is missing from the nav",
                page.dest_filename
            );
        }
    }

    /// A `contents`/`part_of` cycle terminates and still places each page once —
    /// the `placed` set does the work a depth cap used to.
    #[test]
    fn a_containment_cycle_terminates() {
        let pages = vec![
            make_page("a.html", "A", false, vec![link("b.html")], parent("b.html")),
            make_page("b.html", "B", false, vec![link("a.html")], parent("a.html")),
        ];

        let found = hrefs(&build_site_nav_tree(&pages, &[]));
        assert_eq!(found.len(), 2, "each page once: {found:?}");
        assert!(found.contains(&"a.html".to_string()));
        assert!(found.contains(&"b.html".to_string()));
    }

    /// A `contents` link whose target is not in the render set is dropped, not
    /// rendered from the link text — it names a page this audience may not see,
    /// so showing its title would leak it and clicking it would 404.
    #[test]
    fn a_link_to_a_page_outside_the_set_is_dropped() {
        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![link("here.html"), link("excluded.html")],
                None,
            ),
            make_page("here.html", "Here", false, vec![], parent("index.html")),
        ];

        let found = hrefs(&build_site_nav_tree(&pages, &[]));
        assert!(!found.iter().any(|h| h == "excluded.html"), "{found:?}");
        assert_eq!(found.len(), 2);
    }

    /// With nothing marked `is_root` the forest comes back bare, for a caller
    /// that is about to synthesize a front page for it.
    #[test]
    fn a_rootless_set_returns_a_bare_forest() {
        let pages = vec![
            make_page("a.html", "A", false, vec![], None),
            make_page("b.html", "B", false, vec![], None),
        ];
        let tree = build_site_nav_tree(&pages, &[]);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].href, "a.html");
        assert_eq!(tree[1].href, "b.html");
    }

    #[test]
    fn test_nav_for_page_marks_current_and_ancestors() {
        let pages = vec![
            make_page(
                "index.html",
                "Root",
                true,
                vec![NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }],
                None,
            ),
            make_page(
                "parent.html",
                "Parent",
                false,
                vec![NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Root".into(),
                }),
            ),
            make_page(
                "child.html",
                "Child",
                false,
                vec![],
                Some(NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages, &[]);
        let nav = nav_for_page(&tree, "child.html", &pages);

        // Root should be ancestor
        assert!(nav.tree[0].is_ancestor_of_current);
        assert!(!nav.tree[0].is_current);

        // Parent should be ancestor
        assert!(nav.tree[0].children[0].is_ancestor_of_current);
        assert!(!nav.tree[0].children[0].is_current);

        // Child should be current
        assert!(nav.tree[0].children[0].children[0].is_current);
        assert!(!nav.tree[0].children[0].children[0].is_ancestor_of_current);

        // Breadcrumbs: Root → Parent → Child
        assert_eq!(nav.breadcrumbs.len(), 3);
        assert_eq!(nav.breadcrumbs[0].title, "Root");
        assert_eq!(nav.breadcrumbs[1].title, "Parent");
        assert_eq!(nav.breadcrumbs[2].title, "Child");
    }

    // ── the archive's own outline ───────────────────────────────────────────

    /// An outline node, with no label of its own.
    fn node(path: &str, children: Vec<OutlineNode>) -> OutlineNode {
        OutlineNode {
            path: path.to_string(),
            label: None,
            children,
        }
    }

    /// `page`, as the document at `source` — the name an outline node knows it
    /// by, since a node names a source and a nav node names a destination.
    fn sourced(mut page: PublishedPage, source: &str) -> PublishedPage {
        page.source_path = PathBuf::from(source);
        page
    }

    /// A vault whose spine is *not* spelled `contents:`/`part_of:` — prov's
    /// `spanning:` names the relation, and a vault is free to name one this
    /// crate has never heard of. These pages carry no containment frontmatter at
    /// all, so the only thing that can nest them is the outline the workspace
    /// layer walked.
    #[test]
    fn the_nav_follows_the_configured_spanning_relation() {
        let pages = vec![
            sourced(
                make_page("index.html", "Home", true, vec![], None),
                "index.md",
            ),
            sourced(
                make_page("field.html", "Field Notes", false, vec![], None),
                "field.md",
            ),
            sourced(
                make_page("monday.html", "Monday", false, vec![], None),
                "field/monday.md",
            ),
        ];

        // Nothing in the documents nests them: flat under the front page.
        assert_eq!(
            hrefs(&build_site_nav_tree(&pages, &[])),
            ["index.html", "field.html", "monday.html"]
        );

        let outline = vec![node(
            "index.md",
            vec![node("field.md", vec![node("field/monday.md", vec![])])],
        )];
        let tree = build_site_nav_tree(&pages, &outline);

        let field = &tree[0].children[0];
        assert_eq!(field.href, "field.html");
        assert_eq!(field.children.len(), 1, "the hierarchy the archive has");
        assert_eq!(field.children[0].href, "monday.html");

        // …and the trail down to a page is the trail the sidebar shows, rather
        // than a second walk of a `part_of` chain this vault never wrote.
        let nav = nav_for_page(&tree, "monday.html", &pages);
        let trail: Vec<&str> = nav.breadcrumbs.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(trail, ["Home", "Field Notes", "Monday"]);
    }

    /// A node the site does not publish is not a place in the nav — the gate
    /// held it back — so what hangs below it hoists to the nearest ancestor that
    /// *is* published, rather than vanishing with it.
    #[test]
    fn a_pruned_container_hoists_the_pages_below_it() {
        let pages = vec![
            sourced(
                make_page("index.html", "Home", true, vec![], None),
                "index.md",
            ),
            sourced(
                make_page("letters.html", "Letters", false, vec![], None),
                "letters.md",
            ),
            sourced(
                make_page("1943.html", "1943", false, vec![], None),
                "letters/private/1943.md",
            ),
        ];
        // `letters/private.md` is in the archive's tree and not in this site.
        let outline = vec![node(
            "index.md",
            vec![node(
                "letters.md",
                vec![node(
                    "letters/private.md",
                    vec![node("letters/private/1943.md", vec![])],
                )],
            )],
        )];

        let letters = &build_site_nav_tree(&pages, &outline)[0].children[0];
        assert_eq!(letters.href, "letters.html");
        assert_eq!(
            letters.children.len(),
            1,
            "the withheld tier is not a nav entry"
        );
        assert_eq!(letters.children[0].href, "1943.html", "what was under it");
    }

    /// Hoisting runs out at the forest, which is the ordinary shape under
    /// explicit-only visibility: a vault's root document is private, so nothing
    /// above a published entry is published either.
    #[test]
    fn a_page_with_no_published_ancestor_becomes_a_forest_root() {
        let pages = vec![sourced(
            make_page("mon.html", "Monday", false, vec![], None),
            "daily/mon.md",
        )];
        let outline = vec![node(
            "root.md",
            vec![node("daily.md", vec![node("daily/mon.md", vec![])])],
        )];

        assert_eq!(hrefs(&build_site_nav_tree(&pages, &outline)), ["mon.html"]);
    }

    /// The invariant holds against an outline too: a page the archive's tree
    /// never reached is still in the nav, once.
    #[test]
    fn a_page_the_outline_does_not_reach_is_still_placed() {
        let pages = vec![
            sourced(
                make_page("index.html", "Home", true, vec![], None),
                "index.md",
            ),
            sourced(
                make_page("loose.html", "Loose", false, vec![], None),
                "loose.md",
            ),
        ];
        let outline = vec![node("index.md", vec![])];

        let found = hrefs(&build_site_nav_tree(&pages, &outline));
        assert_eq!(found, ["index.html", "loose.html"]);
    }
}
