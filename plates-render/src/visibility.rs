//! Audience visibility filtering: which *parts* of a document leave.
//!
//! The gate ([`prov::exports`]) decides which documents a site publishes. This
//! decides which regions of one does, filtered against the same audience name,
//! so a body and the site holding it can never disagree about who a paragraph
//! is for.
//!
//! # A region is a container
//!
//! Every grammar spells a marked region as one node, and twig parses all three
//! into the same AST kind — a `container` with a name, a class list and
//! children:
//!
//! | Grammar | Spelling | `name` | classes |
//! |---|---|---|---|
//! | Markdown | `:::vis{.family}` … `:::` | `"vis"` | `family` |
//! | Markdown (inline) | `:vis[text]{.family}` | `"vis"` | `family` |
//! | Djot | `{.vis .family}` on the line above `:::` | `""` | `vis family` |
//! | HTML | `<div class="vis family">` | `"div"` | `vis family` |
//!
//! So the predicate is uniform and needs no per-grammar branch: **a region is a
//! container named `vis`, or one whose classes contain `vis`**, and **its
//! declared audiences are its classes, less `vis` itself**.
//! That is why a Djot body and a Markdown body filter through one function
//! rather than two that agree until one of them is fixed.
//!
//! # Why the parser and not a scanner
//!
//! This module used to scan text for `:::vis{…}` and `:vis[…]{…}` without
//! parsing it, which was grammar-blind on purpose — one scanner for three
//! grammars. It was also blind to everything else, and the cost was a
//! disclosure bug: a marker inside a code span
//! (`` `:::vis{.family}` ``, in a document *explaining* the syntax) was treated
//! as a real directive, and a real directive whose fence a list had indented
//! was not.
//!
//! twig parses the body it is going to render anyway, so the spans are
//! available for free and they are the spans the renderer will agree with.
//! prov re-exports twig for exactly this ([`prov::twig`]).
//!
//! # Fail-closed
//!
//! Filtering is a disclosure boundary, so every way this can fail ends with
//! *less* leaving rather than more. A body whose grammar cannot be parsed, a
//! region this cannot account for, a marker left standing after the walk — all
//! are [`Error`], never a body returned unfiltered. See [`Error`] for why the
//! residue check exists at all.

use prov::ContentFormat;
use prov::twig::{self, Editor, MarkdownExtensions};

/// The class — and, in Markdown, the directive name — that marks a region as
/// audience-scoped.
///
/// One word across all three grammars, because it is the *marker*, not a
/// grammar's spelling of one.
pub const MARKER: &str = "vis";

/// Why a body could not be filtered.
///
/// Every variant means the caller must **not** publish the body it passed in.
/// There is no "filtered as best we could" outcome on purpose: a partial filter
/// is indistinguishable from a complete one by inspection, and the thing it
/// silently keeps is the thing someone declared private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The body could not be parsed, so no region in it could be located.
    Parse(String),
    /// An edit twig refused. Structural, not authorial — a bug here rather than
    /// in the document.
    Edit(String),
    /// A marker survived the filter.
    ///
    /// The backstop for the failure this module is most likely to have: a
    /// region spelled in a way this module does not recognize is a region
    /// nothing removed, and its content is then published to everyone. Cheap to
    /// check, and it converts the worst outcome (a silent leak) into the
    /// ordinary one (a refused publish naming the document).
    Residue {
        /// What was still there, for the message.
        found: String,
    },
    /// A region the parser could not read as one.
    ///
    /// The signature of twig's one structural gap here: it does not nest
    /// *inline* directives, so the outer half of `:vis[a :vis[b]{.x} c]{.y}`
    /// parses as a bare `:vis` carrying neither attributes nor an interior,
    /// while `a`, `c` and the `{.y}` that scoped them stay outside it as prose.
    ///
    /// Unwrapping that marker would delete the word `:vis` and publish
    /// everything it was scoping — a leak with no marker left behind for
    /// [`Residue`](Self::Residue) to find, which is why it is caught here by
    /// shape instead. Nested *block* regions are unaffected and work.
    Malformed {
        /// The source of the region that could not be read.
        found: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "body could not be parsed for visibility filtering: {e}"),
            Self::Edit(e) => write!(f, "visibility filter could not edit the body: {e}"),
            Self::Malformed { found } => write!(
                f,
                "a `{MARKER}` region could not be read as one ({found}) — an inline region \
                 nested inside another inline region is not supported; use a block region \
                 (`:::{MARKER}`) for the outer one"
            ),
            Self::Residue { found } => write!(
                f,
                "a `{MARKER}` marker survived visibility filtering ({found}) — refusing to \
                 publish a body whose audience regions were not all resolved"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Who the filtered body is for.
#[derive(Debug, Clone, Copy)]
pub enum Audience<'a> {
    /// Every region survives; only the markers are removed.
    ///
    /// For rendering a document *as its author sees it* — an editor preview,
    /// a local build with no audience chosen. Never for a publish.
    All,
    /// A region survives when it declares at least one of these.
    ///
    /// An empty slice keeps nothing, which is the honest reading of "for no
    /// audience" and matches the gate: a document declaring nothing is visible
    /// to no one.
    Only(&'a [&'a str]),
}

