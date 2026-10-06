//! Page chrome spelled as directives: `:::hero` and `:::ledger`.
//!
//! A landing page wants a banner and a strip of labelled figures, and until
//! these two existed the only way to get them was to write the markup by hand
//! — a `<section class="hero">` around a `<div class="wrap">` around an
//! eyebrow paragraph. Every editor over the source then showed the chrome as
//! content: a stray first line above the heading, labels running into their
//! figures. These two directives let the source say what the block *is* and
//! leave the markup to the render.
//!
//! ```markdown
//! :::hero[About Diaryx]
//! # Open infrastructure for personal memory.
//!
//! What you remember shouldn't depend on whether a company stays alive.
//! :::
//!
//! :::ledger
//! - **Open source** Every core engine is MIT / Apache-2.0 and public
//! - **Plain files** Markdown and YAML in folders you control
//! :::
//! ```
//!
//! renders as
//!
//! ```html
//! <section class="hero">
//! <div class="wrap">
//! <p class="eyebrow">About Diaryx</p>
//! <h1>Open infrastructure for personal memory.</h1>
//! <p class="lede">What you remember shouldn't depend on whether a company stays alive.</p>
//! </div>
//! </section>
//! <div class="ledger">
//! <div class="wrap ledger-inner">
//! <div>
//! <span class="label">Open source</span>
//! <span class="val">Every core engine is MIT / Apache-2.0 and public</span>
//! </div>
//! …
//! </div>
//! </div>
//! ```
//!
//! # The rules
//!
//! - **`:::hero[eyebrow]`.** The label, when there is one, is the eyebrow.
//!   Every paragraph directly inside is a lede; every other block (the
//!   heading, a list) renders as it would anywhere.
//! - **`:::ledger`.** Each item of a list directly inside is one entry. An
//!   item that opens with bold text takes it as the entry's label, and the
//!   rest of the item is the value; an item without one is a value alone.
//!   Anything inside that is not a list renders as it would anywhere, inside
//!   the strip.
//! - A class or id on the directive (`:::hero{.tall #top}`) joins the outer
//!   element's own.
//!
//! The class names are the ones diaryx.org's stylesheet was written against,
//! which is the whole reason the vocabulary is two words and not a layout
//! language: it is what pages already ask for, given a spelling an editor can
//! show as a labelled panel.
//!
//! # How
//!
//! Each block is cut out of the source before the render, its children are
//! rendered one at a time, and the assembled markup goes back in where a
//! placeholder paragraph stood. Children rendered on their own cannot see the
//! rest of the page, so a reference-style link (`[x][ref]`) or a footnote
//! inside a hero or ledger has no definition to resolve against — write the
//! link inline.

use std::fmt::Write as _;

use prov::twig::{ContainerOrigin, DirectiveForm, FlatNode, Kind, NodeId};

/// The two chrome directives this module renders.
const HERO: &str = "hero";
const LEDGER: &str = "ledger";

/// A placeholder that no author writes: private-use code points around an
/// index. It stands in a paragraph of its own while the rest of the page
/// renders, and the chrome's markup replaces that paragraph afterwards.
fn placeholder(index: usize) -> String {
    format!("\u{E000}plates-chrome-{index}\u{E000}")
}

/// The chrome blocks of a parsed body, each with the markup that replaces it.
pub(crate) struct Chrome {
    /// `(source span, rendered HTML)`, in source order.
    blocks: Vec<(std::ops::Range<usize>, String)>,
}

