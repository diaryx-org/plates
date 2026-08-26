//! Site navigation tree construction.
//!
//! [`build_site_nav_tree`] builds the whole-site tree from every page's
//! `contents_links`/`parent_link`; [`nav_for_page`] specializes that tree for a
//! single page (marking current/ancestor nodes and computing breadcrumbs).
//! Both are pure functions over [`PublishedPage`].
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

use crate::types::{NavLink, PublishedPage, SiteNavNode, SiteNavigation};

/// How deep the nav renders before it stops descending.
///
/// Not a safety bound — a page is placed at most once (tracked in `placed`), so
/// a `contents`/`part_of` cycle cannot loop this walk. It is a readability bound
/// on a sidebar, raised from the 3 the single-rooted tree used because a forest
/// hangs its roots one level lower.
const MAX_NAV_DEPTH: usize = 6;

/// Build a site navigation tree from all published pages.
///
/// Containment edges that survived audience filtering still nest; every page the
/// walk from the index cannot reach becomes its own root, so nothing in the
/// render set is missing from the nav. Filters out `hide_from_nav` pages and
/// sorts siblings by `nav_order` (if present), then by their position in the
/// parent's `contents_links`.
///
/// With no page marked `is_root` the forest is returned bare — a caller that
/// synthesizes a front page (`site::synthesize_index`) hands it back here as a
/// page with `is_root` set, and the forest nests under it on the next call.
pub fn build_site_nav_tree(pages: &[PublishedPage]) -> Vec<SiteNavNode> {
    let page_map: HashMap<&str, &PublishedPage> = pages
        .iter()
        .map(|p| (p.dest_filename.as_str(), p))
        .collect();

    // Every page placed so far, so no page is listed twice and a containment
    // cycle terminates.
    let mut placed: HashSet<&str> = HashSet::new();

    let root = pages.iter().find(|p| p.is_root);
    if let Some(r) = root {
        placed.insert(r.dest_filename.as_str());
    }

    // 1. Everything reachable from the index through surviving containment.
    let mut top: Vec<SiteNavNode> = match root {
        Some(r) => build_children(r, &page_map, &mut placed, 0),
        None => Vec::new(),
    };

    // 2. Forest roots: a page whose `part_of` target is not in this render set
    //    (audience filtering dropped it, so `parent_link` is already `None`) is
    //    the root of its own subtree. Source order, `nav_order` overriding.
    let mut orphans: Vec<(i32, SiteNavNode)> = Vec::new();
    for (idx, page) in pages.iter().enumerate() {
        if page.hide_from_nav || placed.contains(page.dest_filename.as_str()) {
            continue;
        }
        if !is_forest_root(page, &page_map) {
            continue;
        }
        placed.insert(page.dest_filename.as_str());
        let children = build_children(page, &page_map, &mut placed, 1);
        orphans.push((
            page.nav_order.unwrap_or(idx as i32),
            node_for(page, None, children),
        ));
    }
    orphans.sort_by_key(|(key, _)| *key);
    top.extend(orphans.into_iter().map(|(_, node)| node));

    // 3. Anything still unplaced — a page whose parent *is* in the set but never
    //    listed it in `contents` (a one-sided link), or a member of a cycle the
    //    walk entered elsewhere. Listing it flat is worse than nesting it and
    //    better than dropping it: a published page missing from its own site's
    //    nav is the failure this rewrite exists to end.
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

/// Whether a page starts its own subtree: it names no parent, or names one that
/// is not in this render set.
fn is_forest_root(page: &PublishedPage, page_map: &HashMap<&str, &PublishedPage>) -> bool {
    match &page.parent_link {
        Some(link) => !page_map.contains_key(link.href.as_str()),
        None => true,
    }
}

/// A nav node for `page`. `link` supplies the display title when the page was
/// reached through a `contents` link that carried its own label; otherwise the
/// page's `nav_title`/`title` is used.
fn node_for(
    page: &PublishedPage,
    link: Option<&NavLink>,
    children: Vec<SiteNavNode>,
) -> SiteNavNode {
    let title = page
        .nav_title
        .clone()
        .or_else(|| link.map(|l| l.title.clone()))
        .unwrap_or_else(|| page.title.clone());
    SiteNavNode {
        title,
        href: page.dest_filename.clone(),
        is_current: false,
        is_ancestor_of_current: false,
        children,
    }
}

/// Build `page`'s children from its `contents_links`, recursively.
///
/// A link whose target is not in the render set is **dropped**, not rendered
/// from the link text: it was excluded by audience filtering, and emitting a nav
/// entry for it would both leak the excluded page's title and 404 on click.
fn build_children<'p>(
    page: &'p PublishedPage,
    page_map: &HashMap<&'p str, &'p PublishedPage>,
    placed: &mut HashSet<&'p str>,
    depth: usize,
) -> Vec<SiteNavNode> {
    if depth >= MAX_NAV_DEPTH || page.contents_links.is_empty() {
        return Vec::new();
    }

    let mut children: Vec<(i32, SiteNavNode)> = Vec::new();

    for (idx, link) in page.contents_links.iter().enumerate() {
        let Some(child) = page_map.get(link.href.as_str()) else {
            continue;
        };
        if child.hide_from_nav || placed.contains(child.dest_filename.as_str()) {
            continue;
        }
        placed.insert(child.dest_filename.as_str());

        let sub_children = build_children(child, page_map, placed, depth + 1);
        let sort_key = child.nav_order.unwrap_or(idx as i32);
        children.push((sort_key, node_for(child, Some(link), sub_children)));
    }

    // Sort by nav_order (encoded in sort_key), stable for equal keys.
    children.sort_by_key(|(key, _)| *key);
    children.into_iter().map(|(_, node)| node).collect()
}

/// Build navigation context (tree with current-page marking + breadcrumbs) for a specific page.
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

    // Build breadcrumbs by walking parent_link chain
    let page_map: HashMap<&str, &PublishedPage> = pages
        .iter()
        .map(|p| (p.dest_filename.as_str(), p))
        .collect();

    let mut breadcrumbs = Vec::new();
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

        let tree = build_site_nav_tree(&pages);
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

        let tree = build_site_nav_tree(&pages);
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

        let tree = build_site_nav_tree(&pages);
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

        let tree = build_site_nav_tree(&pages);
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

        let tree = build_site_nav_tree(&pages);
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

        let tree = build_site_nav_tree(&pages);
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

        let tree = build_site_nav_tree(&pages);
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

        let found = hrefs(&build_site_nav_tree(&pages));
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

        let found = hrefs(&build_site_nav_tree(&pages));
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

        let found = hrefs(&build_site_nav_tree(&pages));
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
        let tree = build_site_nav_tree(&pages);
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

        let tree = build_site_nav_tree(&pages);
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
}
