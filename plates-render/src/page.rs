//! Page-shell helpers: navigation, breadcrumbs, SEO meta, feed/sitemap/robots
//! generation, and small HTML/XML escaping utilities.
//!
//! These are pure functions over the value types in [`crate::types`]. The page
//! *assembly* (full `<html>` document, theme/CSS/favicon) still lives in the
//! publish plugin and will move here in a later slice.

use crate::dates::{EPOCH_RFC3339, to_rfc822, to_rfc3339};
use crate::links::{absolutize_html, root_prefix};
use crate::types::{NavLink, PublishedPage, SiteNavNode, SiteNavigation};

/// The newest-first order a feed lists entries in.
///
/// Sorts on [`PublishedPage::published_date`] — the same chain the site's own
/// index groups and orders by — so a reader's list and the front page agree.
/// Ties fall back to the title so a set of entries sharing a day is at least
/// stable between builds.
fn feed_items(pages: &[PublishedPage]) -> Vec<&PublishedPage> {
    let mut items: Vec<&PublishedPage> = pages
        .iter()
        .filter(|p| !p.is_root && p.contents_links.is_empty() && !p.hide_from_feed)
        .collect();

    items.sort_by(|a, b| newest_first(a, b));
    items.truncate(50);
    items
}

/// Order two entries newest-first, with the undated last.
///
/// The one answer to "which of these comes first", shared with the grouped
/// index in [`crate::site`] so a site cannot list its entries in one order and
/// syndicate them in another.
///
/// Two things it has to get right, both of which a naive comparison gets wrong:
///
/// **The date is normalized before it is compared.** Sorting the raw string
/// looks equivalent, since ISO dates sort lexicographically — right up until a
/// date is not an ISO date, and a vault has ordinary ways to hand over one that
/// is not. `date_of_document: unknown` is the conventional marker for a
/// deliberately-undated record: a shoebox
/// of scans imported on one afternoon must not inherit that afternoon. The
/// chain is first-key-*present* wins, so the marker stops it here as it does in
/// a view — but `'u' > '2'`, so a descending raw-string sort put every undated
/// record *above* every dated one, at the head of the feed. Nothing validates a
/// `type: date` field, so a typo (`19430512`) or a human spelling (`May 1943`)
/// arrives the same way and sorts by its own first character.
///
/// **Undated is not a date.** The obvious repair — normalize, and let an
/// unreadable one fall back to [`EPOCH_RFC3339`] like the emitted element does
/// — is wrong for the archive this program is for. The epoch is not a floor,
/// it is 1970, so an undated scan would sort *above* every letter written
/// before it. A record with no readable date is therefore ordered as absent
/// rather than as a moment, and lands after everything that has one.
pub(crate) fn newest_first(a: &PublishedPage, b: &PublishedPage) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let key = |p: &PublishedPage| p.published_date().and_then(to_rfc3339);
    match (key(a), key(b)) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    // Ties fall back to the title so a set of entries sharing a day — or
    // sharing no day at all — is at least stable between builds.
    .then_with(|| a.title.cmp(&b.title))
}

/// Escape HTML special characters.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Convert a title to an anchor ID.
///
/// [`prov::link::slug`] is the one slug rule in the project, so a heading anchor
/// and the filename prov would mint for the same title agree. It drops
/// punctuation rather than turning it into a separator (`v1.0 Release` becomes
/// `v10-release`), and yields `"untitled"` for a title with nothing slug-able.
pub fn title_to_anchor(title: &str) -> String {
    prov::link::slug(title)
}