impl Chrome {
    /// Find and render every `:::hero` and `:::ledger` in `nodes`, the parse of
    /// `source`. `render` renders a fragment of Markdown on its own.
    pub(crate) fn find(
        source: &str,
        nodes: &[FlatNode],
        render: &dyn Fn(&str) -> prov::Result<String>,
    ) -> prov::Result<Self> {
        let by_id = |id: NodeId| nodes.iter().find(|n| n.id == id);
        let mut blocks = Vec::new();
        for node in nodes {
            let Some(name) = chrome_name(node) else {
                continue;
            };
            // A block must sit where a paragraph can stand in for it: at the top
            // level or inside other containers, never in a list item or a quote,
            // whose continuation lines a placeholder would break. And one inside
            // another is rendered by its outer block's own pass.
            let mut placeable = true;
            let mut parent = node.parent.and_then(by_id);
            while let Some(p) = parent {
                match p.kind {
                    Kind::Doc => break,
                    Kind::Container if chrome_name(p).is_some() => placeable = false,
                    Kind::Container => {}
                    _ => placeable = false,
                }
                parent = p.parent.and_then(by_id);
            }
            if !placeable {
                continue;
            }
            let children: Vec<&FlatNode> =
                nodes.iter().filter(|n| n.parent == Some(node.id)).collect();
            let html = match name {
                HERO => hero(source, node, &children, render)?,
                _ => ledger(source, node, &children, nodes, render)?,
            };
            blocks.push((node.span.clone(), html));
        }
        Ok(Self { blocks })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// `source` with every chrome block replaced by a placeholder paragraph.
    pub(crate) fn cut(&self, source: &str) -> String {
        let mut out = source.to_string();
        // Back to front, so each replacement leaves the offsets before it true.
        for (index, (span, _)) in self.blocks.iter().enumerate().rev() {
            out.replace_range(span.clone(), &format!("\n\n{}\n\n", placeholder(index)));
        }
        out
    }

    /// `html`, rendered from [`Self::cut`]'s output, with each placeholder
    /// paragraph replaced by its block's markup.
    pub(crate) fn paste(&self, mut html: String) -> String {
        for (index, (_, markup)) in self.blocks.iter().enumerate() {
            let mark = placeholder(index);
            let paragraph = format!("<p>{mark}</p>\n");
            if html.contains(&paragraph) {
                html = html.replacen(&paragraph, markup, 1);
            } else {
                html = html.replacen(&mark, markup.trim_end(), 1);
            }
        }
        html
    }
}

/// `"hero"` or `"ledger"` when `node` is that container directive.
fn chrome_name(node: &FlatNode) -> Option<&'static str> {
    if !matches!(node.kind, Kind::Container)
        || !matches!(node.origin, Some(ContainerOrigin::Directive))
        || !matches!(node.directive_form, Some(DirectiveForm::Container))
    {
        return None;
    }
    match node.name.as_deref() {
        Some(HERO) => Some(HERO),
        Some(LEDGER) => Some(LEDGER),
        _ => None,
    }
}

fn hero(
    source: &str,
    node: &FlatNode,
    children: &[&FlatNode],
    render: &dyn Fn(&str) -> prov::Result<String>,
) -> prov::Result<String> {
    let mut out = String::new();
    let _ = writeln!(out, "<section{}>", outer_attrs(HERO, node));
    out.push_str("<div class=\"wrap\">\n");
    if let Some(label) = label(source, node)
        && !label.trim().is_empty()
    {
        let _ = writeln!(out, "<p class=\"eyebrow\">{}</p>", inline(&render(label)?));
    }
    for child in children {
        let text = &source[child.span.clone()];
        if matches!(child.kind, Kind::Para) {
            let _ = writeln!(out, "<p class=\"lede\">{}</p>", inline(&render(text)?));
        } else {
            out.push_str(&block(render(text)?));
        }
    }
    out.push_str("</div>\n</section>\n");
    Ok(out)
}

