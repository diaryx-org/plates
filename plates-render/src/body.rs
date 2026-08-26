//! Body prose → HTML, in whichever grammar the document is written in.
//!
//! Three stages run in order:
//! 1. [`preprocess_custom_syntax`] rewrites Diaryx-specific syntax (highlights,
//!    spoilers, HTML embeds) into raw HTML, skipping fenced/inline code.
//! 2. [`render_body`] parses the result as the document's [`ContentFormat`] and
//!    renders it, via `twig` (through [`prov::render_html`]).
//! 3. With the `syntax-highlighting` feature, [`crate::syntax`] colours the
//!    fenced code blocks in that HTML. Third rather than woven into stage two
//!    because `prov::render_html` is one string-to-string call with no node
//!    hook — and being a pass over the output is what lets it cover all three
//!    grammars, hand-written HTML bodies included, with one implementation.
//!
//! ## Why twig rather than a Markdown-only parser
//!
//! This crate used to run comrak, which meant Diaryx could only ever publish
//! Markdown, and meant the publisher parsed a document with a different engine
//! than the editor did — the editor has always been twig, through `leaf`. One
//! engine for three grammars is the whole reason `content_format` can exist:
//! `twig` is already linked into every build (via `prov` *and* `leaf`), it
//! ships a `wasm32-unknown-unknown` package so this crate stays portable to the
//! Cloudflare worker, and it covers what comrak covered — tables,
//! strikethrough, tasklists, footnotes, autolinks, raw-HTML passthrough.
//!
//! Its HTML is not byte-identical to comrak's: tasklists come out as
//! `<ul class="task-list">` and footnotes as `role="doc-endnotes"` with `#fn1`
//! anchors rather than comrak's `#fn-1`. `html_format_css.css` styles both
//! spellings, so a site published before this change and one published after
//! render the same.

use prov::ContentFormat;

/// Render a document body to HTML.
///
/// `format` is the document's own grammar — taken from its extension, not from
/// the vault's `content_format`, because a vault may hold both (an imported
/// `.html` artifact beside a `.md` transcription is the normal case, not the
/// exotic one).
///
/// A body twig cannot parse renders as escaped source in a `<pre>` rather than
/// failing the page: a publish that drops one document's prose on the floor is
/// worse than one that shows it unformatted, and the alternative — comrak's
/// infallible signature — was only infallible because it silently accepted
/// anything as Markdown.
pub fn render_body(body: &str, format: ContentFormat) -> String {
    let html = render_markup(body, format);
    // The built-in grammars. A caller with its own reaches for
    // [`render_body_with`]; this spelling stays the one that needs no setup.
    #[cfg(feature = "syntax-highlighting")]
    let html = crate::syntax::highlight_code_blocks(&html, crate::syntax::Syntaxes::bundled());
    html
}

/// Render a document body to HTML, highlighting its code with `syntaxes`.
///
/// What [`render_body`] does, against a grammar set the caller assembled —
/// which is how a site publishes code in a language the built-in set has no
/// grammar for. Building that set is the expensive half, so build it once and
/// pass it to every page rather than once per page. See [`crate::syntax`].
#[cfg(feature = "syntax-highlighting")]
pub fn render_body_with(
    body: &str,
    format: ContentFormat,
    syntaxes: &crate::syntax::Syntaxes,
) -> String {
    crate::syntax::highlight_code_blocks(&render_markup(body, format), syntaxes)
}

/// Stages one and two: the document's own grammar, through twig, with no
/// colour applied yet.
fn render_markup(body: &str, format: ContentFormat) -> String {
    let mut preprocessed = preprocess_custom_syntax(body, format);
    // twig drops the *content* of a Djot raw inline span (`` `…`{=html} ``) when
    // the source does not end in a newline — `a `x`{=html} b` renders as
    // `<p>a  b</p>`. A document whose last line is unterminated is ordinary, so
    // this is not only a test artifact: without the newline, a highlight on the
    // final line of a Djot entry would silently vanish from the published page.
    // Terminating the source is semantically neutral in all three grammars.
    if !preprocessed.ends_with('\n') {
        preprocessed.push('\n');
    }
    prov::render_html(&preprocessed, format).unwrap_or_else(|_| {
        format!(
            "<pre class=\"diaryx-unrendered\">{}</pre>\n",
            html_escape(body)
        )
    })
}

