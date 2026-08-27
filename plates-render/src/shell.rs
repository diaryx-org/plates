//! The site shell as named slots, and the substitutor that fills a
//! caller-supplied template with them.
//!
//! A shell template is the outer HTML document a page is wrapped in: everything
//! from `<!DOCTYPE html>` down to `</html>`, with the parts this crate computes
//! left as named slots. The built-in shell in [`crate::html`] fills exactly the
//! same [`ShellSlots`], so a template is a replacement for that document rather
//! than a second, parallel notion of what a page is made of.
//!
//! ## Why not handlebars
//!
//! The reason that decides it is that **handlebars-rust has no configurable
//! delimiters** — `{{` is hardcoded in its grammar. A shell is an HTML
//! document, which is exactly where inline `<style>` and `<script>` braces
//! live, and the `braces_that_are_not_slots_pass_through` test below guarantees
//! that `<style>a{b:c}</style>` and `<script>if(x){{y()}}</script>` survive a
//! shell verbatim. Handlebars would read `{{y()}}` as an expression, breaking
//! every existing theme in favour of `\{{`.
//!
//! Two further reasons used to be listed here and are recorded as *refuted*,
//! since both are contradicted by this crate's own code: that handlebars
//! escapes by its own rule (`register_escape_fn` installs one, so
//! [`crate::page::html_escape`] could have been it), and that a misspelled
//! variable cannot be reported precisely (`Template::elements` is `pub`, so
//! walking the compiled AST to validate slot names is a short function).
//!
//! Bodies pay no delimiter cost, because a body is Markdown — which is why
//! [`crate::template`] spells its values with a directive instead, and why this
//! module keeps a substitutor of its own rather than sharing one. It is
//! deliberately small: named slots, no expressions, no control flow. Anything a
//! shell wants to vary per page it varies by rendering a different site.
//!
//! ## Syntax
//!
//! `{{name}}` inserts a **text** slot, HTML-escaped. `{{{name}}}` inserts a
//! **raw HTML** slot verbatim. Whitespace inside the braces is allowed
//! (`{{ site_title }}`). The two spellings are not interchangeable: each slot is
//! one kind or the other, and writing it the other way is an error rather than a
//! silently escaped `<div>`. Anything that is not a well-formed slot reference —
//! `{{` in an inline script, a CSS block, a `{{}}` with no name — passes through
//! literally.

use crate::page::html_escape;

/// The named values a shell template is filled with.
///
/// Text fields hold their *unescaped* text; escaping happens where the slot is
/// filled, so a value is escaped exactly once no matter which shell renders it.
#[derive(Debug, Clone, Default)]
pub struct ShellSlots {
    /// `{{lang}}` — the document language, for `<html lang="…">`.
    pub lang: String,
    /// `{{document_title}}` — the `<title>` text: `"Entry - Site"`, or just the
    /// site's name on the page that *is* the site.
    pub document_title: String,
    /// `{{site_title}}` — the site's name on its own.
    pub site_title: String,
    /// `{{body_class}}` — the class list for `<body>`, to be written *inside*
    /// `class="…"`. Empty when the page has no site nav.
    pub body_class: String,
    /// `{{{head}}}` — stylesheet link, favicon link, SEO meta, feed links and
    /// the page's own `styles:`, as a newline-separated run of tags indented
    /// four spaces. Does **not** include `<title>`, which is its own slot.
    pub head: String,
    /// `{{{site_nav}}}` — the site navigation sidebar. Empty when the site has
    /// no nav tree.
    pub site_nav: String,
    /// `{{{breadcrumbs}}}` — the breadcrumb trail for this page.
    pub breadcrumbs: String,
    /// `{{{content}}}` — the rendered body, with its links already rewritten.
    pub content: String,
    /// `{{{footer}}}` — the built-in attribution footer.
    pub footer: String,
    /// `{{{scripts}}}` — the built-in interactivity script and the page's own
    /// `scripts:`, as a newline-separated run of tags indented four spaces.
    pub scripts: String,
}

/// Whether a slot is text (escaped on the way in) or raw HTML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Raw,
}