impl Audience<'_> {
    /// Whether a region declaring `declared` survives.
    fn admits(&self, declared: &[String]) -> bool {
        match self {
            Self::All => true,
            // Case-insensitive and trimmed, which is what this has always been.
            // Deliberately *not* tightened to the gate's exact match in the same
            // change that moves the spelling to classes: two ways for a region
            // to vanish silently, landing together, is one migration nobody can
            // debug. `SitePlan::case_drift` is where that argument belongs.
            Self::Only(wanted) => declared.iter().any(|d| {
                let d = d.trim();
                wanted.iter().any(|w| d.eq_ignore_ascii_case(w.trim()))
            }),
        }
    }
}

/// Cheap pre-check: is it even worth parsing this body?
///
/// Text-level and deliberately over-eager — it answers "might there be a
/// region", and a false positive costs one parse. A false *negative* would skip
/// the filter on a body that needed it, so this must stay wider than the real
/// syntax, never narrower.
pub fn has_visibility_directives(body: &str) -> bool {
    body.contains(MARKER)
}

/// A region twig found, reduced to what filtering needs.
struct Region {
    /// The node's whole byte range.
    span: std::ops::Range<usize>,
    /// Its interior, when it has one — what survives an unwrap.
    content: Option<std::ops::Range<usize>>,
    /// The audiences it declares: its classes, less [`MARKER`].
    declared: Vec<String>,
}