/// Pre-process Diaryx's custom syntax (highlights, spoilers, HTML embeds) into
/// raw HTML before the body is parsed. Skips fenced code blocks and inline code.
///
/// Runs for Markdown and Djot, which share backticks for both code spellings,
/// so the same scanner keeps its hands off code in either. It is deliberately
/// the *same* syntax in both: someone who switches a vault's `content_format`
/// should not find that `==highlight==` stopped working. Djot's native
/// `{=highlight=}` still works too — twig parses it — it just renders a plain
/// `<mark>` without Diaryx's colour classes.
///
/// HTML bodies are returned untouched. `==` and `||` are literal text there,
/// and a body that is already HTML has no need of an escape hatch into it.
pub fn preprocess_custom_syntax(source: &str, format: ContentFormat) -> String {
    if format == ContentFormat::Html {
        return source.to_string();
    }
    let markdown = source;
    let bytes = markdown.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        // Skip fenced code blocks (``` ... ```)
        if i + 2 < len && bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
            let fence_start = i;
            i += 3;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            loop {
                if i >= len {
                    out.push_str(&markdown[fence_start..]);
                    return out;
                }
                if bytes[i] == b'\n'
                    && i + 3 < len
                    && bytes[i + 1] == b'`'
                    && bytes[i + 2] == b'`'
                    && bytes[i + 3] == b'`'
                {
                    i += 4;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                    break;
                }
                i += 1;
            }
            out.push_str(&markdown[fence_start..i]);
            continue;
        }

        // Skip inline code (` ... `)
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            out.push_str(&markdown[start..i]);
            continue;
        }

        // An escaped opener is text, not syntax. Both characters are emitted
        // verbatim so the body's own parser does the unescaping — `\!` renders
        // as `!` in Markdown and Djot alike — which is the difference between
        // `\![x](y.html)` reading as a literal embed and becoming an island.
        // `\\` is consumed as a pair so an escaped backslash does not shield the
        // opener after it.
        if bytes[i] == b'\\'
            && let Some(next) = bytes.get(i + 1)
            && matches!(next, b'\\' | b'!' | b'=' | b'|')
        {
            out.push_str(&markdown[i..i + 2]);
            i += 2;
            continue;
        }

        // Try HTML embed: ![alt](path.html) or ![alt](path.htm)
        if bytes[i] == b'!'
            && i + 1 < len
            && bytes[i + 1] == b'['
            && let Some((html, consumed)) = try_parse_html_embed(&markdown[i..])
        {
            out.push_str(&raw_inline(&html, format));
            i += consumed;
            continue;
        }

        // Try highlight: ==text== or =={color}text==
        if i + 1 < len
            && bytes[i] == b'='
            && bytes[i + 1] == b'='
            && let Some((html, consumed)) = try_parse_highlight(&markdown[i..])
        {
            out.push_str(&raw_inline(&html, format));
            i += consumed;
            continue;
        }

        // Try spoiler: ||text||
        if i + 1 < len
            && bytes[i] == b'|'
            && bytes[i + 1] == b'|'
            && let Some((html, consumed)) = try_parse_spoiler(&markdown[i..])
        {
            out.push_str(&raw_inline(&html, format));
            i += consumed;
            continue;
        }

        out.push(markdown[i..].chars().next().unwrap());
        i += markdown[i..].chars().next().unwrap().len_utf8();
    }

    out
}

