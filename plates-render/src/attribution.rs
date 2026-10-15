//! Who wrote which *parts* of a document.
//!
//! prov's `generated.by` says who made a page. A page more than one hand has
//! written in — a person's draft with a model's paragraph in it, two people's
//! notes on one meeting — needs to say it of a passage, and this reads where a
//! body does.
//!
//! # A passage is a container
//!
//! Spelled as [`crate::visibility`] spells an audience region, and for the
//! same reason: twig parses all three grammars into one `container` node with
//! a name, attributes and children, so one predicate reads them all.
//!
//! | Grammar | Spelling |
//! |---|---|
//! | Markdown | `:::by{who="agent:claude-opus-5"}` … `:::` |
//! | Markdown (inline) | `:by[text]{who="Adam Harris"}` |
//! | Djot | `{.by who="agent:claude-opus-5"}` on the line above `:::` |
//! | HTML | `<div class="by" data-who="…">`, `<span class="by" data-who="…">` |
//!
//! **A passage is a container named `by`, or one whose classes contain `by`**,
//! and **its author is its `who` attribute** — `data-who` in HTML, where a
//! custom attribute has to be spelled so, and accepted in every grammar so
//! that a body converted from HTML keeps its authors. The value is a prov
//! actor: a bare name is a person, `agent:` a model, `process:` a program.
//!
//! Passages nest, and the innermost one is whose a word is
//! ([`author_at`]). A word in no passage is the page's own — `generated.by`,
//! or the library's owner — which is the caller's to know, not this module's.
//!
//! # A claim, not a proof
//!
//! A passage says who wrote it; anyone who can edit the file can write any
//! `who`. Whose a revision is, is the history's to say. What this reads is
//! what the text claims, for showing and for checking against that history.
//!
//! # Not a filter
//!
//! Unlike [`crate::visibility`], nothing here decides what leaves, so nothing
//! here fails closed. A passage that names no author is still an error,
//! because the marker plainly meant to name one and a reader that skipped it
//! would show the passage as the page's own.

use std::ops::Range;

use prov::twig::{self, Editor};
use prov::{Actor, ContentFormat};

use crate::visibility::{attrs_aware_start, features, twig_format};

/// The class — and, in Markdown, the directive name — that marks a passage
/// as attributed.
pub const MARKER: &str = "by";

/// The attribute naming a passage's author.
pub const WHO: &str = "who";

/// [`WHO`] as HTML spells a custom attribute.
pub const DATA_WHO: &str = "data-who";

/// Why a body's passages could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The body could not be parsed.
    Parse(String),
    /// A `by` passage that names no author, or names an empty one.
    Unattributed {
        /// The source of the passage, for the message.
        found: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "body could not be parsed for attribution: {e}"),
            Self::Unattributed { found } => write!(
                f,
                "a `{MARKER}` passage names no author ({found}) — give it a `{WHO}` attribute"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// One attributed passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    /// Whose it says it is.
    pub who: Actor,
    /// The whole passage, marker included — and, for a Djot block, the
    /// attribute line above it, which is where its `who` is written.
    pub span: Range<usize>,
    /// Its interior: the words that are `who`'s.
    pub content: Option<Range<usize>>,
}

/// Cheap pre-check: might this body hold a passage?
///
/// Wider than the syntax, as [`crate::visibility::has_visibility_directives`]
/// is: a false positive costs one parse.
pub fn has_attribution(body: &str) -> bool {
    body.contains(MARKER)
}

/// Every attributed passage in `body`, in document order — outer before inner
/// where they nest.
pub fn passages(body: &str, format: ContentFormat) -> Result<Vec<Passage>, Error> {
    if !has_attribution(body) {
        return Ok(Vec::new());
    }
    let mut editor =
        Editor::new_with_features(body.as_bytes(), twig_format(format), features(format))
            .map_err(|e| Error::Parse(format!("{e:?}")))?;
    let nodes = editor.nodes().map_err(|e| Error::Parse(format!("{e:?}")))?;

    let mut found = Vec::new();
    for node in &nodes {
        if let Some(passage) = passage_of(node, body)? {
            found.push(passage);
        }
    }
    found.sort_by_key(|p| (p.span.start, std::cmp::Reverse(p.span.end)));
    Ok(found)
}

