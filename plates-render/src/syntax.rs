//! Syntax highlighting for fenced code blocks, as a pass over rendered HTML.
//!
//! This is the third stage of [`crate::body`], after the custom-syntax
//! pre-processor and twig itself. It runs on twig's *output* rather than on its
//! AST because [`prov::render_html`] is a string-to-string call with no node
//! hook to hang this off — and because doing it here means one pass covers
//! Markdown, Djot *and* hand-written HTML bodies, which an AST walk over one
//! grammar's tree would not.
//!
//! # Classes, not inline styles
//!
//! syntect can emit `style="color:#…"` on every span. It is not used, because
//! the colour would then be decided here, at render time, for a stylesheet that
//! has both a light and a dark palette (`html_format_css.css` defines the pair
//! and swaps it under `prefers-color-scheme`). A page highlighted inline would
//! keep one theme's colours in the other mode.
//!
//! So the output is *classed* — [`CLASS_PREFIX`] on every scope atom — and the
//! colours live in the stylesheet with the rest of the site's. A consequence
//! worth knowing: giving a language its own colours never means touching this
//! module, only adding a selector.
//!
//! # Where the grammars come from
//!
//! [`Syntaxes::bundled`] is `two-face`'s set, which is bat's: 213 grammars,
//! including the Zig, Swift, TOML and TypeScript that syntect's own 75 lack.
//!
//! [`Syntaxes::with_custom`] adds more, and takes each as the **text** of a
//! `.sublime-syntax` file rather than a path — this crate opens nothing, so a
//! caller with a grammar on disk reads it and passes the bytes in, exactly as
//! it already does for a shell template or a stylesheet.

use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxDefinition, SyntaxSet};
use syntect::util::LinesWithEndings;

/// The prefix on every class this module emits.
///
/// syntect names a class per atom of the scope it matched, so
/// `keyword.control.rust` becomes `plates-keyword plates-control plates-rust`.
/// Styling the first atom is usually what a stylesheet wants; the rest are
/// there for a site that wants to be more specific than that.
pub const CLASS_PREFIX: &str = "plates-";

/// Added to the `<code>` of a block this module actually highlighted.
///
/// A block whose language is not one the [`Syntaxes`] knows is left exactly as
/// twig wrote it, so this class — not the `language-…` one, which twig writes
/// either way — is what a stylesheet should scope a highlighting palette to.
pub const HIGHLIGHTED_CLASS: &str = "plates-highlighted";

const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed {
    prefix: CLASS_PREFIX,
};

/// The grammars one render has available.
///
/// Built once and shared across a site's pages: assembling a [`SyntaxSet`] is
/// far more expensive than using one, and a site that rebuilt it per page would
/// pay that for every document it publishes.
pub struct Syntaxes {
    set: SyntaxSet,
    warnings: Vec<String>,
}

impl Syntaxes {
    /// The built-in grammars, assembled once per process.
    ///
    /// Returns a `&'static` because there is no reason for two of them and the
    /// dump costs about a megabyte of resident memory to unpack.
    pub fn bundled() -> &'static Self {
        static BUNDLED: OnceLock<Syntaxes> = OnceLock::new();
        BUNDLED.get_or_init(|| Syntaxes {
            set: two_face::syntax::extra_newlines(),
            warnings: Vec::new(),
        })
    }

    /// The built-in grammars plus a caller's own.
    ///
    /// Each item is `(label, text)`: the text of a `.sublime-syntax` file, and
    /// a label used only to say which one failed. The label is ordinarily the
    /// path the declaration named, since that is what whoever reads the warning
    /// has to go and fix.
    ///
    /// A grammar that will not parse is **reported and skipped**, never fatal —
    /// the same bargain a broken shell template gets, for the same reason: a
    /// site should lose some colour over a bad file, not its publication. What
    /// went wrong is on [`warnings`](Self::warnings).
    pub fn with_custom<'a, I>(definitions: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut builder = two_face::syntax::extra_newlines().into_builder();
        let mut warnings = Vec::new();
        for (label, text) in definitions {
            // `true` is `lines_include_newline`, and has to match the set being
            // extended — `extra_newlines` — or a custom grammar would match
            // subtly differently from a built-in one at every end of line.
            match SyntaxDefinition::load_from_str(text, true, Some(label)) {
                Ok(parsed) => {
                    builder.add(parsed);
                }
                Err(e) => warnings.push(format!(
                    "syntax definition {label:?} could not be parsed ({e}) — code in that \
                     language is published unhighlighted"
                )),
            }
        }
        Syntaxes {
            set: builder.build(),
            warnings,
        }
    }

    /// Custom grammars that could not be parsed, as messages for whoever wrote
    /// the declaration that named them. Always empty for [`bundled`](Self::bundled).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Whether a fence tag resolves to a grammar: `"rust"`, `"rs"` and `"zig"`
    /// do, `""` and `"no-such-language"` do not.
    pub fn knows(&self, token: &str) -> bool {
        self.set.find_syntax_by_token(token).is_some()
    }
}