/// Every slot a template may name, with its kind. The order is the order the
/// error message lists them in.
const SLOTS: &[(&str, Kind)] = &[
    ("lang", Kind::Text),
    ("document_title", Kind::Text),
    ("site_title", Kind::Text),
    ("body_class", Kind::Text),
    ("head", Kind::Raw),
    ("site_nav", Kind::Raw),
    ("breadcrumbs", Kind::Raw),
    ("content", Kind::Raw),
    ("footer", Kind::Raw),
    ("scripts", Kind::Raw),
];

/// A shell template that could not be compiled. Carries a message written for
/// whoever wrote the template, since that is the only person who can fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellError(String);

impl ShellError {
    /// The message, for a caller that reports it its own way.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShellError {}

/// One piece of a compiled template.
#[derive(Debug)]
enum Segment {
    Literal(String),
    /// An index into [`SLOTS`], resolved at compile time so rendering is a
    /// lookup rather than a second parse.
    Slot(usize),
}

/// A compiled shell template.
///
/// Compiling is separate from rendering so a bad template is reported once,
/// against the file it came from, rather than once per page.
#[derive(Debug)]
pub struct ShellTemplate {
    segments: Vec<Segment>,
}

impl ShellTemplate {
    /// Compile a template, rejecting unknown slot names and slots written with
    /// the wrong braces for their kind.
    pub fn parse(source: &str) -> Result<Self, ShellError> {
        let bytes = source.as_bytes();
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] == b'{'
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'{'
                && let Some((name, raw, consumed)) = scan_slot(&source[i..])
            {
                let index = slot_index(name, raw)?;
                if !literal.is_empty() {
                    segments.push(Segment::Literal(std::mem::take(&mut literal)));
                }
                segments.push(Segment::Slot(index));
                i += consumed;
                continue;
            }

            let ch = source[i..].chars().next().unwrap_or('\u{fffd}');
            literal.push(ch);
            i += ch.len_utf8();
        }

        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }
        Ok(Self { segments })
    }

    /// Fill the template's slots.
    pub fn render(&self, slots: &ShellSlots) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Slot(index) => {
                    let (name, kind) = SLOTS[*index];
                    let value = slot_value(slots, name);
                    match kind {
                        Kind::Text => out.push_str(&html_escape(value)),
                        Kind::Raw => out.push_str(value),
                    }
                }
            }
        }
        out
    }
}

/// Look a slot name up, checking that the braces match its kind.
fn slot_index(name: &str, raw: bool) -> Result<usize, ShellError> {
    let Some(index) = SLOTS.iter().position(|(n, _)| *n == name) else {
        let known: Vec<&str> = SLOTS.iter().map(|(n, _)| *n).collect();
        return Err(ShellError(format!(
            "unknown shell slot `{name}`. Known slots: {}",
            known.join(", ")
        )));
    };
    let (_, kind) = SLOTS[index];
    match (kind, raw) {
        (Kind::Text, false) | (Kind::Raw, true) => Ok(index),
        (Kind::Text, true) => Err(ShellError(format!(
            "shell slot `{name}` is text and is HTML-escaped; write it as {{{{{name}}}}}"
        ))),
        (Kind::Raw, false) => Err(ShellError(format!(
            "shell slot `{name}` is raw HTML; write it as {{{{{{{name}}}}}}}"
        ))),
    }
}

fn slot_value<'a>(slots: &'a ShellSlots, name: &str) -> &'a str {
    match name {
        "lang" => &slots.lang,
        "document_title" => &slots.document_title,
        "site_title" => &slots.site_title,
        "body_class" => &slots.body_class,
        "head" => &slots.head,
        "site_nav" => &slots.site_nav,
        "breadcrumbs" => &slots.breadcrumbs,
        "content" => &slots.content,
        "footer" => &slots.footer,
        "scripts" => &slots.scripts,
        // Unreachable: `slot_index` accepted the name against the same table.
        _ => "",
    }
}