/// Whose the byte at `offset` is: the innermost passage holding it, or `None`
/// when it is in none and so the page's own.
///
/// Measured against the interior, so a marker's own bytes are no one's.
pub fn author_at(passages: &[Passage], offset: usize) -> Option<&Actor> {
    passages
        .iter()
        .filter(|p| p.content.as_ref().is_some_and(|c| c.contains(&offset)))
        .min_by_key(|p| p.span.len())
        .map(|p| &p.who)
}

/// Read a container as a passage, or `None` when it is some other container.
fn passage_of(node: &twig::FlatNode, source: &str) -> Result<Option<Passage>, Error> {
    if !matches!(node.kind, twig::Kind::Container) {
        return Ok(None);
    }
    let attr = |key: &str| {
        node.attrs
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_deref())
    };
    let named = node.name.as_deref() == Some(MARKER);
    let classed = attr("class").is_some_and(|v| v.split_whitespace().any(|c| c == MARKER));
    if !named && !classed {
        return Ok(None);
    }

    let span = attrs_aware_start(source, node.span.start)..node.span.end;
    let who = attr(WHO)
        .or_else(|| attr(DATA_WHO))
        .map(str::trim)
        .filter(|w| !w.is_empty());
    let Some(who) = who else {
        return Err(Error::Unattributed {
            found: source.get(span.clone()).unwrap_or("?").replace('\n', "\\n"),
        });
    };

    Ok(Some(Passage {
        who: Actor::parse(who),
        span,
        content: node.content_span.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The authors of `body`'s passages, and each one's interior, trimmed.
    fn read(body: &str, format: ContentFormat) -> Vec<(String, String)> {
        passages(body, format)
            .unwrap()
            .into_iter()
            .map(|p| {
                let words = p.content.map_or("", |c| &body[c]).trim().to_string();
                (p.who.to_string(), words)
            })
            .collect()
    }

    fn pairs(want: &[(&str, &str)]) -> Vec<(String, String)> {
        want.iter()
            .map(|(w, t)| (w.to_string(), t.to_string()))
            .collect()
    }

    #[test]
    fn markdown_block_passages_name_their_authors() {
        let body = "Mine.\n\n:::by{who=\"agent:claude-opus-5\"}\nThe model's.\n:::\n\n\
                    :::by{who=\"Sam Lee\"}\nSam's.\n:::\n";
        assert_eq!(
            read(body, ContentFormat::Markdown),
            pairs(&[
                ("agent:claude-opus-5", "The model's."),
                ("Sam Lee", "Sam's.")
            ])
        );
    }

    #[test]
    fn markdown_inline_passages_name_their_authors() {
        let body = "I wrote this, :by[and Sam this]{who=\"Sam Lee\"}.\n";
        assert_eq!(
            read(body, ContentFormat::Markdown),
            pairs(&[("Sam Lee", "and Sam this")])
        );
    }

    /// Djot writes a block's attributes above it; the passage's span takes
    /// that line in, since it is where the author is named.
    #[test]
    fn djot_passages_name_their_authors() {
        let body = "Mine.\n\n{.by who=\"agent:claude-opus-5\"}\n:::\nThe model's.\n:::\n";
        let found = passages(body, ContentFormat::Djot).unwrap();
        assert_eq!(
            read(body, ContentFormat::Djot),
            pairs(&[("agent:claude-opus-5", "The model's.")])
        );
        assert!(
            body[found[0].span.clone()].starts_with("{.by"),
            "{:?}",
            &body[found[0].span.clone()]
        );
    }

    #[test]
    fn html_passages_name_their_authors() {
        let body =
            "<p>Mine.</p>\n<div class=\"by\" data-who=\"agent:claude-opus-5\">The model's.</div>\n";
        assert_eq!(
            read(body, ContentFormat::Html),
            pairs(&[("agent:claude-opus-5", "The model's.")])
        );
    }

    #[test]
    fn inline_passages_in_djot_and_html_name_their_authors() {
        let djot = "I wrote this, [and Sam this]{.by who=\"Sam Lee\"}.\n";
        assert_eq!(
            read(djot, ContentFormat::Djot),
            pairs(&[("Sam Lee", "and Sam this")])
        );
        let html =
            "<p>I wrote this, <span class=\"by\" data-who=\"Sam Lee\">and Sam this</span>.</p>\n";
        assert_eq!(
            read(html, ContentFormat::Html),
            pairs(&[("Sam Lee", "and Sam this")])
        );
    }

    #[test]
    fn an_author_is_read_as_a_prov_actor() {
        let body = ":::by{who=\"agent:claude-opus-5\"}\nA\n:::\n\n:::by{who=\"process:ocr\"}\nB\n:::\n\n\
                    :::by{who=\"Sam Lee\"}\nC\n:::\n";
        let who: Vec<Actor> = passages(body, ContentFormat::Markdown)
            .unwrap()
            .into_iter()
            .map(|p| p.who)
            .collect();
        assert_eq!(
            who,
            [
                Actor::Agent("claude-opus-5".into()),
                Actor::Process("ocr".into()),
                Actor::Person("Sam Lee".into()),
            ]
        );
    }

    /// A model's sentence inside a person's paragraph is the model's; the
    /// rest of the paragraph is still the person's.
    #[test]
    fn the_innermost_passage_is_whose_a_word_is() {
        let body = ":::by{who=\"Sam Lee\"}\nSam starts, :by[the model adds]{who=\"agent:claude-opus-5\"}, Sam ends.\n:::\n";
        let found = passages(body, ContentFormat::Markdown).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
        let at = |word: &str| author_at(&found, body.find(word).unwrap()).map(|a| a.to_string());
        assert_eq!(at("starts").as_deref(), Some("Sam Lee"));
        assert_eq!(at("adds").as_deref(), Some("agent:claude-opus-5"));
        assert_eq!(at("ends").as_deref(), Some("Sam Lee"));
    }

    #[test]
    fn a_word_in_no_passage_is_no_ones() {
        let body = "Mine.\n\n:::by{who=\"Sam Lee\"}\nSam's.\n:::\n";
        let found = passages(body, ContentFormat::Markdown).unwrap();
        assert_eq!(author_at(&found, body.find("Mine").unwrap()), None);
    }

    #[test]
    fn a_passage_with_no_author_is_refused() {
        for (body, format) in [
            (":::by\nWhose?\n:::\n", ContentFormat::Markdown),
            (":::by{who=\"\"}\nWhose?\n:::\n", ContentFormat::Markdown),
            ("{.by}\n:::\nWhose?\n:::\n", ContentFormat::Djot),
            ("<div class=\"by\">Whose?</div>\n", ContentFormat::Html),
        ] {
            let err = passages(body, format).unwrap_err();
            assert!(
                matches!(err, Error::Unattributed { .. }),
                "{body:?}: {err:?}"
            );
        }
    }

    /// A document explaining the syntax quotes it; a quoted marker is prose.
    #[test]
    fn a_marker_inside_code_is_prose() {
        let body = "Write `:by[text]{who=\"Sam\"}` to attribute a sentence.\n";
        assert!(passages(body, ContentFormat::Markdown).unwrap().is_empty());
    }

    /// An audience region and a passage are both containers; each module
    /// reads only its own.
    #[test]
    fn an_audience_region_is_not_a_passage() {
        let body = ":::vis{.family}\nFamily only.\n:::\n\n<div class=\"note\">Aside.</div>\n";
        assert!(passages(body, ContentFormat::Markdown).unwrap().is_empty());
    }

    /// One container can be both: Sam's paragraph, for the family.
    #[test]
    fn a_passage_can_also_be_an_audience_region() {
        let body = "{.by .vis .family who=\"Sam Lee\"}\n:::\nSam's, for the family.\n:::\n";
        assert_eq!(
            read(body, ContentFormat::Djot),
            pairs(&[("Sam Lee", "Sam's, for the family.")])
        );
    }
}