fn ledger(
    source: &str,
    node: &FlatNode,
    children: &[&FlatNode],
    nodes: &[FlatNode],
    render: &dyn Fn(&str) -> prov::Result<String>,
) -> prov::Result<String> {
    let kids = |id: NodeId| nodes.iter().filter(move |n| n.parent == Some(id));
    let mut out = String::new();
    let _ = writeln!(out, "<div{}>", outer_attrs(LEDGER, node));
    out.push_str("<div class=\"wrap ledger-inner\">\n");
    for child in children {
        if !matches!(child.kind, Kind::BulletList | Kind::OrderedList) {
            out.push_str(&block(render(&source[child.span.clone()])?));
            continue;
        }
        for item in kids(child.id).filter(|n| matches!(n.kind, Kind::ListItem)) {
            out.push_str("<div>\n");
            let para = kids(item.id).next();
            let body = para
                .and_then(|p| p.content_span.clone())
                .unwrap_or_else(|| item.content_span.clone().unwrap_or(item.span.clone()));
            // The label is a strong run that opens the item's paragraph.
            let strong = para.and_then(|p| {
                kids(p.id)
                    .next()
                    .filter(|s| matches!(s.kind, Kind::Strong) && s.span.start == body.start)
            });
            let value_from = match strong.and_then(|s| s.content_span.clone().map(|c| (s, c))) {
                Some((s, c)) => {
                    let _ = writeln!(
                        out,
                        "<span class=\"label\">{}</span>",
                        inline(&render(&source[c])?)
                    );
                    s.span.end
                }
                None => body.start,
            };
            let value = source[value_from..body.end].trim();
            if !value.is_empty() {
                let _ = writeln!(
                    out,
                    "<span class=\"val\">{}</span>",
                    inline(&render(value)?)
                );
            }
            out.push_str("</div>\n");
        }
    }
    out.push_str("</div>\n</div>\n");
    Ok(out)
}

/// The outer element's attributes: its own class first, then whatever the
/// author put on the directive.
fn outer_attrs(class: &str, node: &FlatNode) -> String {
    let mut classes = vec![class.to_string()];
    let mut rest = String::new();
    for (key, value) in &node.attrs {
        match (key.as_str(), value) {
            ("class", Some(v)) => classes.extend(v.split_whitespace().map(str::to_string)),
            (k, Some(v)) => {
                let _ = write!(rest, " {k}=\"{}\"", crate::page::html_escape(v));
            }
            (k, None) => {
                let _ = write!(rest, " {k}");
            }
        }
    }
    format!(" class=\"{}\"{rest}", classes.join(" "))
}

/// A container directive's `[label]`, read off its opening fence: twig keeps no
/// node for it.
fn label<'a>(source: &'a str, node: &FlatNode) -> Option<&'a str> {
    let fence_end = node
        .content_span
        .as_ref()
        .map(|c| c.start)
        .unwrap_or(node.span.end);
    let fence = &source[node.span.start..fence_end];
    let fence = fence.lines().next()?;
    let after_name = fence.trim_start_matches(':');
    let after_name =
        after_name.trim_start_matches(|c: char| c.is_alphanumeric() || c == '-' || c == '_');
    let rest = after_name.strip_prefix('[')?;
    let mut depth = 1usize;
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// A fragment rendered on its own comes back as one paragraph; its inside is
/// the inline markup.
fn inline(html: &str) -> &str {
    let html = html.trim();
    html.strip_prefix("<p>")
        .and_then(|h| h.strip_suffix("</p>"))
        .unwrap_or(html)
}

/// A rendered block, ending in exactly one newline.
fn block(html: String) -> String {
    let mut html = html.trim_end().to_string();
    html.push('\n');
    html
}

#[cfg(test)]
mod tests {
    use crate::render_body;
    use prov::ContentFormat;

    fn md(source: &str) -> String {
        render_body(source, ContentFormat::Markdown)
    }