/// Render the full site navigation sidebar.
pub fn render_site_nav(nav: &SiteNavigation, root_prefix: &str) -> String {
    if nav.tree.is_empty() {
        return String::new();
    }

    fn render_nodes(nodes: &[SiteNavNode], prefix: &str) -> String {
        let mut html = String::from("<ul class=\"nav-list\">");
        for node in nodes {
            let mut classes = Vec::new();
            if node.is_current {
                classes.push("nav-current");
            }
            if node.is_ancestor_of_current {
                classes.push("nav-ancestor");
            }

            let class_attr = if classes.is_empty() {
                String::new()
            } else {
                format!(r#" class="{}""#, classes.join(" "))
            };

            let aria = if node.is_current {
                r#" aria-current="page""#
            } else {
                ""
            };

            html.push_str(&format!(
                r#"<li{class}><a href="{prefix}{href}"{aria}>{title}</a>"#,
                class = class_attr,
                prefix = prefix,
                href = html_escape(&node.href),
                aria = aria,
                title = html_escape(&node.title),
            ));

            if !node.children.is_empty() {
                html.push_str(&render_nodes(&node.children, prefix));
            }

            html.push_str("</li>");
        }
        html.push_str("</ul>");
        html
    }

    let nav_list = render_nodes(&nav.tree, root_prefix);

    format!(
        r#"<button class="nav-toggle" aria-label="Toggle navigation" aria-expanded="false">&#9776;</button>
<nav class="site-nav" aria-label="Site navigation">
{nav_list}
</nav>"#,
        nav_list = nav_list,
    )
}

/// Render full breadcrumb trail from root to current page.
pub fn render_full_breadcrumbs(breadcrumbs: &[NavLink], prefix: &str) -> String {
    if breadcrumbs.len() <= 1 {
        return String::new();
    }

    let items: Vec<String> = breadcrumbs
        .iter()
        .enumerate()
        .map(|(i, crumb)| {
            if i == breadcrumbs.len() - 1 {
                // Current page — no link
                format!(
                    r#"<span aria-current="page">{}</span>"#,
                    html_escape(&crumb.title)
                )
            } else {
                format!(
                    r#"<a href="{}{}">{}</a>"#,
                    prefix,
                    html_escape(&crumb.href),
                    html_escape(&crumb.title)
                )
            }
        })
        .collect();

    format!(
        r#"<nav class="breadcrumbs" aria-label="Breadcrumb">{}</nav>"#,
        items.join(r#" <span class="breadcrumb-sep">/</span> "#)
    )
}

/// Render breadcrumb navigation (parent link above the title).
pub fn render_breadcrumb(page: &PublishedPage, single_file: bool) -> String {
    let prefix = root_prefix(&page.dest_filename);
    if let Some(ref parent) = page.parent_link {
        let href = if single_file {
            format!("#{}", title_to_anchor(&parent.title))
        } else {
            format!("{}{}", prefix, parent.href)
        };
        format!(
            r#"<nav class="breadcrumb" aria-label="Breadcrumb"><a href="{}">{}</a></nav>"#,
            html_escape(&href),
            html_escape(&parent.title),
        )
    } else {
        String::new()
    }
}

/// Generate SEO meta tags for a page.
pub fn generate_seo_meta(page: &PublishedPage, site_title: &str, base_url: &str) -> String {
    let mut tags = Vec::new();

    // og:title
    tags.push(format!(
        r#"<meta property="og:title" content="{}">"#,
        html_escape(&page.title)
    ));

    // description + og:description
    if let Some(ref desc) = page.description {
        tags.push(format!(
            r#"<meta name="description" content="{}">"#,
            html_escape(desc)
        ));
        tags.push(format!(
            r#"<meta property="og:description" content="{}">"#,
            html_escape(desc)
        ));
    }

    // author
    if let Some(ref author) = page.author {
        tags.push(format!(
            r#"<meta name="author" content="{}">"#,
            html_escape(author)
        ));
    }

    // article:published_time — the date the entry is *of*, matching the order
    // the site's own index lists it in, and in the RFC 3339 the Open Graph
    // spec asks for rather than whatever the frontmatter happened to say.
    if let Some(published) = page.published_date().and_then(to_rfc3339) {
        tags.push(format!(
            r#"<meta property="article:published_time" content="{}">"#,
            html_escape(&published)
        ));
    }

    // article:modified_time
    if let Some(modified) = page.modified_date().and_then(to_rfc3339) {
        tags.push(format!(
            r#"<meta property="article:modified_time" content="{}">"#,
            html_escape(&modified)
        ));
    }

    // og:image — scan attachments for images, then fall back to first <img> in body
    let og_image = find_og_image(page);
    if let Some(img_url) = og_image {
        let full_url = if img_url.starts_with("http://") || img_url.starts_with("https://") {
            img_url
        } else if !base_url.is_empty() {
            format!(
                "{}/{}",
                base_url.trim_end_matches('/'),
                img_url.trim_start_matches('/')
            )
        } else {
            img_url
        };
        tags.push(format!(
            r#"<meta property="og:image" content="{}">"#,
            html_escape(&full_url)
        ));
    }

    // og:type
    let og_type = if page.is_root { "website" } else { "article" };
    tags.push(format!(
        r#"<meta property="og:type" content="{}">"#,
        og_type
    ));

    // og:site_name
    tags.push(format!(
        r#"<meta property="og:site_name" content="{}">"#,
        html_escape(site_title)
    ));

    // og:url + canonical
    if !base_url.is_empty() {
        let url = format!("{}/{}", base_url.trim_end_matches('/'), page.dest_filename);
        tags.push(format!(
            r#"<meta property="og:url" content="{}">"#,
            html_escape(&url)
        ));
        tags.push(format!(
            r#"<link rel="canonical" href="{}">"#,
            html_escape(&url)
        ));
    }

    tags.join("\n    ")
}

/// Find the best og:image for a page.
fn find_og_image(page: &PublishedPage) -> Option<String> {
    const IMAGE_EXTENSIONS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"];

    // Check attachments for images
    for s in &page.attachments {
        let lower = s.to_lowercase();
        if IMAGE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
            // An attachment is a link value like any other: unwrap `[alt](target)`
            // and resolve it against the page that carries it.
            let target = prov::Link::parse_path_only(s.trim()).target;
            return Some(
                prov::link::resolve(&page.source_path, &target)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }

    // Fall back to first <img src="..."> in rendered body
    if let Some(pos) = page.rendered_body.find("src=\"") {
        let after = &page.rendered_body[pos + 5..];
        if let Some(end) = after.find('"') {
            return Some(after[..end].to_string());
        }
    }

    None
}

/// Generate `<link>` tags for Atom and RSS feeds.
pub fn generate_feed_link_tags(root_prefix: &str) -> String {
    format!(
        r#"<link rel="alternate" type="application/atom+xml" title="Atom Feed" href="{}feed.xml">
    <link rel="alternate" type="application/rss+xml" title="RSS Feed" href="{}rss.xml">"#,
        root_prefix, root_prefix,
    )
}

/// Generate a sitemap.xml from published pages.
pub fn generate_sitemap(pages: &[PublishedPage], base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
"#,
    );

    for page in pages {
        let loc = format!("{}/{}", base, page.dest_filename);
        // W3C Datetime, which is what a sitemap's `lastmod` is specified as and
        // which RFC 3339 satisfies. A date the vault wrote in some other
        // spelling is left out rather than emitted for a crawler to reject.
        let lastmod = page
            .modified_date()
            .and_then(to_rfc3339)
            .unwrap_or_default();
        let priority = if page.is_root {
            "1.0"
        } else if !page.contents_links.is_empty() {
            "0.8"
        } else {
            "0.6"
        };

        xml.push_str("  <url>\n");
        xml.push_str(&format!("    <loc>{}</loc>\n", xml_escape(&loc)));
        if !lastmod.is_empty() {
            xml.push_str(&format!(
                "    <lastmod>{}</lastmod>\n",
                xml_escape(&lastmod)
            ));
        }
        xml.push_str(&format!("    <priority>{}</priority>\n", priority));
        xml.push_str("  </url>\n");
    }

    xml.push_str("</urlset>\n");
    xml
}

/// Generate robots.txt content.
pub fn generate_robots_txt(base_url: &str, is_public: bool) -> String {
    if is_public {
        format!(
            "User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n",
            base_url.trim_end_matches('/')
        )
    } else {
        "User-agent: *\nDisallow: /\n".to_string()
    }
}

/// Generate an Atom 1.0 feed.
pub fn generate_atom_feed(
    pages: &[PublishedPage],
    site_title: &str,
    base_url: &str,
    site_description: &str,
    site_author: &str,
) -> String {
    let base = base_url.trim_end_matches('/');

    let items = feed_items(pages);

    // Atom makes the feed's own `<updated>` mandatory, so this one falls back
    // rather than being omitted.
    let feed_updated = items
        .first()
        .and_then(|p| p.modified_date())
        .and_then(to_rfc3339)
        .unwrap_or_else(|| EPOCH_RFC3339.to_string());

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>{title}</title>
  <link href="{base}/" rel="alternate"/>
  <link href="{base}/feed.xml" rel="self"/>
  <id>{base}/</id>
  <updated>{updated}</updated>
"#,
        title = xml_escape(site_title),
        base = xml_escape(base),
        updated = xml_escape(&feed_updated),
    );

    if !site_author.is_empty() {
        xml.push_str(&format!(
            "  <author><name>{}</name></author>\n",
            xml_escape(site_author)
        ));
    }
    if !site_description.is_empty() {
        xml.push_str(&format!(
            "  <subtitle>{}</subtitle>\n",
            xml_escape(site_description)
        ));
    }

    for page in &items {
        let link = format!("{}/{}", base, page.dest_filename);
        let published = page.published_date().and_then(to_rfc3339);
        // Mandatory on every entry, like the feed's own above.
        let updated = page
            .modified_date()
            .and_then(to_rfc3339)
            .unwrap_or_else(|| EPOCH_RFC3339.to_string());
        let summary = strip_html_truncate(&page.rendered_body, 280);

        xml.push_str("  <entry>\n");
        xml.push_str(&format!("    <title>{}</title>\n", xml_escape(&page.title)));
        xml.push_str(&format!(
            "    <link href=\"{}\" rel=\"alternate\"/>\n",
            xml_escape(&link)
        ));
        xml.push_str(&format!("    <id>{}</id>\n", xml_escape(&link)));
        if let Some(published) = published {
            xml.push_str(&format!(
                "    <published>{}</published>\n",
                xml_escape(&published)
            ));
        }
        xml.push_str(&format!(
            "    <updated>{}</updated>\n",
            xml_escape(&updated)
        ));
        if !summary.is_empty() {
            xml.push_str(&format!(
                "    <summary>{}</summary>\n",
                xml_escape(&summary)
            ));
        }
        // The body leaves the site here, so its page-relative links have to be
        // resolved now — a reader has no way to reconstruct the base later.
        xml.push_str(&format!(
            "    <content type=\"html\"><![CDATA[{}]]></content>\n",
            absolutize_html(&page.rendered_body, &page.dest_filename, base)
        ));
        xml.push_str("  </entry>\n");
    }

    xml.push_str("</feed>\n");
    xml
}

/// Generate an RSS 2.0 feed.
pub fn generate_rss_feed(
    pages: &[PublishedPage],
    site_title: &str,
    base_url: &str,
    site_description: &str,
    _site_author: &str,
) -> String {
    let base = base_url.trim_end_matches('/');

    let items = feed_items(pages);

    // RSS 2.0 dates are RFC 822, which is a different grammar from Atom's —
    // hence the second spelling of the same instants.
    let last_build = items
        .first()
        .and_then(|p| p.modified_date())
        .and_then(to_rfc822)
        .unwrap_or_default();

    let desc = if site_description.is_empty() {
        site_title
    } else {
        site_description
    };

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
<channel>
  <title>{title}</title>
  <link>{base}/</link>
  <description>{description}</description>
  <atom:link href="{base}/rss.xml" rel="self" type="application/rss+xml"/>
"#,
        title = xml_escape(site_title),
        base = xml_escape(base),
        description = xml_escape(desc),
    );

    if !last_build.is_empty() {
        xml.push_str(&format!(
            "  <lastBuildDate>{}</lastBuildDate>\n",
            xml_escape(&last_build)
        ));
    }

    for page in &items {
        let link = format!("{}/{}", base, page.dest_filename);
        let pub_date = page.published_date().and_then(to_rfc822);

        xml.push_str("  <item>\n");
        xml.push_str(&format!("    <title>{}</title>\n", xml_escape(&page.title)));
        xml.push_str(&format!("    <link>{}</link>\n", xml_escape(&link)));
        xml.push_str(&format!(
            "    <guid isPermaLink=\"true\">{}</guid>\n",
            xml_escape(&link)
        ));
        if let Some(pub_date) = pub_date {
            xml.push_str(&format!(
                "    <pubDate>{}</pubDate>\n",
                xml_escape(&pub_date)
            ));
        }
        xml.push_str(&format!(
            "    <description><![CDATA[{}]]></description>\n",
            absolutize_html(&page.rendered_body, &page.dest_filename, base)
        ));
        xml.push_str("  </item>\n");
    }

    xml.push_str("</channel>\n</rss>\n");
    xml
}

