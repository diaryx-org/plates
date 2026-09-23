//! The library slots: a page as a place in a library rather than a row in a
//! sidebar.
//!
//! A site's containment already says which pages are books (they contain
//! chapters), which are chapters, and which page is the front door. These
//! slots draw that for a shell that wants to show it — a band at the head of
//! a front page or a book, a shelf of what it holds, and a book's own contents
//! beside a chapter being read. The built-in shell uses none of them, so a
//! site that never asks is published byte for byte as it was.
//!
//! Colour is a **name**, from each page's frontmatter `color:`, written as a
//! `tone-<name>` class. Nothing here knows what `green` looks like; the
//! stylesheet does. Two rules decide whose name a thing wears, and they are the
//! Diaryx library's:
//!
//! 1. A page that holds pages (a book, or a chapter with parts) is a *place*
//!    and wears its own colour.
//! 2. A page that can only be read wears the colour of the place it is in, so
//!    a reader never follows a page of one colour into a room of another.

use std::collections::HashMap;

use crate::page::html_escape;
use crate::types::{PublishedPage, SiteNavNode, SiteNavigation};

/// The library theme's shell template: a bar, the band, the text, the shelf,
/// and the two panels — a book's contents on the left and an empty
/// `<aside id="margin">` on the right for a reader's annotation layer to fill.
/// Pass it as [`SiteOptions::template`](crate::site::SiteOptions::template).
pub const LIBRARY_SHELL: &str = include_str!("library_shell.html");

/// The library theme's own rules, all scoped under `.library`. They sit on top
/// of the base stylesheet rather than replacing it; see [`library_stylesheet`].
pub const LIBRARY_CSS: &str = include_str!("library.css");

/// The whole stylesheet a site in the library shell wants: the base sheet, so
/// every body renders as it always has, then the library's rules. Pass it as
/// [`SiteStyle::custom_css`](crate::SiteStyle::custom_css).
pub fn library_stylesheet() -> String {
    format!(
        "{}\n/* ── Library theme ── */\n{}",
        crate::html::base_css(),
        LIBRARY_CSS
    )
}

/// What a page is, for a shell that lays the three out differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// The site's front page.
    Front,
    /// A page that holds pages.
    Book,
    /// A page that is read.
    Page,
}

impl PageKind {
    /// The word written into the `page_kind` slot.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Book => "book",
            Self::Page => "page",
        }
    }
}

/// The library slots for one page, already rendered.
#[derive(Debug, Clone, Default)]
pub struct LibrarySlots {
    /// `front`, `book` or `page`.
    pub page_kind: String,
    /// What the library is called: its authored front page's title — the
    /// face a reader was shown at the door — or the site's name when the
    /// front page is generated.
    pub library_title: String,
    /// The colour name this page's room wears, or empty.
    pub page_color: String,
    /// The band: cover, title, description, counts and the way in — or, on a
    /// page that is read, the book it is in and its title.
    pub page_head: String,
    /// What this page holds, as covers and sheets. Empty on a page that holds
    /// nothing.
    pub shelf: String,
    /// The book this page is in, as a contents list with a way back to the
    /// front page. Empty outside a book.
    pub book_nav: String,
    /// The body without the leading `<h1>` that repeats the page's title,
    /// since `page_head` writes the title itself.
    pub content_below_title: String,
}

/// Everything about the site [`library_slots`] reads besides the page.
pub struct LibraryContext<'a> {
    /// This page's nav tree, current page marked.
    pub nav: &'a SiteNavigation,
    /// Every page in the render, by destination.
    pub pages: &'a HashMap<String, &'a PublishedPage>,
    /// What the library is called, for its bar and the way back to its front
    /// page — see [`LibrarySlots::library_title`].
    pub library_title: &'a str,
    /// `../` per level of depth.
    pub root_prefix: &'a str,
    /// The page is a generated front page whose body only lists what the shelf
    /// already shows.
    pub listing_body: bool,
}

/// The front page's destination, which is where the way back leads.
const FRONT_PAGE_DEST: &str = "index.html";

