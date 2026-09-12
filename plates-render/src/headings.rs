//! Heading anchors and the page outline, as a pass over rendered HTML.
//!
//! Every `<h1>`–`<h6>` in a body leaves here with an `id` and a link to
//! itself, and the pass hands back the list of what it found — which is what
//! the `toc` shell slot, the `headings` template key and the built-in "On this
//! page" block are all made of. One pass, on the output, on the precedent
//! [`crate::syntax`] set: twig's serializer offers no heading-id option, and a
//! pass over the HTML covers Markdown, Djot and hand-written HTML bodies with
//! one implementation.
//!
//! # The id
//!
//! [`prov::link::slug`] of the heading's text — the same function
//! [`crate::page::title_to_anchor`] uses, so a single-file render and a site
//! render agree on what `## Status` is called. A second heading with the same
//! slug on one page gets `-2`, then `-3`. A heading that already carries an
//! `id` — an HTML body, a Djot `{#custom}` attribute — keeps it, and still
//! counts towards the numbering so a later `## Status` cannot collide with it.
//!
//! # The anchor
//!
//! ```html
//! <h2 id="status">Status <a class="heading-anchor" href="#status" aria-label="Link to this section">#</a></h2>
//! ```
//!
//! Inside the heading rather than beside it, so the heading's text is what a
//! screen reader reads first and the link is one tab stop after it. The
//! stylesheet hides the mark until the heading is hovered or the link focused.

use crate::page::html_escape;
use crate::types::Heading;

/// The class on the anchor link this pass appends to every heading.
pub const ANCHOR_CLASS: &str = "heading-anchor";

/// Give every heading in `html` an `id` and an anchor, and list them.
///
/// Headings are returned in document order, each with the id it ended up
/// with and its text with markup stripped and entities decoded — the string a
/// template or an outline wants to print, not the bytes twig wrote.
pub fn anchor_headings(html: &str) -> (String, Vec<Heading>) {
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    let mut headings = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    let mut rest = html;

    while let Some(at) = find_heading_open(rest) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(open) = split_open_tag(rest) else {
            // `<h2` that never closes its tag: not a heading, and nothing after
            // it can be either — publish the remainder as it is.
            break;
        };
        let close = format!("</h{}>", open.level);
        let after_open = &rest[open.len..];
        let Some(end) = after_open.find(&close) else {
            break;
        };
        let inner = &after_open[..end];
        let text = decode_entities(&strip_tags(inner));

        let id = match open.id {
            Some(id) => id.to_string(),
            None => unique_id(&prov::link::slug(&text), &taken),
        };
        taken.push(id.clone());

        let escaped_id = html_escape(&id);
        out.push_str(&format!("<h{}", open.level));
        if open.id.is_none() {
            out.push_str(&format!(r#" id="{escaped_id}""#));
        }
        out.push_str(open.attrs);
        out.push('>');
        out.push_str(inner);
        out.push_str(&format!(
            r##" <a class="{ANCHOR_CLASS}" href="#{escaped_id}" aria-label="Link to this section">#</a>"##
        ));
        out.push_str(&close);

        headings.push(Heading {
            level: open.level,
            id,
            text,
        });
        rest = &after_open[end + close.len()..];
    }

    out.push_str(rest);
    (out, headings)
}

/// The byte offset of the next `<h1`–`<h6` tag opening in `s`, when there is
/// one and it is a tag rather than the start of a longer name (`<h2>` yes,
/// `<header>` no).
fn find_heading_open(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut from = 0;
    while let Some(rel) = s[from..].find("<h") {
        let at = from + rel;
        if let (Some(level), Some(next)) = (bytes.get(at + 2), bytes.get(at + 3))
            && (b'1'..=b'6').contains(level)
            && (next.is_ascii_whitespace() || *next == b'>' || *next == b'/')
        {
            return Some(at);
        }
        from = at + 2;
    }
    None
}

/// An opening heading tag, taken apart.
struct OpenTag<'a> {
    level: u8,
    /// The value of an `id` attribute the tag already carries.
    id: Option<&'a str>,
    /// Everything between the tag name and the `>`, to be written back as it
    /// was: an HTML body's own `class`, a Djot attribute's `id`.
    attrs: &'a str,
    /// How many bytes of the input the opening tag occupies.
    len: usize,
}