/// Strip HTML tags and truncate to `max_len` characters.
fn strip_html_truncate(html: &str, max_len: usize) -> String {
    let mut text = String::new();
    let mut in_tag = false;

    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag {
            text.push(ch);
            if text.len() >= max_len {
                break;
            }
        }
    }

    text.trim().to_string()
}

/// Escape characters for XML content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NavLink, PageLayout};
    use std::path::PathBuf;

    fn make_page(dest: &str, title: &str, is_root: bool) -> PublishedPage {
        PublishedPage {
            source_path: PathBuf::from(format!("/workspace/{}", dest.replace(".html", ".md"))),
            dest_filename: dest.to_string(),
            title: title.to_string(),
            rendered_body: "<p>Hello world</p>".to_string(),
            markdown_body: "Hello world".to_string(),
            contents_links: vec![],
            parent_link: None,
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
    fn test_seo_meta_basic() {
        let mut page = make_page("about.html", "About", false);
        page.description = Some("A test page".into());
        page.author = Some("Alice".into());
        let meta = generate_seo_meta(&page, "My Site", "https://example.com");

        assert!(meta.contains(r#"og:title" content="About""#));
        assert!(meta.contains(r#"name="description" content="A test page""#));
        assert!(meta.contains(r#"og:description" content="A test page""#));
        assert!(meta.contains(r#"name="author" content="Alice""#));
        assert!(meta.contains(r#"og:type" content="article""#));
        assert!(meta.contains(r#"og:site_name" content="My Site""#));
        assert!(meta.contains(r#"og:url" content="https://example.com/about.html""#));
        assert!(meta.contains(r#"canonical" href="https://example.com/about.html""#));
    }

    #[test]
    fn test_seo_meta_root_is_website_type() {
        let page = make_page("index.html", "Home", true);
        let meta = generate_seo_meta(&page, "My Site", "https://example.com");
        assert!(meta.contains(r#"og:type" content="website""#));
    }

    #[test]
    fn test_seo_meta_no_base_url() {
        let page = make_page("page.html", "Page", false);
        let meta = generate_seo_meta(&page, "Site", "");
        assert!(!meta.contains("canonical"));
        assert!(!meta.contains("og:url"));
    }

    #[test]
    fn test_sitemap_structure() {
        let root = make_page("index.html", "Home", true);
        let mut child = make_page("child.html", "Child", false);
        child.contents_links = vec![NavLink {
            href: "leaf.html".into(),
            title: "Leaf".into(),
        }];
        let leaf = make_page("leaf.html", "Leaf", false);

        let sitemap = generate_sitemap(&[root, child, leaf], "https://example.com");

        assert!(sitemap.contains("<loc>https://example.com/index.html</loc>"));
        assert!(sitemap.contains("<priority>1.0</priority>")); // root
        assert!(sitemap.contains("<priority>0.8</priority>")); // child with contents
        assert!(sitemap.contains("<priority>0.6</priority>")); // leaf
    }

    #[test]
    fn test_robots_txt_public() {
        let robots = generate_robots_txt("https://example.com", true);
        assert!(robots.contains("Allow: /"));
        assert!(robots.contains("Sitemap: https://example.com/sitemap.xml"));
    }

    #[test]
    fn test_robots_txt_private() {
        let robots = generate_robots_txt("https://example.com", false);
        assert!(robots.contains("Disallow: /"));
        assert!(!robots.contains("Sitemap"));
    }

    #[test]
    fn test_atom_feed_excludes_root_and_index_pages() {
        let root = make_page("index.html", "Home", true);
        let mut index_child = make_page("section.html", "Section", false);
        index_child.contents_links = vec![NavLink {
            href: "leaf.html".into(),
            title: "Leaf".into(),
        }];
        let leaf = make_page("leaf.html", "Leaf", false);

        let atom = generate_atom_feed(
            &[root, index_child, leaf],
            "Site",
            "https://example.com",
            "",
            "",
        );

        // Only the leaf should appear as an entry
        assert_eq!(atom.matches("<entry>").count(), 1);
        assert!(atom.contains("<title>Leaf</title>"));
        assert!(!atom.contains("<title>Home</title>"));
        assert!(!atom.contains("<title>Section</title>"));
    }

    #[test]
    fn test_atom_feed_hide_from_feed() {
        let root = make_page("index.html", "Home", true);
        let mut hidden = make_page("hidden.html", "Hidden", false);
        hidden.hide_from_feed = true;
        let visible = make_page("visible.html", "Visible", false);

        let atom = generate_atom_feed(
            &[root, hidden, visible],
            "Site",
            "https://example.com",
            "",
            "",
        );

        assert_eq!(atom.matches("<entry>").count(), 1);
        assert!(atom.contains("<title>Visible</title>"));
        assert!(!atom.contains("<title>Hidden</title>"));
    }

    #[test]
    fn test_rss_feed_structure() {
        let root = make_page("index.html", "Home", true);
        let mut leaf = make_page("post.html", "Post", false);
        leaf.created = Some("2024-01-15".into());

        let rss = generate_rss_feed(
            &[root, leaf],
            "My Blog",
            "https://example.com",
            "A blog",
            "Author",
        );

        assert!(rss.contains("<title>My Blog</title>"));
        assert!(rss.contains("<description>A blog</description>"));
        assert!(rss.contains("<title>Post</title>"));
        assert!(rss.contains("<guid isPermaLink=\"true\">https://example.com/post.html</guid>"));
        // RFC 822, not the `2024-01-15` the vault wrote: RSS specifies the
        // grammar, and readers hold it to that.
        assert!(
            rss.contains("<pubDate>Mon, 15 Jan 2024 00:00:00 +0000</pubDate>"),
            "got {rss}"
        );
    }

    /// The date chain the site's index groups by, answered the same way by the
    /// feeds. A scanned letter's `date_of_document` is the year it was written
    /// and its `created` is the day it was scanned; the feed used to order by
    /// the latter while the front page listed by the former.
    #[test]
    fn feeds_order_by_the_same_date_chain_the_index_does() {
        let mut letter = make_page("letter.html", "Letter", false);
        letter.date_of_document = Some("1944-06-06".into());
        letter.created = Some("2026-08-16".into());

        let mut note = make_page("note.html", "Note", false);
        note.created = Some("2026-01-02".into());

        let pages = [letter, note];
        let atom = generate_atom_feed(&pages, "Site", "https://ex.com", "", "");

        // The letter is *of* 1944, so it sorts below the 2026 note and is
        // published as its own date rather than its scanning date.
        assert!(
            atom.find("<title>Note</title>") < atom.find("<title>Letter</title>"),
            "got {atom}"
        );
        assert!(atom.contains("<published>1944-06-06T00:00:00Z</published>"));
        assert!(!atom.contains("1944-06-06</published>\n    <published>"));

        let rss = generate_rss_feed(&pages, "Site", "https://ex.com", "", "");
        assert!(rss.contains("<pubDate>Tue, 06 Jun 1944 00:00:00 +0000</pubDate>"));
    }

    /// `date_of_document: unknown` is the marker for a record that is
    /// undated on purpose — a shoebox of scans must not inherit the afternoon
    /// it was imported. The chain stops at the marker correctly, but a
    /// descending sort of the *raw* string put `"unknown"` above every ISO
    /// date, so the undated scans headed the feed wearing a 1970 timestamp.
    #[test]
    fn a_deliberately_undated_record_sorts_to_the_bottom() {
        let mut undated = make_page("undated.html", "Undated", false);
        undated.date_of_document = Some("unknown".into());
        // Pre-epoch on purpose. Normalizing an unreadable date to
        // `EPOCH_RFC3339` and sorting on that would place the undated scan
        // *above* this letter, because the epoch is not a floor — it is 1970,
        // and an archive of a family's papers is mostly older than that.
        let mut old = make_page("old.html", "Old", false);
        old.date_of_document = Some("1943-05-12".into());
        let mut new = make_page("new.html", "New", false);
        new.date_of_document = Some("2026-08-16".into());

        let pages = [undated, old, new];
        let atom = generate_atom_feed(&pages, "Site", "https://ex.com", "", "");

        let at = |t: &str| atom.find(&format!("<title>{t}</title>")).unwrap();
        assert!(
            at("New") < at("Old") && at("Old") < at("Undated"),
            "newest first, and the undated record last: {atom}"
        );
        // Where it sorts and what it says agree: 1970 sorts like 1970.
        assert!(!atom.contains("<published>unknown</published>"));
    }

    /// Nothing validates a `type: date` field, so a typo and a human spelling
    /// reach the feed the same way the marker does — and used to sort by their
    /// own first character, above or below the ISO dates by accident.
    #[test]
    fn an_unparseable_date_does_not_sort_by_its_spelling() {
        let mut wordy = make_page("wordy.html", "Wordy", false);
        wordy.date_of_document = Some("May 1943".into());
        let mut typo = make_page("typo.html", "Typo", false);
        typo.date_of_document = Some("19430512".into());
        let mut dated = make_page("dated.html", "Dated", false);
        dated.date_of_document = Some("2026-08-16".into());

        let pages = [wordy, typo, dated];
        let atom = generate_atom_feed(&pages, "Site", "https://ex.com", "", "");

        let at = |t: &str| atom.find(&format!("<title>{t}</title>")).unwrap();
        assert!(
            at("Dated") < at("Typo") && at("Dated") < at("Wordy"),
            "the only readable date leads: {atom}"
        );
    }

    /// Atom makes `<updated>` mandatory on the feed and on every entry, so an
    /// entry the vault gave no date at all still has to carry one.
    #[test]
    fn an_undated_entry_still_carries_a_valid_atom_updated() {
        let page = make_page("post.html", "Post", false);
        let atom = generate_atom_feed(&[page], "Site", "https://ex.com", "", "");

        assert!(atom.contains(&format!("<updated>{EPOCH_RFC3339}</updated>")));
        // …but not a `<published>` it would have had to invent.
        assert!(!atom.contains("<published>"));
    }

    /// A date the vault wrote in a spelling no feed grammar recognizes is left
    /// out rather than passed through for a validator to choke on.
    #[test]
    fn an_unreadable_date_is_omitted_not_forwarded() {
        let mut page = make_page("post.html", "Post", false);
        page.created = Some("sometime last summer".into());

        let atom = generate_atom_feed(&[page.clone()], "Site", "https://ex.com", "", "");
        assert!(!atom.contains("sometime last summer"));
        assert!(atom.contains(&format!("<updated>{EPOCH_RFC3339}</updated>")));

        let rss = generate_rss_feed(&[page.clone()], "Site", "https://ex.com", "", "");
        assert!(!rss.contains("sometime last summer"));
        assert!(!rss.contains("<pubDate>"));

        let sitemap = generate_sitemap(&[page.clone()], "https://ex.com");
        assert!(!sitemap.contains("<lastmod>"));

        let meta = generate_seo_meta(&page, "Site", "https://ex.com");
        assert!(!meta.contains("article:published_time"));
    }

    /// A sitemap's `lastmod` is a W3C Datetime, which RFC 3339 satisfies and a
    /// bare vault date does not reliably.
    #[test]
    fn sitemap_lastmod_is_a_w3c_datetime() {
        let mut page = make_page("post.html", "Post", false);
        page.updated = Some("2026-08-16".into());
        let sitemap = generate_sitemap(&[page], "https://ex.com");
        assert!(sitemap.contains("<lastmod>2026-08-16T00:00:00Z</lastmod>"));
    }

    /// Open Graph asks for RFC 3339 here too, and the published time follows
    /// the same chain the feeds do.
    #[test]
    fn seo_article_times_are_rfc3339_from_the_shared_chain() {
        let mut page = make_page("post.html", "Post", false);
        page.date_of_document = Some("2026-01-15".into());
        page.created = Some("2026-08-16".into());
        page.updated = Some("2026-08-20".into());

        let meta = generate_seo_meta(&page, "Site", "https://ex.com");
        assert!(meta.contains(r#"article:published_time" content="2026-01-15T00:00:00Z""#));
        assert!(meta.contains(r#"article:modified_time" content="2026-08-20T00:00:00Z""#));
    }

    #[test]
    fn feed_content_carries_absolute_links_and_images() {
        // A feed entry is read away from the site — in a reader, or in an email
        // built from the feed — where a page-relative href resolves to nothing.
        let root = make_page("index.html", "Home", true);
        let mut leaf = make_page("notes/entry.html", "Entry", false);
        leaf.rendered_body =
            r#"<p><a href="../other.html">o</a><img src="../_attachments/a.jpg"></p>"#.to_string();

        let pages = [root, leaf];
        let atom = generate_atom_feed(&pages, "Site", "https://ex.com", "", "");
        assert!(atom.contains(r#"href="https://ex.com/other.html""#));
        assert!(atom.contains(r#"src="https://ex.com/_attachments/a.jpg""#));
        assert!(!atom.contains(r#"href="../other.html""#));

        let rss = generate_rss_feed(&pages, "Site", "https://ex.com", "", "");
        assert!(rss.contains(r#"href="https://ex.com/other.html""#));
        assert!(rss.contains(r#"src="https://ex.com/_attachments/a.jpg""#));
    }

    #[test]
    fn test_feed_links() {
        let links = generate_feed_link_tags("");
        assert!(links.contains("application/atom+xml"));
        assert!(links.contains("feed.xml"));
        assert!(links.contains("application/rss+xml"));
        assert!(links.contains("rss.xml"));
    }

    #[test]
    fn test_strip_html_truncate() {
        let html = "<p>Hello <strong>world</strong>, this is a test.</p>";
        let result = strip_html_truncate(html, 11);
        assert_eq!(result, "Hello world");
    }
}