/// Wrap a generated HTML fragment so the body's own parser passes it through
/// verbatim instead of escaping it.
///
/// Markdown needs nothing — twig emits inline raw HTML as-is. Djot does not:
/// a bare `<mark>` comes out as `&lt;mark&gt;`, and the only way in is an inline
/// raw span, `` `…`{=html} ``. The fence is one backtick longer than the longest
/// run inside the fragment, because the highlight/spoiler scanners can swallow a
/// backtick that the inline-code branch didn't reach first (`==a ` b==`), and a
/// fence the content also contains would close the span early.
fn raw_inline(html: &str, format: ContentFormat) -> String {
    if format != ContentFormat::Djot {
        return html.to_string();
    }
    let longest = html
        .split(|c| c != '`')
        .map(|run| run.len())
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    // Djot reads a leading/trailing backtick as part of the fence unless a
    // space separates them; the space is not part of the raw content.
    let pad = if html.starts_with('`') || html.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{html}{pad}{fence}{{=html}}")
}

/// Try to parse a highlight starting at `==`. Returns `(html, bytes_consumed)`.
fn try_parse_highlight(s: &str) -> Option<(String, usize)> {
    const VALID_COLORS: &[&str] = &[
        "red", "orange", "yellow", "green", "cyan", "blue", "violet", "pink", "brown", "grey",
    ];

    if !s.starts_with("==") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("==") {
        return None;
    }

    let (color, content_start) = if after_open.starts_with('{') {
        let close_brace = after_open.find('}')?;
        let color_name = &after_open[1..close_brace];
        if !VALID_COLORS.contains(&color_name) {
            return None;
        }
        (color_name, close_brace + 1)
    } else {
        ("yellow", 0)
    };

    let content_region = &after_open[content_start..];
    let close_pos = content_region.find("==")?;
    if close_pos == 0 {
        return None;
    }

    let content = &content_region[..close_pos];
    if content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + content_start + close_pos + 2;
    let html = format!(
        r#"<mark data-highlight-color="{color}" class="highlight-mark highlight-{color}">{content}</mark>"#,
        color = color,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

/// Try to parse a spoiler starting at `||`. Returns `(html, bytes_consumed)`.
fn try_parse_spoiler(s: &str) -> Option<(String, usize)> {
    if !s.starts_with("||") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("||") {
        return None;
    }

    let close_pos = after_open.find("||")?;
    if close_pos == 0 {
        return None;
    }

    let content = &after_open[..close_pos];
    if content.contains('|') || content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + close_pos + 2;
    let html = format!(
        r#"<span data-spoiler="" class="spoiler-mark spoiler-hidden">{content}</span>"#,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

/// The height range an island is allowed to occupy, in CSS pixels.
///
/// The same clamp the parent-side resize bridge applies to a measurement from
/// the frame (see `HtmlRenderer::interactivity_script`), applied here to the
/// authored `{height=…}` so the two cannot disagree about what an island may be:
/// a one-pixel embed is invisible, and one taller than any screen is a scroll
/// trap in a page that already scrolls.
const ISLAND_MIN_HEIGHT: u32 = 200;
const ISLAND_MAX_HEIGHT: u32 = 4000;

/// Try to parse an HTML embed starting at `![`. Returns `(html, bytes_consumed)`.
///
/// Matches `![alt](path.html)` or `![alt](path.htm)`, optionally followed by an
/// attribute block, and converts it to a sandboxed `<iframe>` tag. This runs
/// before the body's own parser so the raw HTML is passed through unchanged.
///
/// The only attribute is `{height=400}`, which sets the frame's initial
/// `min-height` — what the reader sees before the resize bridge has measured the
/// document, and what they keep seeing if it loads no child script. An attribute
/// block spelling anything else leaves the whole embed unmatched, so it falls
/// through to ordinary image parsing: an unknown attribute is more likely a
/// syntax this version has not learned than a mistake worth eating the embed
/// over, and a visible `![demo](x.html){wdith=400}` is a legible way to say so.
fn try_parse_html_embed(s: &str) -> Option<(String, usize)> {
    if !s.starts_with("![") {
        return None;
    }

    let after_bang = &s[2..];
    let close_bracket = after_bang.find(']')?;
    let alt = &after_bang[..close_bracket];

    let after_bracket = &after_bang[close_bracket + 1..];
    if !after_bracket.starts_with('(') {
        return None;
    }

    let after_paren = &after_bracket[1..];
    let close_paren = after_paren.find(')')?;
    let path = after_paren[..close_paren].trim();

    // Only match .html / .htm extensions
    let lower = path.to_lowercase();
    if !lower.ends_with(".html") && !lower.ends_with(".htm") {
        return None;
    }

    let mut total_consumed = 2 + close_bracket + 1 + 1 + close_paren + 1;
    let mut min_height = ISLAND_MIN_HEIGHT;
    let after_embed = &s[total_consumed..];
    if after_embed.starts_with('{') {
        let close_brace = after_embed.find('}')?;
        min_height = parse_island_height(&after_embed[1..close_brace])?;
        total_consumed += close_brace + 1;
    }

    let html = format!(
        r#"<iframe src="{}" title="{}" class="diaryx-island" sandbox="allow-scripts" loading="lazy" style="width:100%;min-height:{}px;border:none;"></iframe>"#,
        html_escape(path),
        html_escape(alt),
        min_height,
    );

    Some((html, total_consumed))
}

/// Read an island's attribute block. `None` for anything but `height=<integer>`,
/// which unmatches the embed rather than silently dropping the attribute.
fn parse_island_height(attributes: &str) -> Option<u32> {
    let value = attributes.trim().strip_prefix("height")?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    let height: u32 = value.parse().ok()?;
    Some(height.clamp(ISLAND_MIN_HEIGHT, ISLAND_MAX_HEIGHT))
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Markdown case, which is what every existing test asserted before a
    /// body had a grammar to be in.
    fn preprocess(source: &str) -> String {
        preprocess_custom_syntax(source, ContentFormat::Markdown)
    }

    #[test]
    fn highlight_default_color() {
        let out = preprocess("a ==hi== b");
        assert_eq!(
            out,
            r#"a <mark data-highlight-color="yellow" class="highlight-mark highlight-yellow">hi</mark> b"#
        );
    }

    #[test]
    fn highlight_named_color() {
        let out = preprocess("=={red}danger==");
        assert!(out.contains(r#"data-highlight-color="red""#));
        assert!(out.contains("highlight-red"));
        assert!(out.contains(">danger<"));
    }

    #[test]
    fn highlight_invalid_color_is_left_alone() {
        let out = preprocess("=={mauve}x==");
        assert_eq!(out, "=={mauve}x==");
    }

    #[test]
    fn spoiler_basic() {
        let out = preprocess("||secret||");
        assert_eq!(
            out,
            r#"<span data-spoiler="" class="spoiler-mark spoiler-hidden">secret</span>"#
        );
    }

    #[test]
    fn html_embed_becomes_iframe() {
        let out = preprocess("![demo](island.html)");
        assert!(out.contains(r#"<iframe src="island.html""#));
        assert!(out.contains(r#"title="demo""#));
        assert!(out.contains(r#"class="diaryx-island""#));
    }

    /// The initial height an island opens at, before — or without — a
    /// measurement from the document inside it.
    #[test]
    fn html_embed_takes_an_authored_height() {
        let out = preprocess("![demo](island.html){height=520}");
        assert!(out.contains("min-height:520px"), "got {out}");
        assert!(
            !out.contains("{height=520}"),
            "the block is consumed: {out}"
        );
    }

    /// The same range the resize bridge clamps a measurement to, so an island
    /// cannot open at a size it would never be allowed to reach.
    #[test]
    fn an_authored_height_is_clamped_to_the_bridges_range() {
        assert!(preprocess("![d](i.html){height=10}").contains("min-height:200px"));
        assert!(preprocess("![d](i.html){height=99999}").contains("min-height:4000px"));
    }

    /// An attribute this version does not know leaves the embed unmatched, so
    /// the reader sees the syntax rather than an island silently missing the
    /// thing it was asked for.
    #[test]
    fn an_unknown_island_attribute_leaves_the_embed_alone() {
        let source = "![demo](island.html){wdith=400}";
        assert_eq!(preprocess(source), source);
        assert_eq!(
            preprocess("![demo](island.html){height=tall}"),
            "![demo](island.html){height=tall}"
        );
    }

    /// `\!` is an escape in every grammar this preprocessor runs for, so an
    /// escaped embed is text about an embed — a line of documentation, most
    /// likely — and turning it into an island was the scanner reading past the
    /// backslash it should have stopped at.
    #[test]
    fn an_escaped_embed_is_not_an_island() {
        let out = preprocess(r"Write \![alt](page.html) to embed one.");
        assert_eq!(out, r"Write \![alt](page.html) to embed one.");
        assert!(!render_body(&out, ContentFormat::Markdown).contains("<iframe"));

        // The same for the other openers, and an escaped backslash still shields
        // nothing but itself.
        assert_eq!(preprocess(r"\==not a highlight=="), r"\==not a highlight==");
        assert_eq!(preprocess(r"\||not a spoiler||"), r"\||not a spoiler||");
        assert!(preprocess(r"\\==yes==").contains("highlight-mark"));
    }

    #[test]
    fn inline_code_is_untouched() {
        let out = preprocess("`==not a highlight==`");
        assert_eq!(out, "`==not a highlight==`");
    }

    #[test]
    fn fenced_code_is_untouched() {
        let input = "```\n==no==\n||no||\n```";
        let out = preprocess(input);
        assert_eq!(out, input);
    }

    #[test]
    fn escapes_content() {
        let out = preprocess("==<b>&\"==");
        assert!(out.contains("&lt;b&gt;&amp;&quot;"));
    }

    #[test]
    fn markdown_renders_basics() {
        let html = render_body("# Title\n\n~~struck~~", ContentFormat::Markdown);
        assert!(html.contains("<h1>"));
        assert!(html.contains("<del>struck</del>"));
    }

    /// The whole comrak feature set this crate used to enable by hand
    /// (`strikethrough`, `table`, `autolink`, `tasklist`, `footnotes`,
    /// `unsafe`), asserted against twig so a regression in the engine that
    /// replaced it cannot land quietly.
    #[test]
    fn markdown_still_covers_what_comrak_was_configured_for() {
        let src = "~~struck~~\n\n\
                   | a | b |\n|---|---|\n| 1 | 2 |\n\n\
                   - [ ] todo\n- [x] done\n\n\
                   A note.[^1]\n\n[^1]: The note.\n\n\
                   <div class=\"raw\">passed through</div>\n\n\
                   https://example.test\n\n```rust\nlet x = 1;\n```\n";
        let html = render_body(src, ContentFormat::Markdown);
        assert!(html.contains("<del>struck</del>"), "strikethrough");
        assert!(
            html.contains("<table>") && html.contains("<th>a</th>"),
            "tables"
        );
        assert!(html.contains("type=\"checkbox\""), "tasklists");
        assert!(html.contains("checked"), "a checked tasklist item");
        assert!(html.contains("The note."), "footnote text");
        assert!(html.contains("<div class=\"raw\">"), "raw HTML passthrough");
        assert!(
            html.contains("<a href=\"https://example.test\""),
            "autolinks"
        );
        assert!(html.contains("language-rust"), "fenced code language");
    }

    /// Stage three, end to end and in all three grammars: the pre-processor
    /// keeps its hands off fenced code, twig tags it with the language, and the
    /// highlighter colours it — none of the three knowing about the others.
    #[cfg(feature = "syntax-highlighting")]
    #[test]
    fn fenced_code_is_highlighted_in_every_grammar() {
        for (format, src) in [
            (ContentFormat::Markdown, "```rust\nlet x = 1;\n```\n"),
            (ContentFormat::Djot, "```rust\nlet x = 1;\n```\n"),
            (
                ContentFormat::Html,
                "<pre><code class=\"language-rust\">let x = 1;\n</code></pre>\n",
            ),
        ] {
            let html = render_body(src, format);
            assert!(
                html.contains(crate::syntax::HIGHLIGHTED_CLASS),
                "{format:?} left it uncoloured: {html}"
            );
            assert!(html.contains("plates-storage"), "{format:?}: {html}");
        }
    }

    /// The escaping twig applied has to survive the round trip, or a code block
    /// starts publishing tags instead of showing them.
    #[cfg(feature = "syntax-highlighting")]
    #[test]
    fn highlighting_does_not_unescape_the_page() {
        let html = render_body(
            "```rust\nlet s = \"<b>&amp;</b>\";\n```\n",
            ContentFormat::Markdown,
        );
        assert!(!html.contains("<b>"), "a tag reached the page: {html}");
        assert!(html.contains("&lt;b&gt;"), "still escaped: {html}");
    }

    /// A grammar the site supplied itself, reaching the page through the one
    /// call that takes one.
    #[cfg(feature = "syntax-highlighting")]
    #[test]
    fn a_site_grammar_reaches_a_rendered_body() {
        let syntaxes = crate::syntax::Syntaxes::with_custom([(
            "wat.sublime-syntax",
            "name: Wat\nfile_extensions: [wat]\nscope: source.wat\ncontexts:\n  main:\n    - match: ';;.*$'\n      scope: comment.line.wat\n",
        )]);
        let html = render_body_with(
            "```wat\n;; a note\n```\n",
            ContentFormat::Markdown,
            &syntaxes,
        );
        assert!(html.contains("plates-comment"), "{html}");
    }

    #[test]
    fn markdown_passes_preprocessed_raw_html_through() {
        let html = render_body("==hi==", ContentFormat::Markdown);
        assert!(html.contains("<mark"), "got {html}");
    }

    /// Djot escapes a bare tag, so the same custom syntax has to arrive as an
    /// inline raw span. This is the assertion that the Djot path is not just
    /// the Markdown path with a different parser.
    #[test]
    fn djot_custom_syntax_survives_as_raw_html() {
        let html = render_body("a ==hi== and ||shh|| b", ContentFormat::Djot);
        assert!(
            html.contains("<mark"),
            "highlight reached the output: {html}"
        );
        assert!(html.contains("data-spoiler"), "spoiler too: {html}");
        assert!(!html.contains("&lt;mark"), "and was not escaped: {html}");
    }

    #[test]
    fn djot_renders_its_own_grammar() {
        let html = render_body("_emph_ and {=native=}\n", ContentFormat::Djot);
        assert!(html.contains("<em>emph</em>"));
        assert!(html.contains("<mark>native</mark>"));
    }

    /// A fragment carrying a backtick would close a one-backtick raw span early.
    #[test]
    fn djot_raw_span_outruns_backticks_in_the_content() {
        let out = preprocess_custom_syntax("==a ` b==", ContentFormat::Djot);
        assert!(out.starts_with("``"), "fence outgrew the content: {out}");
        assert!(out.ends_with("{=html}"), "and is a raw span: {out}");
        let html = render_body("==a ` b==", ContentFormat::Djot);
        assert!(html.contains("<mark"), "still a highlight: {html}");
    }

    #[test]
    fn html_bodies_are_left_alone() {
        // `==` and `||` are literal text in an HTML body, not Diaryx syntax.
        let src = "<p>a == b || c</p>";
        assert_eq!(preprocess_custom_syntax(src, ContentFormat::Html), src);
        let html = render_body(src, ContentFormat::Html);
        assert!(html.contains("a == b || c"), "got {html}");
        assert!(!html.contains("<mark"));
    }
}