/// Split the opening tag at the start of `s`, which is known to begin `<hN`.
fn split_open_tag(s: &str) -> Option<OpenTag<'_>> {
    let level = s.as_bytes()[2] - b'0';
    let gt = s.find('>')?;
    let attrs = &s[3..gt];
    Some(OpenTag {
        level,
        id: attribute(attrs, "id"),
        attrs,
        len: gt + 1,
    })
}

/// The value of `name="…"` (or `name='…'`) among a tag's attributes.
fn attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(rel) = attrs[from..].find(name) {
        let at = from + rel;
        let before_ok = at == 0 || attrs.as_bytes()[at - 1].is_ascii_whitespace();
        let after = &attrs[at + name.len()..];
        let after = after.trim_start();
        if before_ok && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &value[1..];
                let end = body.find(quote)?;
                return Some(&body[..end]);
            }
            let end = value
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(value.len());
            return Some(&value[..end]);
        }
        from = at + name.len();
    }
    None
}

/// `slug`, or `slug-2`, `slug-3`, … — the first spelling not already taken.
fn unique_id(slug: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == slug) {
        return slug.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{slug}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The text of a fragment of HTML, tags removed.
fn strip_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    text
}

/// The five entities twig's escaper writes, put back — the text of a heading
/// called `Ben & Co` is `Ben & Co`, and its slug is `ben-co` rather than
/// `ben-amp-co`.
fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// The outline an `<nav class="toc">` holds: a nested list of the page's
/// `h2`–`h3` headings, each linking to its anchor.
///
/// Empty — no element at all — when there are fewer than two of them: an
/// outline of one entry is a heading the reader can already see. Levels above
/// `h2` are left out because a body's `h1` is its title, and below `h3`
/// because an outline that lists every `h5` is the page again.
pub fn render_toc(headings: &[Heading]) -> String {
    let listed: Vec<&Heading> = headings
        .iter()
        .filter(|h| h.level == 2 || h.level == 3)
        .collect();
    if listed.len() < 2 {
        return String::new();
    }

    let mut out = String::from(
        r#"<nav class="toc" aria-label="On this page"><details open><summary>On this page</summary><ul>"#,
    );
    // Whether the cursor is inside an `h3` sub-list. Two levels is the whole
    // grammar, so a flag is the whole state.
    let mut nested = false;
    for (i, h) in listed.iter().enumerate() {
        match (h.level, nested) {
            (2, true) => {
                out.push_str("</li></ul></li>");
                nested = false;
            }
            (2, false) if i > 0 => out.push_str("</li>"),
            (3, false) => {
                // A leading `h3` with no `h2` above it still gets an item to
                // hang from; the item is just empty.
                if i == 0 {
                    out.push_str("<li>");
                }
                out.push_str("<ul>");
                nested = true;
            }
            (3, true) => out.push_str("</li>"),
            _ => {}
        }
        out.push_str(&format!(
            r##"<li><a href="#{}">{}</a>"##,
            html_escape(&h.id),
            html_escape(&h.text)
        ));
    }
    if nested {
        out.push_str("</li></ul>");
    }
    out.push_str("</li></ul></details></nav>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_heading_gets_an_id_and_an_anchor() {
        let (html, headings) = anchor_headings("<h1>Title</h1>\n<p>x</p>\n<h2>A Section</h2>");
        assert_eq!(
            html,
            "<h1 id=\"title\">Title <a class=\"heading-anchor\" href=\"#title\" aria-label=\"Link to this section\">#</a></h1>\n\
             <p>x</p>\n\
             <h2 id=\"a-section\">A Section <a class=\"heading-anchor\" href=\"#a-section\" aria-label=\"Link to this section\">#</a></h2>"
        );
        assert_eq!(headings.len(), 2);
        assert_eq!((headings[0].level, headings[0].id.as_str()), (1, "title"));
        assert_eq!(headings[1].text, "A Section");
    }

    /// The same slug twice on one page is numbered, so both are addressable.
    #[test]
    fn a_repeated_heading_is_numbered() {
        let (html, headings) = anchor_headings("<h2>Status</h2><h2>Status</h2><h2>Status</h2>");
        assert!(html.contains(r##"id="status">"##));
        assert!(html.contains(r##"id="status-2">"##));
        assert!(html.contains(r##"id="status-3">"##));
        assert_eq!(headings[2].id, "status-3");
    }

    /// An `id` the body already carries — an HTML body's, or a Djot
    /// `{#custom}` — is the heading's name, and the pass does not rename it.
    #[test]
    fn an_existing_id_is_kept() {
        let (html, headings) = anchor_headings(r#"<h2 id="custom" class="x">Custom</h2>"#);
        assert!(
            html.starts_with(r#"<h2 id="custom" class="x">Custom "#),
            "got {html}"
        );
        assert!(html.contains(r##"href="#custom""##));
        assert_eq!(headings[0].id, "custom");
        // …and it is taken, so a heading that would slug to it is numbered.
        let (_, headings) = anchor_headings(r#"<h2 id="status">A</h2><h2>Status</h2>"#);
        assert_eq!(headings[1].id, "status-2");
    }

    /// Text is what a reader sees: markup gone, entities put back, and the
    /// slug is made from that rather than from `&amp;`.
    #[test]
    fn heading_text_is_the_text_the_reader_sees() {
        let (html, headings) = anchor_headings("<h2>Ben &amp; <em>Co</em></h2>");
        assert_eq!(headings[0].text, "Ben & Co");
        assert_eq!(headings[0].id, "ben-co");
        assert!(html.contains("<em>Co</em>"), "the markup survives in place");
    }

    /// `<header>` and `<hr>` are not headings, and a heading inside a code
    /// block is text twig already escaped.
    #[test]
    fn only_headings_are_touched() {
        let source =
            "<header><h2>In</h2></header><hr><pre><code>&lt;h2&gt;no&lt;/h2&gt;</code></pre>";
        let (html, headings) = anchor_headings(source);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].id, "in");
        assert!(html.contains("<header>"));
        assert!(html.contains("<hr>"));
        assert!(html.contains("&lt;h2&gt;no&lt;/h2&gt;"));
    }

    #[test]
    fn a_body_with_no_headings_is_itself() {
        let (html, headings) = anchor_headings("<p>plain</p>");
        assert_eq!(html, "<p>plain</p>");
        assert!(headings.is_empty());
    }

    fn h(level: u8, id: &str) -> Heading {
        Heading {
            level,
            id: id.to_string(),
            text: id.to_uppercase(),
        }
    }

    #[test]
    fn the_outline_nests_h3_under_h2_and_lists_nothing_else() {
        let toc = render_toc(&[
            h(1, "title"),
            h(2, "a"),
            h(3, "a1"),
            h(3, "a2"),
            h(2, "b"),
            h(4, "deep"),
        ]);
        assert_eq!(
            toc,
            r##"<nav class="toc" aria-label="On this page"><details open><summary>On this page</summary><ul><li><a href="#a">A</a><ul><li><a href="#a1">A1</a></li><li><a href="#a2">A2</a></li></ul></li><li><a href="#b">B</a></li></ul></details></nav>"##
        );
    }

    /// One entry is not an outline.
    #[test]
    fn an_outline_needs_two_entries() {
        assert_eq!(render_toc(&[h(1, "t"), h(2, "only")]), "");
        assert!(!render_toc(&[h(2, "a"), h(2, "b")]).is_empty());
    }
}