/// Read a slot reference off the front of `s`, which is known to start `{{`.
///
/// Returns the slot name, whether it was written with three braces, and how many
/// bytes it occupied. `None` when what follows is not a well-formed reference —
/// which is how a template carrying an inline script or a CSS block keeps its
/// braces.
fn scan_slot(s: &str) -> Option<(&str, bool, usize)> {
    let raw = s.as_bytes().get(2) == Some(&b'{');
    let open = if raw { 3 } else { 2 };
    let close = if raw { "}}}" } else { "}}" };

    let after_open = s.get(open..)?;
    let name_start = after_open.len() - after_open.trim_start_matches([' ', '\t']).len();
    let name_region = &after_open[name_start..];
    let name_len = name_region
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(name_region.len());
    if name_len == 0 {
        return None;
    }
    let name = &name_region[..name_len];

    let after_name = &name_region[name_len..];
    let pad = after_name.len() - after_name.trim_start_matches([' ', '\t']).len();
    if !after_name[pad..].starts_with(close) {
        return None;
    }

    Some((name, raw, open + name_start + name_len + pad + close.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots() -> ShellSlots {
        ShellSlots {
            lang: "en".into(),
            document_title: "A & B".into(),
            site_title: "<Site>".into(),
            body_class: "has-site-nav".into(),
            head: r#"<link rel="stylesheet" href="style.css">"#.into(),
            site_nav: "<nav>n</nav>".into(),
            breadcrumbs: "<p>b</p>".into(),
            content: "<p>Hello</p>".into(),
            footer: "<footer>f</footer>".into(),
            scripts: "<script>s</script>".into(),
        }
    }

    #[test]
    fn text_slots_are_escaped_and_raw_slots_are_not() {
        let t = ShellTemplate::parse("<title>{{document_title}}</title>{{{content}}}").unwrap();
        assert_eq!(
            t.render(&slots()),
            "<title>A &amp; B</title><p>Hello</p>",
            "text escaped, HTML passed through"
        );
    }

    #[test]
    fn whitespace_inside_the_braces_is_allowed() {
        let t = ShellTemplate::parse("{{ site_title }}|{{{ site_nav }}}").unwrap();
        assert_eq!(t.render(&slots()), "&lt;Site&gt;|<nav>n</nav>");
    }

    #[test]
    fn every_slot_is_reachable() {
        let source: String = SLOTS
            .iter()
            .map(|(name, kind)| match kind {
                Kind::Text => format!("[{{{{{name}}}}}]"),
                Kind::Raw => format!("[{{{{{{{name}}}}}}}]"),
            })
            .collect();
        let out = ShellTemplate::parse(&source).unwrap().render(&slots());
        assert!(out.contains("[en]"));
        assert!(out.contains("[has-site-nav]"));
        assert!(out.contains("[<footer>f</footer>]"));
        assert!(!out.contains("{{"), "nothing was left unfilled: {out}");
    }

    #[test]
    fn an_unknown_slot_is_an_error_naming_the_known_ones() {
        let err = ShellTemplate::parse("{{titel}}").unwrap_err();
        assert!(err.message().contains("unknown shell slot `titel`"));
        assert!(err.message().contains("document_title"));
    }

    #[test]
    fn a_raw_slot_written_as_text_is_an_error_rather_than_escaped_html() {
        let err = ShellTemplate::parse("{{content}}").unwrap_err();
        assert!(err.message().contains("raw HTML"), "{err}");
        assert!(err.message().contains("{{{content}}}"), "{err}");
    }

    #[test]
    fn a_text_slot_written_as_raw_is_an_error_rather_than_unescaped_text() {
        let err = ShellTemplate::parse("{{{site_title}}}").unwrap_err();
        assert!(err.message().contains("HTML-escaped"), "{err}");
        assert!(err.message().contains("{{site_title}}"), "{err}");
    }

    /// A shell carrying an inline script or a CSS block must survive it: braces
    /// that are not a slot reference are not the substitutor's business.
    #[test]
    fn braces_that_are_not_slots_pass_through() {
        let source = "<style>a{b:c}</style><script>if(x){{y()}}</script>{{}}{ {a} }";
        let t = ShellTemplate::parse(source).unwrap();
        assert_eq!(t.render(&slots()), source);
    }

    #[test]
    fn a_template_with_no_slots_is_itself() {
        let t = ShellTemplate::parse("<p>plain</p>").unwrap();
        assert_eq!(t.render(&ShellSlots::default()), "<p>plain</p>");
    }
}