    /// Whitespace between and just inside block tags is layout, not markup: the
    /// hand-written pages indent theirs and the render does not.
    fn normalize(html: &str) -> String {
        let mut out = html.split_whitespace().collect::<Vec<_>>().join(" ");
        out = out.replace("> <", "><");
        for tag in ["section", "div", "p", "h1", "span"] {
            for open in [format!("<{tag}>"), format!("<{tag} ")] {
                let mut from = 0;
                while let Some(at) = out[from..].find(&open) {
                    let start = from + at;
                    let Some(close) = out[start..].find('>') else {
                        break;
                    };
                    let end = start + close + 1;
                    if out[end..].starts_with(' ') {
                        out.remove(end);
                    }
                    from = end;
                }
            }
            out = out.replace(&format!(" </{tag}>"), &format!("</{tag}>"));
        }
        out
    }

    /// The markup the site's pages carried by hand before these directives
    /// existed, in the shape diaryx.org's About page wrote it: indentation,
    /// line breaks and all.
    const HAND_WRITTEN: &str = r#"<section class="hero">
<div class="wrap">
<p class="eyebrow">About the shelf</p>

# A place for things to stay.

<p class="lede">
What you keep should outlast the tools that made it, and
<a href="../index.html">the app</a> is one way in.
</p>
</div>
</section>

<div class="ledger">
  <div class="wrap ledger-inner">
    <div>
      <span class="label">Open source</span>
      <span class="val">Every engine is public</span>
    </div>
    <div>
      <span class="label">Plain files</span>
      <span class="val">Markdown and YAML in folders you control</span>
    </div>
  </div>
</div>
"#;

    const DIRECTIVES: &str = r#":::hero[About the shelf]
# A place for things to stay.

What you keep should outlast the tools that made it, and
[the app](../index.html) is one way in.
:::

:::ledger
- **Open source** Every engine is public
- **Plain files** Markdown and YAML in folders you control
:::
"#;

    /// The directives render to the markup the pages used to write by hand.
    #[test]
    fn hero_and_ledger_render_the_hand_written_markup() {
        let before = md(HAND_WRITTEN);
        let after = md(DIRECTIVES);
        assert_eq!(
            normalize(&after),
            normalize(&before),
            "\n{after}\n---\n{before}"
        );
    }

    #[test]
    fn a_hero_without_a_label_has_no_eyebrow() {
        let html = md(":::hero\n# Title\n\nOne.\n\nTwo.\n:::\n");
        assert!(!html.contains("eyebrow"), "{html}");
        assert!(html.contains("<p class=\"lede\">One.</p>"), "{html}");
        assert!(html.contains("<p class=\"lede\">Two.</p>"), "{html}");
        assert!(!html.contains("<hero"), "{html}");
    }

    #[test]
    fn a_ledger_item_without_bold_is_a_value_alone() {
        let html = md(":::ledger\n- just a figure\n:::\n");
        assert!(!html.contains("class=\"label\""), "{html}");
        assert!(
            html.contains("<span class=\"val\">just a figure</span>"),
            "{html}"
        );
    }

    #[test]
    fn attributes_join_the_outer_element() {
        let html = md(":::hero{.tall #top}\n# T\n:::\n");
        assert!(
            html.contains("<section class=\"hero tall\" id=\"top\">"),
            "{html}"
        );
    }

    /// The prose around a block is untouched, and the page still reads in order.
    #[test]
    fn prose_around_chrome_keeps_its_place() {
        let html = md("Before.\n\n:::ledger\n- **A** b\n:::\nAfter.\n");
        let before = html.find("<p>Before.</p>").expect(&html);
        let ledger = html.find("<div class=\"ledger\">").expect(&html);
        let after = html.find("<p>After.</p>").expect(&html);
        assert!(before < ledger && ledger < after, "{html}");
        assert!(!html.contains('\u{E000}'), "{html}");
    }

    /// Quoted in a code fence, a directive is text.
    #[test]
    fn a_fenced_hero_is_quoted() {
        let html = md("```\n:::hero[x]\n:::\n```\n");
        assert!(html.contains(":::hero[x]"), "{html}");
        assert!(!html.contains("class=\"hero\""), "{html}");
    }
}
