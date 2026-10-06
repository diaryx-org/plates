//! A node a page draws in place: `::album{src="…"}`.
//!
//! A page links another node with a link, and the reader goes there. A page
//! *draws* one with a leaf directive naming it, and the reader sees it where
//! the directive stands: an album's photographs, as the gallery the album's own
//! page is ([`crate::manifest`]), captioned with the album's title and linking
//! to it. The vocabulary is Diaryx's (its editor draws the same directive), and
//! every other leaf directive still renders as an element wearing its
//! attributes ([`crate::body`]).
//!
//! The drawing is made of what the site already holds. The node a directive
//! names is one of the site's own sources, so a node the site's gate did not
//! admit is not there to draw, and the directive draws **nothing** — not a
//! placeholder, and not a link to a page that does not exist. That is the
//! whole of the disclosure rule, and it is the same one the template context
//! keeps: what this render was not handed, it cannot show.
//!
//! The body is rendered with a marker where each directive stood — a raw HTML
//! block the grammar passes through untouched — and the drawing is put in its
//! place once the page's own HTML, links rewritten, is done. Late, so the
//! drawing's references (written from the site's root already) are not
//! rewritten a second time as if the page had written them.

use std::path::{Path, PathBuf};

use prov::ContentFormat;
use prov::twig;

/// The directive that draws an album — a manifest node — as its gallery.
pub const ALBUM: &str = "album";

/// Every directive name this module draws.
pub const NAMES: &[&str] = &[ALBUM];

/// One directive the body draws a node with, found by [`mark`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The directive's name, one of [`NAMES`].
    pub name: String,
    /// The node its `src` names, resolved against the page and in the
    /// coordinates a source's `path` is in.
    pub target: PathBuf,
}

/// The marker standing where the `n`th directive stood, as the rendered HTML
/// carries it.
pub fn marker(n: usize) -> String {
    format!(r#"<div data-plates-embed="{n}"></div>"#)
}

/// `body` with every drawing directive replaced by a [`marker`], and the
/// directives in order — or `body` untouched and nothing, for a body that has
/// none, an HTML body, and one twig cannot parse.
///
/// Markdown takes the marker as a raw HTML block, djot inside a raw `=html`
/// block; either way it is a block of its own, with a blank line each side, so
/// it never joins the paragraph next to it.
pub fn mark(body: &str, format: ContentFormat, page: &Path) -> (String, Vec<Found>) {
    let untouched = || (body.to_string(), Vec::new());
    if !NAMES.iter().any(|name| body.contains(name)) || !body.contains("::") {
        return untouched();
    }
    let grammar = match format {
        ContentFormat::Markdown => twig::Format::Markdown,
        ContentFormat::Djot => twig::Format::Djot,
        _ => return untouched(),
    };
    let extensions = twig::MarkdownExtensions {
        directives: true,
        ..twig::MarkdownExtensions::default()
    };
    let Ok(mut doc) = twig::Document::parse_str_with(body, grammar, extensions) else {
        return untouched();
    };
    let Ok(nodes) = doc.nodes() else {
        return untouched();
    };
    let mut spans: Vec<(std::ops::Range<usize>, Found)> = nodes
        .iter()
        .filter(|n| {
            matches!(n.kind, twig::Kind::Container)
                && matches!(n.origin, Some(twig::ContainerOrigin::Directive))
                && match n.directive_form {
                    Some(twig::DirectiveForm::Leaf) => true,
                    // Djot has no leaf directive: twig writes one as an empty
                    // fenced div wearing the attributes, `{src="x"}` over
                    // `::: album` and `:::`, whose word is its class.
                    Some(twig::DirectiveForm::Container) => grammar == twig::Format::Djot,
                    _ => false,
                }
        })
        .filter_map(|n| {
            let attr = |key: &str| {
                n.attrs
                    .iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| v.as_deref())
            };
            let name = n
                .name
                .as_deref()
                .filter(|name| !name.is_empty())
                .or_else(|| attr("class").and_then(|c| c.split_whitespace().next()))
                .filter(|name| NAMES.contains(name))?;
            let src = attr("src").map(str::trim).filter(|s| !s.is_empty())?;
            if src.starts_with("id:") || src.contains("://") || src.starts_with('#') {
                return None;
            }
            let target = prov::link::resolve(page, src);
            Some((
                n.span.clone(),
                Found {
                    name: name.to_string(),
                    target,
                },
            ))
        })
        .collect();
    if spans.is_empty() {
        return untouched();
    }
    spans.sort_by_key(|(span, _)| span.start);

    let mut out = String::with_capacity(body.len());
    let mut cursor = 0;
    let mut found = Vec::with_capacity(spans.len());
    for (span, embed) in spans {
        if span.start < cursor {
            continue;
        }
        out.push_str(&body[cursor..span.start]);
        let block = match grammar {
            twig::Format::Djot => format!("\n\n``` =html\n{}\n```\n\n", marker(found.len())),
            _ => format!("\n\n{}\n\n", marker(found.len())),
        };
        out.push_str(&block);
        cursor = span.end;
        found.push(embed);
    }
    out.push_str(&body[cursor..]);
    (out, found)
}

/// `html` with each [`marker`] replaced by its drawing — `draw(n)`, or nothing
/// where that answers `None`.
pub fn fill(html: &str, count: usize, mut draw: impl FnMut(usize) -> Option<String>) -> String {
    let mut out = html.to_string();
    for n in 0..count {
        let drawing = draw(n).unwrap_or_default();
        out = out.replacen(&marker(n), &drawing, 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_album_directive_is_marked_and_named() {
        let body = "Before.\n\n::album{src=\"holiday.yaml\"}\n\nAfter.\n";
        let (marked, found) = mark(body, ContentFormat::Markdown, Path::new("trip/trip.md"));
        assert_eq!(
            found,
            vec![Found {
                name: ALBUM.into(),
                target: PathBuf::from("trip/holiday.yaml"),
            }]
        );
        assert!(marked.contains(&marker(0)), "{marked}");
        assert!(!marked.contains("::album"), "{marked}");
        assert!(marked.contains("Before.") && marked.contains("After."));
    }

    #[test]
    fn djot_spells_it_as_an_empty_div() {
        let body = "Before.\n\n{src=\"holiday.yaml\"}\n::: album\n:::\n";
        let (marked, found) = mark(body, ContentFormat::Djot, Path::new("trip/trip.dj"));
        assert_eq!(found.len(), 1, "{marked}");
        assert!(marked.contains(&marker(0)), "{marked}");
        assert!(!marked.contains("::: album"), "{marked}");
    }

    #[test]
    fn other_directives_and_code_are_left_alone() {
        let body = "::embed{src=\"x.yaml\"}\n\n```\n::album{src=\"y.yaml\"}\n```\n";
        let (marked, found) = mark(body, ContentFormat::Markdown, Path::new("a.md"));
        assert!(found.is_empty());
        assert_eq!(marked, body);
    }

    #[test]
    fn fill_puts_each_drawing_in_place_or_nothing() {
        let html = format!("<p>a</p>\n{}\n{}\n", marker(0), marker(1));
        let out = fill(&html, 2, |n| {
            (n == 0).then(|| "<figure>x</figure>".to_string())
        });
        assert_eq!(out, "<p>a</p>\n<figure>x</figure>\n\n");
    }
}