/// Render the library slots for `page`.
pub fn library_slots(page: &PublishedPage, cx: &LibraryContext<'_>) -> LibrarySlots {
    let place = Place::of(page, cx.nav);
    let kind = place.kind(page);
    let lookup = |node: &SiteNavNode| cx.pages.get(&node.href).copied();

    // The colour of the room: the page's own for a place, else the nearest
    // enclosing place's, else the page's own.
    let color = match kind {
        PageKind::Front | PageKind::Book => page.color.clone(),
        PageKind::Page => place
            .enclosing()
            .and_then(lookup)
            .and_then(|p| p.color.clone())
            .or_else(|| page.color.clone()),
    };

    let children: &[SiteNavNode] = place.current.map(|n| n.children.as_slice()).unwrap_or(&[]);

    LibrarySlots {
        page_kind: kind.as_str().to_string(),
        library_title: cx.library_title.to_string(),
        page_head: page_head(page, kind, &place, children, cx),
        shelf: match kind {
            PageKind::Front => front_shelf(children, page, cx),
            PageKind::Book => book_shelf(children, page, cx),
            PageKind::Page => String::new(),
        },
        book_nav: place
            .book()
            .map(|book| book_nav(book, cx))
            .unwrap_or_default(),
        content_below_title: if cx.listing_body {
            String::new()
        } else {
            without_title(page)
        },
        page_color: color.unwrap_or_default(),
    }
}

/// Where a page sits in the tree: the trail from the top of the library down
/// to the page.
struct Place<'n> {
    /// From the top-level node down to the current page, inclusive. Empty on
    /// the front page and on a page the tree does not hold.
    trail: Vec<&'n SiteNavNode>,
    /// The current page's node, when the tree holds it.
    current: Option<&'n SiteNavNode>,
}

impl<'n> Place<'n> {
    fn of(page: &PublishedPage, nav: &'n SiteNavigation) -> Self {
        let (front, top): (Option<&SiteNavNode>, &[SiteNavNode]) = match nav.tree.as_slice() {
            [root] if root.href == FRONT_PAGE_DEST => (Some(root), &root.children),
            forest => (None, forest),
        };
        if page.is_root {
            return Self {
                trail: Vec::new(),
                current: front,
            };
        }
        let mut trail = Vec::new();
        let mut level = top;
        while let Some(node) = level
            .iter()
            .find(|n| n.is_current || n.is_ancestor_of_current)
        {
            trail.push(node);
            if node.is_current {
                break;
            }
            level = &node.children;
        }
        let current = trail.last().copied().filter(|n| n.is_current);
        Self { trail, current }
    }

    fn kind(&self, page: &PublishedPage) -> PageKind {
        if page.is_root {
            PageKind::Front
        } else if self.current.is_some_and(|n| !n.children.is_empty()) {
            PageKind::Book
        } else {
            PageKind::Page
        }
    }

    /// The top-level place this page is in — itself when it is one.
    fn book(&self) -> Option<&'n SiteNavNode> {
        self.trail
            .first()
            .copied()
            .filter(|n| !n.children.is_empty())
    }

    /// The nearest place that holds this page.
    fn enclosing(&self) -> Option<&'n SiteNavNode> {
        let above = self.trail.len().saturating_sub(1);
        self.trail[..above]
            .iter()
            .rev()
            .copied()
            .find(|n| !n.children.is_empty())
    }

    /// This page's number among its book's chapters, when it is one.
    fn chapter_number(&self) -> Option<usize> {
        let [book, chapter, ..] = self.trail.as_slice() else {
            return None;
        };
        book.children
            .iter()
            .position(|n| n.href == chapter.href)
            .map(|i| i + 1)
    }
}