/// The opening of a fenced block twig has rendered with a language.
///
/// A fence with no language becomes a bare `<pre><code>`, which this never
/// matches — there is nothing to highlight it as.
const OPEN: &str = "<pre><code class=\"language-";
const CLOSE: &str = "</code></pre>";

/// Highlight every fenced code block in `html` whose language `syntaxes` knows.
///
/// Blocks it does not know are returned byte for byte, so a fence tagged with
/// something that is not a language — or with one no grammar covers — publishes
/// exactly as it did before this module existed.
pub fn highlight_code_blocks(html: &str, syntaxes: &Syntaxes) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        // From here `rest` starts at the block, so breaking out of the loop
        // publishes it — and everything after it — untouched.
        rest = &rest[at..];
        let Some(block) = split_block(rest) else {
            break;
        };

        match highlight_one(block.class_value, block.inner, syntaxes) {
            Some(spans) => out.push_str(&format!(
                "<pre><code class=\"language-{} {HIGHLIGHTED_CLASS}\"{}>{spans}</code></pre>",
                block.class_value, block.extras
            )),
            // Untouched, the open tag's bytes included: a block this pass
            // declines is a block that renders as it always did.
            None => out.push_str(&rest[..block.len]),
        }
        rest = &rest[block.len..];
    }

    out.push_str(rest);
    out
}

/// One `<pre><code class="language-…">…</code></pre>`, taken apart.
struct Block<'a> {
    /// The class attribute's value with `language-` stripped: the fence's tag,
    /// plus any further classes the author wrote beside it.
    class_value: &'a str,
    /// Anything between the class attribute and the `>` — another attribute a
    /// hand-written HTML body put there, and this pass's job to hand back.
    extras: &'a str,
    /// The block's content, still escaped as twig wrote it.
    inner: &'a str,
    /// How many bytes of the input the whole block occupies.
    len: usize,
}

/// Split a block that begins at `s`, or `None` if it is not one after all.
fn split_block(s: &str) -> Option<Block<'_>> {
    let after_open = s.strip_prefix(OPEN)?;
    let quote = after_open.find('"')?;
    let class_value = &after_open[..quote];
    let after_attr = &after_open[quote + 1..];
    let gt = after_attr.find('>')?;
    let extras = &after_attr[..gt];
    let inner_and_rest = &after_attr[gt + 1..];
    let end = inner_and_rest.find(CLOSE)?;
    Some(Block {
        class_value,
        extras,
        inner: &inner_and_rest[..end],
        len: OPEN.len() + quote + 1 + gt + 1 + end + CLOSE.len(),
    })
}

/// Highlight one block's content, or decline it.
fn highlight_one(class_value: &str, inner: &str, syntaxes: &Syntaxes) -> Option<String> {
    // A `<` that survived twig's escaping is a real tag, which means this block
    // arrived already marked up — an HTML body that highlighted its own code,
    // most likely. Re-highlighting it would mean parsing its spans as source.
    if inner.contains('<') {
        return None;
    }
    // `class="language-rust extra"` is one class attribute holding two classes.
    // The grammar is looked up by the first; the rest are the author's and are
    // written back untouched.
    let token = class_value.split_whitespace().next()?;
    let syntax = syntaxes.set.find_syntax_by_token(token)?;

    let source = unescape(inner);
    let mut hl = ClassedHTMLGenerator::new_with_class_style(syntax, &syntaxes.set, CLASS_STYLE);
    for line in LinesWithEndings::from(&source) {
        // A grammar can fail mid-parse (a runaway backreference in a custom
        // one, say). Decline the block rather than publish it half-coloured.
        hl.parse_html_for_line_which_includes_newline(line).ok()?;
    }
    Some(hl.finalize())
}