/// Read a container as a visibility region, or `None` when it is some other
/// container (an HTML `<figure>`, a Markdown `:::note`).
///
/// The whole per-grammar difference lives here, and it amounts to two ways of
/// carrying one marker: Markdown puts it in the directive's *name*, Djot and
/// HTML in the element's *class list*. Everything downstream sees one shape.
fn region_of(node: &twig::FlatNode) -> Option<Region> {
    if !matches!(node.kind, twig::Kind::Container) {
        return None;
    }
    let classes: Vec<String> = node
        .attrs
        .iter()
        .find(|(k, _)| k == "class")
        .and_then(|(_, v)| v.as_deref())
        .map(|v| v.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();

    let named = node.name.as_deref() == Some(MARKER);
    let classed = classes.iter().any(|c| c == MARKER);
    if !named && !classed {
        return None;
    }

    Some(Region {
        span: node.span.clone(),
        content: node.content_span.clone(),
        declared: classes.into_iter().filter(|c| c != MARKER).collect(),
    })
}

/// Filter `body` to the regions `audience` may see.
///
/// The document's own grammar decides how a region is spelled and twig decides
/// where it is; this decides which survive. Returns the body with every region
/// either removed or unwrapped — never one with a marker still in it (see
/// [`Error::Residue`]).
pub fn filter_body(
    body: &str,
    format: ContentFormat,
    audience: Audience<'_>,
) -> Result<String, Error> {
    if !has_visibility_directives(body) {
        return Ok(body.to_string());
    }

    let mut editor = Editor::new_ext(body.as_bytes(), twig_format(format), extensions(format))
        .map_err(|e| Error::Parse(format!("{e:?}")))?;

    // One region per pass, re-reading the tree each time: twig reparses after
    // every edit, so every span but the one just used is stale. The alternative
    // — collecting all the spans and splicing them back-to-front — is wrong the
    // moment regions nest, because an outer region's end moves when its inner
    // one is rewritten.
    //
    // Bounded by the region count: each pass removes exactly one container,
    // either by deleting it or by replacing it with its own interior, and
    // neither puts a `vis` container back.
    loop {
        let nodes = editor.nodes().map_err(|e| Error::Parse(format!("{e:?}")))?;
        let Some(region) = nodes.iter().find_map(region_of) else {
            break;
        };

        // A region with neither an interior nor a declared audience is not a
        // region twig read correctly — see `Error::Malformed`. Refusing here is
        // what stops the unwrap below from stripping the marker and publishing
        // the text it was scoping.
        if region.content.is_none() && region.declared.is_empty() {
            let source = editor
                .source_str()
                .map_err(|e| Error::Edit(format!("{e:?}")))?;
            return Err(Error::Malformed {
                found: source
                    .get(region.span.clone())
                    .unwrap_or("?")
                    .replace('\n', "\\n"),
            });
        }

        let replacement = if audience.admits(&region.declared) {
            // Unwrap: the region's own text stays, its marker goes. Taken from
            // the *current* source rather than the original, which is a
            // different string after the first pass.
            match &region.content {
                Some(interior) => {
                    let source = editor
                        .source_str()
                        .map_err(|e| Error::Edit(format!("{e:?}")))?;
                    source
                        .get(interior.clone())
                        .ok_or_else(|| {
                            Error::Edit(format!("interior {interior:?} is not a char boundary"))
                        })?
                        .to_string()
                }
                // A container with no interior has nothing to keep.
                None => String::new(),
            }
        } else {
            String::new()
        };

        let start = attrs_aware_start(
            &editor
                .source_str()
                .map_err(|e| Error::Edit(format!("{e:?}")))?,
            region.span.start,
        );
        editor
            .edit_range(start, region.span.end, &replacement)
            .map_err(|e| Error::Edit(format!("{e:?}")))?;
    }

    let out = editor
        .source_str()
        .map_err(|e| Error::Edit(format!("{e:?}")))?;
    residue_check(&out, format)?;
    Ok(out)
}

/// Extend a region's start backwards over a standalone attribute line.
///
/// Djot writes a block's attributes on the line *above* it (`{.vis .family}`
/// then `:::`), and twig attaches them to the container while leaving them
/// outside its `span`. Deleting the span alone would leave the attribute line
/// behind as a paragraph — which publishes the *audience's name* to everyone,
/// a small disclosure in its own right and a visible artefact either way.
///
/// Only a line that is nothing but a `{…}` block is absorbed, so prose ending
/// in a brace is untouched.
fn attrs_aware_start(source: &str, span_start: usize) -> usize {
    let before = &source[..span_start];
    // Only a block region has a line above it. An inline one starts mid-line,
    // where the preceding text is prose and absorbing it would eat the sentence.
    let Some(trimmed) = before.strip_suffix('\n') else {
        return span_start;
    };
    let line_start = trimmed.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = trimmed[line_start..].trim();
    if line.starts_with('{') && line.ends_with('}') && line.len() > 1 {
        line_start
    } else {
        span_start
    }
}

/// Refuse a body that still carries a marker outside of code.
///
/// The walk above ends when no `vis` container is left, so a *parser*-level
/// check would only re-ask a question that already answered itself. What it
/// cannot answer is the case that matters: a body parsed under the wrong
/// grammar, or Markdown parsed without [`extensions`]'s directive opt-in, has
/// no containers to find and returns every region intact. Text is the only
/// evidence left, so text is what this reads.
///
/// Code is excluded, and that exclusion is the whole reason this needs prov: a
/// document *explaining* the syntax quotes it, and quoting is not declaring.
/// [`prov::code_spans`] is the same code-awareness prov's own link scan uses,
/// so a marker in a code span is prose here for exactly the reason it is prose
/// there.
fn residue_check(out: &str, format: ContentFormat) -> Result<(), Error> {
    let code = prov::code_spans(out, format).unwrap_or_default();
    let in_code = |at: usize| code.iter().any(|s| s.contains(&at));

    for spelling in [":::vis", "::vis", ":vis["] {
        let mut from = 0;
        while let Some(rel) = out[from..].find(spelling) {
            let at = from + rel;
            if !in_code(at) {
                let end = out[at..]
                    .char_indices()
                    .nth(40)
                    .map_or(out.len(), |(i, _)| at + i);
                return Err(Error::Residue {
                    found: out[at..end].replace('\n', "\\n"),
                });
            }
            from = at + spelling.len();
        }
    }
    Ok(())
}

/// twig's name for a prov content format.
///
/// Spelled here because prov's own mapping is private, and prov is right to
/// keep it so: it converts for its two FFI calls, not as a public claim about
/// which twig format a `ContentFormat` *is*.
fn twig_format(format: ContentFormat) -> twig::Format {
    match format {
        ContentFormat::Markdown => twig::Format::Markdown,
        ContentFormat::Djot => twig::Format::Djot,
        ContentFormat::Html => twig::Format::Html,
    }
}

/// The parse extensions a grammar needs to see a region at all.
///
/// Markdown's generic directives are opt-in, and **the opt-in is
/// load-bearing**: without it `:::vis{.family}` is a paragraph of literal text,
/// no container matches, and the region publishes intact. Djot and HTML spell a
/// region with syntax they always parse, so they need nothing.
fn extensions(format: ContentFormat) -> MarkdownExtensions {
    MarkdownExtensions {
        directives: matches!(format, ContentFormat::Markdown),
        ..MarkdownExtensions::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only<'a>(a: &'a [&'a str]) -> Audience<'a> {
        Audience::Only(a)
    }

    #[test]
    fn markdown_keeps_the_matching_region_and_drops_the_rest() {
        let body = ":::vis{.public}\nSeen\n:::\n\n:::vis{.family}\nHidden\n:::\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert!(out.contains("Seen"), "{out:?}");
        assert!(!out.contains("Hidden"), "{out:?}");
    }

    #[test]
    fn a_region_declaring_several_audiences_matches_any_of_them() {
        let body = ":::vis{.family .friends}\nBoth\n:::\n";
        for who in ["family", "friends"] {
            let out = filter_body(body, ContentFormat::Markdown, only(&[who])).unwrap();
            assert!(out.contains("Both"), "{who}: {out:?}");
        }
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert!(!out.contains("Both"), "{out:?}");
    }

    #[test]
    fn inline_regions_filter_too() {
        let body = "a :vis[keep]{.public} b :vis[drop]{.family} c\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert!(out.contains("keep"), "{out:?}");
        assert!(!out.contains("drop"), "{out:?}");
    }

    /// The bug the text scanner had, and the reason this module parses. A
    /// document *about* the syntax quotes it; a quoted marker is prose.
    #[test]
    fn a_marker_inside_a_code_span_is_prose() {
        let body = "Write `:::vis{.family}` to scope a region.\n\n:::vis{.family}\nHidden\n:::\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert!(out.contains("Write `:::vis{.family}`"), "{out:?}");
        assert!(!out.contains("Hidden"), "{out:?}");
    }

    #[test]
    fn nested_regions_resolve_from_the_inside_out() {
        let body = ":::: vis{.public}\nouter\n\n:::vis{.family}\ninner\n:::\n::::\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert!(out.contains("outer"), "{out:?}");
        assert!(!out.contains("inner"), "{out:?}");
    }

    #[test]
    fn html_regions_filter_by_class() {
        let body = "<div class=\"vis public\">Seen</div>\n<div class=\"vis family\">Hidden</div>\n";
        let out = filter_body(body, ContentFormat::Html, only(&["public"])).unwrap();
        assert!(out.contains("Seen"), "{out:?}");
        assert!(!out.contains("Hidden"), "{out:?}");
    }

    /// Djot carries a block's attributes on the line above it. Removing the
    /// container without them would leave `{.vis .family}` standing — the
    /// audience's name, published.
    #[test]
    fn djot_regions_take_their_attribute_line_with_them() {
        let body = "{.vis .family}\n:::\nHidden\n:::\n";
        let out = filter_body(body, ContentFormat::Djot, only(&["public"])).unwrap();
        assert!(!out.contains("Hidden"), "{out:?}");
        assert!(!out.contains("family"), "the audience name leaked: {out:?}");
    }

    #[test]
    fn all_keeps_every_region_and_removes_every_marker() {
        let body = ":::vis{.public}\nA\n:::\n\n:::vis{.family}\nB\n:::\n";
        let out = filter_body(body, ContentFormat::Markdown, Audience::All).unwrap();
        assert!(out.contains('A') && out.contains('B'), "{out:?}");
        assert!(!out.contains("vis"), "{out:?}");
    }

    /// No audience is not "every audience". A document declaring nothing is
    /// visible to no one, and a region is judged the same way.
    #[test]
    fn an_empty_audience_list_keeps_nothing() {
        let body = ":::vis{.public}\nA\n:::\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&[])).unwrap();
        assert!(!out.contains('A'), "{out:?}");
    }

    /// A body with no marker never reaches the parser.
    #[test]
    fn a_body_with_no_regions_is_returned_unchanged() {
        let body = "# Title\n\nJust prose.\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert_eq!(out, body);
    }

    /// The migration's sharp edge, and the reason it is safe. The old bare-key
    /// spelling declares no *class*, so it matches no audience and its content
    /// is dropped — content vanishes rather than leaking. The marker still goes,
    /// so this does not trip the residue check.
    #[test]
    fn the_old_bare_key_spelling_drops_rather_than_leaks() {
        let body = ":::vis{public}\nHidden\n:::\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert!(
            !out.contains("Hidden"),
            "old spelling must not publish: {out:?}"
        );
    }

    /// twig does not nest inline directives, and the half-parsed result would
    /// otherwise publish the text it was scoping with only the marker removed.
    /// Refused by shape, since no marker survives for the residue check to find.
    #[test]
    fn a_nested_inline_region_is_refused_rather_than_half_filtered() {
        let body = "A :vis[secret :vis[inner]{.public} end]{.family}\n";
        let err = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap_err();
        assert!(matches!(err, Error::Malformed { .. }), "{err:?}");
    }

    /// The same nesting spelled with block regions is fine — the gap is inline
    /// directives only.
    #[test]
    fn nested_block_regions_are_not_affected_by_that_gap() {
        let body = ":::: vis{.public}\nouter\n\n:::vis{.family}\ninner\n:::\n::::\n";
        let out = filter_body(body, ContentFormat::Markdown, only(&["public"])).unwrap();
        assert!(out.contains("outer") && !out.contains("inner"), "{out:?}");
    }

    /// The backstop, exercised directly: a marker the walk did not account for
    /// refuses the body instead of publishing it.
    #[test]
    fn a_surviving_marker_is_refused() {
        let err = residue_check(
            "text\n:::vis{.family}\nHidden\n:::\n",
            ContentFormat::Markdown,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Residue { .. }), "{err:?}");
    }
}