fn tone_class(color: Option<&str>) -> String {
    color.map(|c| format!(" tone-{c}")).unwrap_or_default()
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "3 chapters", "2 books" — what a place holds, in the words a library uses.
fn holds(children: &[SiteNavNode], front: bool) -> String {
    if front {
        let books = children.iter().filter(|n| !n.children.is_empty()).count();
        if books > 0 {
            count(books, "book", "books")
        } else {
            count(children.len(), "page", "pages")
        }
    } else {
        count(children.len(), "chapter", "chapters")
    }
}

fn page_head(
    page: &PublishedPage,
    kind: PageKind,
    place: &Place<'_>,
    children: &[SiteNavNode],
    cx: &LibraryContext<'_>,
) -> String {
    let title = html_escape(&page.title);
    let description = page
        .description
        .as_deref()
        .filter(|d| !d.trim().is_empty())
        .map(|d| format!("\n    <p class=\"page-description\">{}</p>", html_escape(d)))
        .unwrap_or_default();

    if kind == PageKind::Page {
        let eyebrow = place
            .book()
            .filter(|book| !book.is_current)
            .map(|book| {
                let number = place
                    .chapter_number()
                    .map(|n| format!(" <span class=\"page-chapter\">Chapter {n}</span>"))
                    .unwrap_or_default();
                format!(
                    "\n    <p class=\"page-eyebrow\"><a href=\"{}{}\">{}</a>{number}</p>",
                    cx.root_prefix,
                    html_escape(&book.href),
                    html_escape(&book.title),
                )
            })
            .unwrap_or_default();
        return format!(
            "<header class=\"page-head page-head-reading\">{eyebrow}\n    <h1 class=\"page-title\">{title}</h1>{description}\n</header>"
        );
    }

    let front = kind == PageKind::Front;
    let held = (!children.is_empty()).then(|| holds(children, front));
    let mut meta = Vec::new();
    if let Some(author) = page.author.as_deref().filter(|a| !a.trim().is_empty()) {
        let verb = if front { "Shared by" } else { "By" };
        meta.push(format!(
            "<span class=\"page-author\">{verb} <b>{}</b></span>",
            html_escape(author)
        ));
    }
    if let Some(held) = &held {
        meta.push(format!("<span class=\"page-holds\">{held}</span>"));
    }
    let meta = if meta.is_empty() {
        String::new()
    } else {
        format!("\n    <p class=\"page-meta\">{}</p>", meta.join(""))
    };
    let start = page
        .start_with
        .as_ref()
        .map(|link| {
            format!(
                "\n    <p class=\"page-actions\"><a class=\"start-with\" href=\"{}{}\">Start with {}</a></p>",
                cx.root_prefix,
                html_escape(&link.href),
                html_escape(&link.title),
            )
        })
        .or_else(|| {
            // A book with no `start_with:` begins at its first chapter.
            (!front)
                .then(|| children.first())
                .flatten()
                .map(|first| {
                    format!(
                        "\n    <p class=\"page-actions\"><a class=\"start-with\" href=\"{}{}\">Start reading</a></p>",
                        cx.root_prefix,
                        html_escape(&first.href),
                    )
                })
        })
        .unwrap_or_default();
    let cover_count = held
        .map(|h| format!("<span class=\"cover-count\">{h}</span>"))
        .unwrap_or_default();

    format!(
        "<header class=\"page-head page-head-{kind}\">\n  <div class=\"cover{tone}\" aria-hidden=\"true\"><span class=\"cover-title\">{title}</span>{cover_count}</div>\n  <div class=\"page-head-text\">\n    <h1 class=\"page-title\">{title}</h1>{description}{meta}{start}\n  </div>\n</header>",
        kind = kind.as_str(),
        tone = tone_class(page.color.as_deref()),
    )
}

/// The front page's shelf: books as covers, then loose pages as sheets.
fn front_shelf(children: &[SiteNavNode], page: &PublishedPage, cx: &LibraryContext<'_>) -> String {
    let (books, loose): (Vec<&SiteNavNode>, Vec<&SiteNavNode>) =
        children.iter().partition(|n| !n.children.is_empty());
    let mut out = String::new();
    if !books.is_empty() {
        out.push_str(&shelf_section(
            "books",
            "Books",
            books.iter().map(|n| cover(n, cx)).collect(),
        ));
    }
    if !loose.is_empty() {
        let tone = page.color.as_deref();
        out.push_str(&shelf_section(
            "pages",
            "Pages",
            loose.iter().map(|n| sheet(n, None, tone, cx)).collect(),
        ));
    }
    out
}

/// A book's shelf: its chapters as sheets, numbered, on the book's own paper —
/// except a chapter with parts, which is a place and wears its own.
fn book_shelf(children: &[SiteNavNode], page: &PublishedPage, cx: &LibraryContext<'_>) -> String {
    let tone = page.color.as_deref();
    let items = children
        .iter()
        .enumerate()
        .map(|(i, n)| sheet(n, Some(i + 1), tone, cx))
        .collect();
    shelf_section("contents", "Contents", items)
}

fn shelf_section(id: &str, heading: &str, items: Vec<String>) -> String {
    format!(
        "<section class=\"shelf shelf-{id}\" aria-labelledby=\"shelf-{id}\">\n  <h2 class=\"shelf-heading\" id=\"shelf-{id}\">{heading} <span class=\"shelf-count\">{n}</span></h2>\n  <ul class=\"shelf-grid\">\n{items}\n  </ul>\n</section>\n",
        n = items.len(),
        items = items.join("\n"),
    )
}

fn cover(node: &SiteNavNode, cx: &LibraryContext<'_>) -> String {
    let page = cx.pages.get(&node.href).copied();
    format!(
        "    <li><a class=\"cover{tone}\" href=\"{prefix}{href}\"><span class=\"cover-title\">{title}</span><span class=\"cover-count\">{held}</span></a><span class=\"cover-label\" aria-hidden=\"true\">{title}</span></li>",
        tone = tone_class(page.and_then(|p| p.color.as_deref())),
        prefix = cx.root_prefix,
        href = html_escape(&node.href),
        title = html_escape(&node.title),
        held = holds(&node.children, false),
    )
}

/// A page on a shelf. `tone` is the colour of the place it sits in, which a
/// page that is only read wears; a page with parts wears its own.
fn sheet(
    node: &SiteNavNode,
    number: Option<usize>,
    tone: Option<&str>,
    cx: &LibraryContext<'_>,
) -> String {
    let page = cx.pages.get(&node.href).copied();
    let room = !node.children.is_empty();
    let tone = if room {
        page.and_then(|p| p.color.as_deref())
    } else {
        tone
    };
    let number = match (number, room) {
        (Some(n), true) => format!(
            "<span class=\"sheet-number\">{n} · {}</span>",
            count(node.children.len(), "part", "parts")
        ),
        (Some(n), false) => format!("<span class=\"sheet-number\">{n}</span>"),
        (None, _) => String::new(),
    };
    let description = page
        .and_then(|p| p.description.as_deref())
        .filter(|d| !d.trim().is_empty())
        .map(|d| {
            format!(
                "<span class=\"sheet-description\">{}</span>",
                html_escape(d)
            )
        })
        .unwrap_or_default();
    format!(
        "    <li><a class=\"sheet{room}{tone}\" href=\"{prefix}{href}\">{number}<span class=\"sheet-title\">{title}</span>{description}</a></li>",
        room = if room { " sheet-room" } else { "" },
        tone = tone_class(tone),
        prefix = cx.root_prefix,
        href = html_escape(&node.href),
        title = html_escape(&node.title),
    )
}

/// The book's contents beside a page being read: a way back to the front
/// page, the book, and its chapters with the current one marked.
fn book_nav(book: &SiteNavNode, cx: &LibraryContext<'_>) -> String {
    fn items(nodes: &[SiteNavNode], numbered: bool, prefix: &str) -> String {
        let mut out = String::new();
        for (i, node) in nodes.iter().enumerate() {
            let mut classes = Vec::new();
            if node.is_current {
                classes.push("current");
            }
            if node.is_ancestor_of_current {
                classes.push("ancestor");
            }
            let class = if classes.is_empty() {
                String::new()
            } else {
                format!(" class=\"{}\"", classes.join(" "))
            };
            let aria = if node.is_current {
                " aria-current=\"page\""
            } else {
                ""
            };
            let number = if numbered {
                format!("<span class=\"book-nav-number\">{}</span>", i + 1)
            } else {
                String::new()
            };
            let parts = if node.children.is_empty() {
                String::new()
            } else {
                format!(
                    "<ol class=\"book-nav-parts\">{}</ol>",
                    items(&node.children, false, prefix)
                )
            };
            out.push_str(&format!(
                "<li{class}><a href=\"{prefix}{href}\"{aria}>{number}<span class=\"book-nav-label\">{title}</span></a>{parts}</li>",
                href = html_escape(&node.href),
                title = html_escape(&node.title),
            ));
        }
        out
    }

    let page = cx.pages.get(&book.href).copied();
    let aria = if book.is_current {
        " aria-current=\"page\""
    } else {
        ""
    };
    format!(
        "<nav class=\"book-nav\" aria-label=\"Contents\">\n  <a class=\"book-nav-up\" href=\"{prefix}{FRONT_PAGE_DEST}\">{site}</a>\n  <a class=\"book-nav-book{tone}\" href=\"{prefix}{href}\"{aria}><span class=\"book-nav-title\">{title}</span><span class=\"book-nav-count\">{held}</span></a>\n  <ol class=\"book-nav-list\">{items}</ol>\n</nav>",
        prefix = cx.root_prefix,
        site = html_escape(cx.library_title),
        tone = tone_class(page.and_then(|p| p.color.as_deref())),
        href = html_escape(&book.href),
        title = html_escape(&book.title),
        held = holds(&book.children, false),
        items = items(&book.children, true, cx.root_prefix),
    )
}

/// The rendered body less a leading `<h1>` that says the page's title.
///
/// Only when it says the title: a first heading that says something else is
/// the author's, and stays.
fn without_title(page: &PublishedPage) -> String {
    let body = &page.rendered_body;
    let trimmed = body.trim_start();
    let repeats_title = page
        .headings
        .first()
        .is_some_and(|h| h.level == 1 && same_words(&h.text, &page.title));
    if !repeats_title || !trimmed.starts_with("<h1") {
        return body.clone();
    }
    match trimmed.find("</h1>") {
        Some(end) => trimmed[end + "</h1>".len()..].trim_start().to_string(),
        None => body.clone(),
    }
}

fn same_words(a: &str, b: &str) -> bool {
    let words = |s: &str| {
        s.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
    };
    words(a) == words(b)
}