/// Undo the escaping twig applied to a code block's text, so the grammar sees
/// source rather than entities.
///
/// The five twig can emit, and no more: this reads what one known encoder
/// wrote, not arbitrary HTML, so a general entity table would be answering a
/// question nobody asked. `&amp;` is decoded like any other — the scan is
/// single-pass and left to right, so a literal `&amp;lt;` in the source, which
/// twig wrote as `&amp;amp;lt;`, comes back as `&amp;lt;` and not as `<`.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let decoded = [
            ("&lt;", '<'),
            ("&gt;", '>'),
            ("&quot;", '"'),
            ("&#39;", '\''),
            ("&apos;", '\''),
            ("&amp;", '&'),
        ]
        .into_iter()
        .find(|(entity, _)| tail.starts_with(entity));
        match decoded {
            Some((entity, ch)) => {
                out.push(ch);
                rest = &tail[entity.len()..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grammar for a language no public dump carries, as a site would supply
    /// it: text, not a path.
    const WAT: &str = "\
name: Wat
file_extensions: [wat]
scope: source.wat
contexts:
  main:
    - match: ';;.*$'
      scope: comment.line.wat
    - match: '\\b(module|func)\\b'
      scope: keyword.control.wat
";

    fn bundled(html: &str) -> String {
        highlight_code_blocks(html, Syntaxes::bundled())
    }

    #[test]
    fn colours_a_language_it_knows() {
        let out = bundled("<pre><code class=\"language-rust\">let x = 1;\n</code></pre>");
        assert!(out.contains(HIGHLIGHTED_CLASS), "marked as highlighted");
        assert!(out.contains("plates-storage"), "`let` is storage: {out}");
        assert!(
            out.contains("class=\"language-rust "),
            "kept the language class twig wrote: {out}"
        );
    }

    /// The four grammars this workspace needs that syntect's own bundle has
    /// none of. If `two-face` is ever dropped for the smaller set, this is
    /// where that decision surfaces.
    #[test]
    fn knows_the_languages_this_organisation_writes() {
        for token in ["zig", "swift", "toml", "typescript", "rust"] {
            assert!(Syntaxes::bundled().knows(token), "no grammar for {token}");
        }
    }

    /// A fence tagged with something that is not a language publishes exactly
    /// as it did before this module existed — not half-styled, not stripped.
    #[test]
    fn leaves_a_language_it_does_not_know_byte_for_byte() {
        let input = "<pre><code class=\"language-not-a-language\">plain\n</code></pre>";
        assert_eq!(bundled(input), input);
    }

    /// A fence with no language is a `<pre><code>` with no class, which this
    /// pass has no way — and no reason — to interpret.
    #[test]
    fn leaves_an_untagged_fence_alone() {
        let input = "<pre><code>plain\n</code></pre>";
        assert_eq!(bundled(input), input);
    }

    /// A body that arrived already marked up is not source, and parsing its
    /// spans as source is how highlighted HTML gets highlighted twice.
    #[test]
    fn declines_a_block_that_is_already_markup() {
        let input = "<pre><code class=\"language-rust\"><span>let</span> x\n</code></pre>";
        assert_eq!(bundled(input), input);
    }

    /// Whatever else the author put on the tag is theirs, and comes back.
    #[test]
    fn hands_back_attributes_it_did_not_write() {
        let out =
            bundled("<pre><code class=\"language-rust\" data-line=\"3\">let x = 1;\n</code></pre>");
        assert!(out.contains("data-line=\"3\""), "{out}");
    }

    /// Prose around the blocks, and more than one of them, survive the walk.
    #[test]
    fn carries_the_rest_of_the_page_through() {
        let out = bundled(
            "<p>before</p>\n<pre><code class=\"language-rust\">let x = 1;\n</code></pre>\n\
             <p>between</p>\n<pre><code class=\"language-toml\">k = 1\n</code></pre>\n<p>after</p>",
        );
        for marker in ["<p>before</p>", "<p>between</p>", "<p>after</p>"] {
            assert!(out.contains(marker), "lost {marker}: {out}");
        }
        assert_eq!(out.matches(HIGHLIGHTED_CLASS).count(), 2, "both blocks");
    }

    /// An open tag this pass cannot make sense of ends the walk without
    /// swallowing what it had already read — the failure mode a `break` in the
    /// middle of a scan invites.
    #[test]
    fn a_truncated_block_is_published_not_eaten() {
        let input = "<p>before</p><pre><code class=\"language-rust\">unterminated";
        assert_eq!(bundled(input), input);
    }

    #[test]
    fn unescapes_what_twig_escaped() {
        assert_eq!(
            unescape("a &lt;b&gt; &quot;c&quot; &amp; d"),
            "a <b> \"c\" & d"
        );
        assert_eq!(unescape("no entities"), "no entities");
        assert_eq!(unescape("bare & ampersand"), "bare & ampersand");
    }

    /// Single pass, left to right: the source text `&lt;` reaches twig as
    /// `&amp;lt;`, and has to come back as `&lt;` rather than as `<`.
    #[test]
    fn does_not_decode_an_entity_it_just_decoded() {
        assert_eq!(unescape("&amp;lt;"), "&lt;");
    }

    #[test]
    fn a_custom_grammar_colours_a_language_the_bundle_lacks() {
        assert!(
            !Syntaxes::bundled().knows("wat"),
            "premise: nothing bundled claims `wat`"
        );
        let syntaxes = Syntaxes::with_custom([("wat.sublime-syntax", WAT)]);
        assert!(syntaxes.warnings().is_empty(), "{:?}", syntaxes.warnings());

        let out = highlight_code_blocks(
            "<pre><code class=\"language-wat\">(module) ;; note\n</code></pre>",
            &syntaxes,
        );
        assert!(out.contains("plates-keyword"), "`module`: {out}");
        assert!(out.contains("plates-comment"), "`;;`: {out}");
    }

    /// A grammar that will not parse costs its language some colour, and
    /// nothing else — every other block still highlights.
    #[test]
    fn a_broken_grammar_is_reported_and_skipped() {
        let syntaxes = Syntaxes::with_custom([("broken.sublime-syntax", "this: is: not: one")]);
        assert_eq!(syntaxes.warnings().len(), 1);
        assert!(
            syntaxes.warnings()[0].contains("broken.sublime-syntax"),
            "names the file: {:?}",
            syntaxes.warnings()
        );
        let out = highlight_code_blocks(
            "<pre><code class=\"language-rust\">let x = 1;\n</code></pre>",
            &syntaxes,
        );
        assert!(out.contains(HIGHLIGHTED_CLASS), "rust still works: {out}");
    }
}
