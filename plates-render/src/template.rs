//! Body templating: block structure as twig directives, values as AST nodes.
//!
//! A template is an ordinary document. Its control flow is spelled with the
//! generic directives twig already parses and an editor already edits —
//! `:::each`, `:::if`, `:::group` — and its values with the text directive
//! `:val[path]`. Nothing here is a plates dialect: the directive family is
//! `micromark-extension-directive`'s, which is the same family
//! [`crate::visibility`] finds `:::vis` in and the same one the editor over
//! this vault reparses after every keystroke.
//!
//! # Why not a template engine
//!
//! This module used to run Handlebars over the body text. Two things were
//! wrong with that, and both are the same thing:
//!
//! - **It was invisible to the AST.** `{{#each}}` is, to twig and to any editor
//!   over it, one undifferentiated paragraph. A format whose reason to exist is
//!   that a template stays editable cannot spell its structure in text the
//!   editor cannot see.
//! - **It was blind to code.** `render_template` was handed the whole body, so
//!   a `{{title}}` inside a fenced block was substituted — in a document
//!   *explaining* the syntax, which is the only kind of document that writes
//!   one. That is the disclosure bug [`crate::visibility`] was rewritten to
//!   delete, arriving back through a second door.
//!
//! A directive has neither problem, because it is a node: it does not exist
//! inside a code span, and it is addressable by every operation twig has.
//!
//! # Why `{{ }}` survives in link destinations
//!
//! One position in Markdown cannot hold a node. A link's destination is not
//! inline-parsed — twig stores it as a byte run (`Link.destination`) and
//! carries a positional escape alphabet for it (`Syntax.link_dest_escapes`),
//! which is a settled decision rather than a gap. So `[:val[t]](:val[href])`
//! cannot work, and a list of links is the single most common thing a template
//! produces.
//!
//! `{{path}}` therefore survives **in a link or image destination and nowhere
//! else**, and — this is the part that matters — it is resolved by reading
//! `destination` off the AST node, never by scanning text. A `{{` in a code
//! block is the contents of a `code_block`, not a `link`, so the substitution
//! cannot reach it. The escape hatch stays AST-driven, which was the point of
//! the format.
//!
//! A `{{ }}` written anywhere else is not a template. It publishes as itself
//! and is reported as a warning, which is the migration this change asks for:
//! loud and self-diagnosing, never a silent fallback.
//!
//! # Markdown only
//!
//! The vocabulary is Markdown's directive extension. A Djot or HTML body is
//! passed through untouched — its `:::` divs and `<div>`s are content, and
//! giving them control-flow meaning would make every existing one a template by
//! accident.

use std::path::Path;

use indexmap::IndexMap;
use prov::twig::{self, Editor, MarkdownExtensions};
use prov::{ContentFormat, Value as YamlValue};
use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::visibility;

/// The text directive that inserts a value: `:val[page.title]`.
pub const VAL: &str = "val";
/// The container directive that repeats its body: `:::each{of=… as=…}`.
pub const EACH: &str = "each";
/// The container directive that includes its body conditionally: `:::if{…}`.
pub const IF: &str = "if";
/// The container directive that repeats its body once per group.
pub const GROUP: &str = "group";

/// Every directive name this module claims, for the fast-path check and for
/// the message that lists them when one is misspelled.
const VOCABULARY: &[&str] = &[VAL, EACH, IF, GROUP];

/// How many directives one `expand` call will resolve before it decides it is
/// not converging.
///
/// The walk is bounded in principle — every pass removes one container and
/// nothing puts one back — but a *value* whose text happens to spell a
/// directive would put one back, and an author should get a refused publish
/// rather than a hung build. Generous enough that no real document reaches it.
const MAX_PASSES: u32 = 10_000;

/// How deep `:::each` bodies may nest.
///
/// Each level is a recursive parse, so this bounds the stack as well as the
/// author's patience. A template past four or five levels is a data model
/// asking to be flattened.
const MAX_DEPTH: u32 = 32;

/// Why a body could not be templated.
///
/// Unlike [`crate::visibility::Error`], none of these is a disclosure failure —
/// a template that will not expand publishes nothing it should not. They are
/// authorial errors, and the caller's job with one is to *say* it: a body that
/// silently publishes its own source is how a broken template survives a
/// release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The body could not be parsed, so no directive in it could be located.
    Parse(String),
    /// An edit twig refused. Structural, not authorial — a bug here rather than
    /// in the document.
    Edit(String),
    /// A directive this module claims, written in a way it cannot read.
    Directive {
        /// Which directive, by name.
        name: String,
        /// What is wrong with it, for whoever wrote it.
        message: String,
    },
    /// The walk did not converge: a value whose text spells a directive puts
    /// back what the pass before it removed.
    Runaway,
    /// Template blocks nested past the depth one recursive parse per level
    /// can carry.
    TooDeep,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "body could not be parsed for templating: {e}"),
            Self::Edit(e) => write!(f, "the template expander could not edit the body: {e}"),
            Self::Directive { name, message } => write!(f, "`:{name}` {message}"),
            Self::Runaway => write!(
                f,
                "template expansion did not converge after {MAX_PASSES} directives — \
                 a value whose text spells a directive will do this"
            ),
            Self::TooDeep => write!(
                f,
                "template blocks nest more than {MAX_DEPTH} deep — \
                 flatten the shape, or the data behind it"
            ),
        }
    }
}

impl std::error::Error for Error {}

// ── The context ─────────────────────────────────────────────────────────────

/// What every page of one site can name: the site itself, and its entries.
///
/// Built once per render from the sources `build_pages` receives, which are
/// already the gate-admitted set — so a template cannot reach a withheld
/// document, because the data was never assembled. That property is a
/// consequence of *where* this is built, not of a check, which is why it is
/// stated here and tested rather than enforced downstream.
#[derive(Debug, Clone, Default)]
pub struct SiteContext {
    values: JsonMap<String, JsonValue>,
}

impl SiteContext {
    /// Assemble the site-level half of the context.
    ///
    /// `entries` are in the site's own order — source order with `nav_order`
    /// overriding, which is the rule [`crate::nav`] sorts siblings by, so a
    /// template listing entries and a nav listing them agree.
    pub fn new(site: JsonValue, entries: Vec<JsonValue>, groups: Vec<JsonValue>) -> Self {
        let mut values = JsonMap::new();
        values.insert("site".into(), site);
        values.insert("entries".into(), JsonValue::Array(entries));
        values.insert("groups".into(), JsonValue::Array(groups));
        Self { values }
    }
}

/// The full scope one page's template resolves against.
///
/// Two layers rather than one map, and borrowed rather than cloned: the
/// site-level half holds every entry in the site, and copying that per page
/// would make a render quadratic in the size of the vault it publishes.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    site: &'a SiteContext,
    page: &'a JsonMap<String, JsonValue>,
}

impl<'a> Context<'a> {
    /// Pair the site's context with this page's own values.
    ///
    /// Page values shadow site values, which is what lets a page's frontmatter
    /// carry a `site:` key without shouting down the site's.
    pub fn new(site: &'a SiteContext, page: &'a JsonMap<String, JsonValue>) -> Self {
        Self { site, page }
    }

    fn root(&self, key: &str) -> Option<&JsonValue> {
        self.page.get(key).or_else(|| self.site.values.get(key))
    }
}

/// A context plus whatever `:::each` has bound on the way in.
///
/// Bindings shadow the context and each other, innermost last, so a nested
/// `each` reusing a name is the ordinary lexical thing rather than a collision.
struct Scope<'a> {
    context: Context<'a>,
    bindings: Vec<(&'a str, &'a JsonValue)>,
}

impl<'a> Scope<'a> {
    fn new(context: Context<'a>) -> Self {
        Self {
            context,
            bindings: Vec::new(),
        }
    }

    /// Resolve a dotted path, or `None` when nothing in scope carries it.
    ///
    /// A numeric segment indexes a sequence (`entries.0.title`). That is still
    /// a path lookup and not an expression — the line this format holds is at
    /// *computation*, not at addressing.
    fn get(&self, path: &str) -> Option<&JsonValue> {
        let mut parts = path.split('.').map(str::trim).filter(|p| !p.is_empty());
        let head = parts.next()?;
        let mut value = self
            .bindings
            .iter()
            .rev()
            .find(|(name, _)| *name == head)
            .map(|(_, v)| *v)
            .or_else(|| self.context.root(head))?;
        for part in parts {
            value = match value {
                JsonValue::Object(map) => map.get(part)?,
                JsonValue::Array(items) => items.get(part.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(value)
    }

    fn with<'s>(&'s self, name: &'a str, value: &'a JsonValue) -> Scope<'a>
    where
        'a: 's,
    {
        let mut bindings = self.bindings.clone();
        bindings.push((name, value));
        Scope {
            context: self.context,
            bindings,
        }
    }
}

/// The text a value inserts.
///
/// An absent path is the empty string rather than an error, so an optional
/// field — a page with no `date`, an entry with no `description` — is writable
/// without wrapping every mention in `:::if`. A path naming a *collection* is
/// an error, because there is no reading of "insert these forty entries here"
/// that an author meant.
fn text_of(value: Option<&JsonValue>, path: &str) -> Result<String, Error> {
    Ok(match value {
        None | Some(JsonValue::Null) => String::new(),
        Some(JsonValue::String(s)) => s.clone(),
        Some(JsonValue::Bool(b)) => b.to_string(),
        Some(JsonValue::Number(n)) => n.to_string(),
        Some(JsonValue::Array(_)) | Some(JsonValue::Object(_)) => {
            return Err(Error::Directive {
                name: VAL.into(),
                message: format!(
                    "names {path:?}, which is a collection — a value position holds text, \
                     so iterate it with `:::each{{of={path} as=…}}` instead"
                ),
            });
        }
    })
}

/// Whether a path holds something, in prov's sense of the word.
///
/// The same reading `prov_views`' `Condition::Has` takes: present means
/// *usable*, not merely written. An empty string, an empty list and a `false`
/// are all absent, because a template that included a region for one would be
/// showing a block it has nothing to put in.
fn is_present(value: Option<&JsonValue>) -> bool {
    match value {
        None | Some(JsonValue::Null) => false,
        Some(JsonValue::Bool(b)) => *b,
        Some(JsonValue::String(s)) => !s.trim().is_empty(),
        Some(JsonValue::Array(items)) => !items.is_empty(),
        Some(JsonValue::Object(map)) => !map.is_empty(),
        Some(JsonValue::Number(_)) => true,
    }
}

// ── The walk ────────────────────────────────────────────────────────────────

/// Cheap pre-check: is it even worth parsing this body?
///
/// Over-eager on purpose, like [`crate::visibility::has_visibility_directives`]
/// — a false positive costs one parse, a false negative skips a template that
/// needed expanding and publishes its own source.
pub fn has_templates(body: &str) -> bool {
    body.contains("{{")
        || VOCABULARY
            .iter()
            .any(|name| body.contains(&format!(":{name}")))
}

/// The parse extensions the vocabulary needs.
///
/// The opt-in is load-bearing in the same way it is for visibility: without it
/// `:::each{…}` is a paragraph of literal text, no container matches, and the
/// template publishes its own source.
fn extensions() -> MarkdownExtensions {
    MarkdownExtensions {
        directives: true,
        ..MarkdownExtensions::default()
    }
}

/// One directive the walk found, reduced to what expansion needs.
struct Found {
    name: String,
    span: std::ops::Range<usize>,
    content: Option<std::ops::Range<usize>>,
    attrs: Vec<(String, Option<String>)>,
}

/// Read a container as a template directive, or `None` for any other one.
///
/// Matched on the directive's *name*, and only when twig recorded the node as
/// a directive rather than a tag: an HTML body's `<each>` element agrees on
/// `kind` and on `name`, and giving it control flow would be a dialect nobody
/// asked for.
fn directive_of(node: &twig::FlatNode) -> Option<Found> {
    if !matches!(node.kind, twig::Kind::Container) {
        return None;
    }
    if matches!(node.origin, Some(twig::ContainerOrigin::Element)) {
        return None;
    }
    let name = node.name.as_deref()?;
    if !VOCABULARY.contains(&name) {
        return None;
    }
    Some(Found {
        name: name.to_string(),
        span: node.span.clone(),
        content: node.content_span.clone(),
        attrs: node.attrs.clone(),
    })
}

impl Found {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_deref())
    }

    fn require(&self, key: &str) -> Result<&str, Error> {
        self.attr(key)
            .filter(|v| !v.is_empty())
            .ok_or(Error::Directive {
                name: self.name.clone(),
                message: format!("needs a `{key}=` attribute naming what to read"),
            })
    }

    /// The directive's interior, as source text.
    fn interior<'s>(&self, source: &'s str) -> &'s str {
        self.content
            .clone()
            .and_then(|r| source.get(r))
            .unwrap_or("")
    }
}

/// Expand every template directive in `body`, and resolve `{{ }}` in the link
/// destinations that survive.
///
/// Markdown only — see the module docs. A body in any other grammar is returned
/// as it arrived.
pub fn expand(
    body: &str,
    format: ContentFormat,
    context: Context<'_>,
    warnings: &mut Vec<String>,
) -> Result<String, Error> {
    if !matches!(format, ContentFormat::Markdown) || !has_templates(body) {
        return Ok(body.to_string());
    }
    let scope = Scope::new(context);
    let mut passes = MAX_PASSES;
    let out = expand_in(body, &scope, 0, &mut passes)?;
    report_stray_braces(&out, warnings);
    Ok(out)
}

/// One scope's worth of expansion: directives first, then destinations.
///
/// Recursive rather than iterative, and the recursion is what carries a
/// binding: a `:::each` body is expanded *per item*, in a scope that has the
/// item bound, and only the finished text is spliced back. That is why an inner
/// `:val[entry.title]` can mean something the outer document has no name for.
fn expand_in(
    source: &str,
    scope: &Scope<'_>,
    depth: u32,
    passes: &mut u32,
) -> Result<String, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::TooDeep);
    }
    if !has_templates(source) {
        return Ok(source.to_string());
    }

    let mut editor = Editor::new_ext(source.as_bytes(), twig::Format::Markdown, extensions())
        .map_err(|e| Error::Parse(format!("{e:?}")))?;

    // One directive per pass, re-reading the tree each time: twig reparses
    // after every edit, so every span but the one just used is stale. The
    // alternative — collecting all the spans and splicing back-to-front — is
    // wrong the moment directives nest, because an outer one's end moves when
    // its inner one is rewritten. The same reasoning `visibility` records.
    loop {
        *passes = passes.checked_sub(1).ok_or(Error::Runaway)?;

        let nodes = editor.nodes().map_err(|e| Error::Parse(format!("{e:?}")))?;
        let Some(found) = nodes.iter().find_map(directive_of) else {
            break;
        };
        let current = editor
            .source_str()
            .map_err(|e| Error::Edit(format!("{e:?}")))?;

        let replacement = match found.name.as_str() {
            VAL => {
                let path = found.interior(&current).trim().to_string();
                if path.is_empty() {
                    return Err(Error::Directive {
                        name: VAL.into(),
                        message: "is empty — write the path it should insert, `:val[page.title]`"
                            .into(),
                    });
                }
                text_of(scope.get(&path), &path)?
            }
            EACH => repeat(&found, found.require("of")?, &current, scope, depth, passes)?,
            GROUP => repeat(&found, "groups", &current, scope, depth, passes)?,
            IF => {
                if holds(&found, scope)? {
                    as_block(expand_in(
                        found.interior(&current),
                        scope,
                        depth + 1,
                        passes,
                    )?)
                } else {
                    String::new()
                }
            }
            _ => unreachable!("directive_of only admits the vocabulary"),
        };

        editor
            .edit_range(found.span.start, found.span.end, &replacement)
            .map_err(|e| Error::Edit(format!("{e:?}")))?;
    }

    let expanded = editor
        .source_str()
        .map_err(|e| Error::Edit(format!("{e:?}")))?;
    resolve_destinations(&expanded, scope, passes)
}

/// Expand a directive's body once per item of a collection.
///
/// Shared by `:::each` and `:::group`, which differ only in where the
/// collection comes from — `:::group` reads the site's own grouping, so it
/// takes no `of=`. It deliberately takes no `by=` either: the arrangement the
/// site's view declares is what decides its groups, and a `by=` that disagreed
/// with it would be a second grouping nothing reconciles.
fn repeat(
    found: &Found,
    of: &str,
    source: &str,
    scope: &Scope<'_>,
    depth: u32,
    passes: &mut u32,
) -> Result<String, Error> {
    let binding = found.require("as")?.to_string();
    let items = match scope.get(of) {
        Some(JsonValue::Array(items)) => items.clone(),
        // Nothing to repeat over is an empty repetition, not an error: a site
        // with no grouped entries has no groups, and a template that named a
        // field this page has not got should render nothing rather than refuse
        // to publish.
        None | Some(JsonValue::Null) => Vec::new(),
        Some(_) => {
            return Err(Error::Directive {
                name: found.name.clone(),
                message: format!("reads {of:?}, which is a single value rather than a list"),
            });
        }
    };

    let interior = found.interior(source).to_string();
    let mut out = String::new();
    for item in &items {
        let inner = scope.with(&binding, item);
        out.push_str(&as_block(expand_in(&interior, &inner, depth + 1, passes)?));
    }
    Ok(out)
}

/// Terminate a block-level replacement with the newline its fence used to
/// supply.
///
/// A container's interior stops at the last character *before* its closing
/// fence, so two repetitions of `- :val[e.title]` spliced back to back become
/// one list item reading `- One- Two`. The fence was carrying the line ending;
/// with the fence gone, this carries it.
fn as_block(mut text: String) -> String {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Whether an `:::if` includes its body.
///
/// The vocabulary is prov's, deliberately: `has=` is `Condition::Has` and
/// `not=` is `Condition::Not(Has)`, read the same way (`is_present`). Several
/// attributes are an implicit **and**, which is prov's rule for a multi-key
/// `where:` block.
///
/// `equals`, `any-of` and `all-of` are **not** implemented. They need a value
/// position, and an attribute gives one key one value; designing that spelling
/// is the open question the format's proposal names, and guessing at it now
/// would settle it by accident.
fn holds(found: &Found, scope: &Scope<'_>) -> Result<bool, Error> {
    let mut asked = false;
    let mut result = true;
    for (key, value) in &found.attrs {
        let path = value.as_deref().unwrap_or("");
        match key.as_str() {
            "has" => {
                asked = true;
                result &= is_present(scope.get(path));
            }
            "not" => {
                asked = true;
                result &= !is_present(scope.get(path));
            }
            // `class` and `id` ride along on the attribute grammar and mean
            // nothing here; anything else is a misspelling worth naming.
            "class" | "id" => {}
            other => {
                return Err(Error::Directive {
                    name: IF.into(),
                    message: format!(
                        "does not know the condition {other:?} — it reads `has=` and `not=`"
                    ),
                });
            }
        }
    }
    if !asked {
        return Err(Error::Directive {
            name: IF.into(),
            message: "has no condition — write `has=page.date` or `not=page.draft`".into(),
        });
    }
    Ok(result)
}

// ── Link destinations ───────────────────────────────────────────────────────

/// Resolve `{{path}}` inside link and image destinations, and only there.
///
/// The node is what makes this safe. A destination is located as the stretch of
/// the link's span *after* its label — `content_span.end .. span.end`, which is
/// `](…)` however the destination is spelled, angle brackets and title
/// included — so a `{{` in the label, in a code span, or in a paragraph is
/// never in range. That is the whole difference between this and running a
/// template engine over the text.
fn resolve_destinations(
    source: &str,
    scope: &Scope<'_>,
    passes: &mut u32,
) -> Result<String, Error> {
    if !source.contains("{{") {
        return Ok(source.to_string());
    }
    let mut editor = Editor::new_ext(source.as_bytes(), twig::Format::Markdown, extensions())
        .map_err(|e| Error::Parse(format!("{e:?}")))?;

    loop {
        *passes = passes.checked_sub(1).ok_or(Error::Runaway)?;

        let nodes = editor.nodes().map_err(|e| Error::Parse(format!("{e:?}")))?;
        let current = editor
            .source_str()
            .map_err(|e| Error::Edit(format!("{e:?}")))?;

        let Some((at, end, path)) = nodes
            .iter()
            .filter(|n| matches!(n.kind, twig::Kind::Link | twig::Kind::Image))
            .filter(|n| n.destination.as_deref().is_some_and(|d| d.contains("{{")))
            .find_map(|n| brace_run(&current, n))
        else {
            break;
        };

        let value = text_of(scope.get(&path), &path)?;
        editor
            .edit_range(at, end, &value)
            .map_err(|e| Error::Edit(format!("{e:?}")))?;
    }

    editor
        .source_str()
        .map_err(|e| Error::Edit(format!("{e:?}")))
}

/// The first `{{…}}` in a link node's destination region, as `(start, end,
/// path)`.
///
/// The region begins where the label ends, so a link whose *text* contains
/// braces keeps them. A node with no recorded label falls back to its whole
/// span, which is the honest answer for an autolink-shaped node and costs
/// nothing, since one has no label to protect.
fn brace_run(source: &str, node: &twig::FlatNode) -> Option<(usize, usize, String)> {
    let from = node
        .content_span
        .as_ref()
        .map(|c| c.end)
        .unwrap_or(node.span.start);
    let region = source.get(from..node.span.end)?;
    let open = region.find("{{")?;
    let close = region[open..].find("}}")? + open;
    let path = region[open + 2..close].trim().to_string();
    Some((from + open, from + close + 2, path))
}

/// Report a `{{ }}` left standing outside a destination.
///
/// This is the migration off the Handlebars body, and it is a report rather
/// than a fallback for the reason the `vis` spelling migration is: the answer
/// to drift is to fix the document. The failure direction is safe here in a way
/// it is not there — an unmigrated `{{title}}` publishes as the literal text
/// `{{title}}`, which is visible on the page rather than silently absent — so
/// this warns and lets the site out, instead of refusing it.
///
/// Code is excluded, via the same [`prov::code_spans`] the visibility residue
/// check uses: a document explaining the syntax quotes it, and quoting is not
/// writing.
fn report_stray_braces(out: &str, warnings: &mut Vec<String>) {
    let code = prov::code_spans(out, ContentFormat::Markdown).unwrap_or_default();
    let mut from = 0;
    while let Some(rel) = out[from..].find("{{") {
        let at = from + rel;
        from = at + 2;
        if code.iter().any(|s| s.contains(&at)) {
            continue;
        }
        let end = out[at..]
            .char_indices()
            .nth(40)
            .map_or(out.len(), |(i, _)| at + i);
        warnings.push(format!(
            "`{}` is not a template: `{{{{ }}}}` is read only inside a link or image \
             destination now — write `:val[…]` for a value in text",
            out[at..end].replace('\n', "\\n")
        ));
    }
}

// ── Entry points ────────────────────────────────────────────────────────────

/// Filter a body to an audience, then expand its template.
///
/// The order is not interchangeable. Filtering first means a `:::each` inside a
/// region this audience may not see is never expanded — so a withheld region
/// costs nothing to render and, more to the point, cannot fail a publish by
/// naming something it should not have.
pub fn render_for_audiences(
    body: &str,
    format: ContentFormat,
    context: Context<'_>,
    viewer_audiences: &[&str],
    warnings: &mut Vec<String>,
) -> Result<String, String> {
    let filtered =
        visibility::filter_body(body, format, visibility::Audience::Only(viewer_audiences))
            .map_err(|e| e.to_string())?;
    expand(&filtered, format, context, warnings).map_err(|e| e.to_string())
}

/// [`render_for_audiences`] with every region kept — an author's own view.
pub fn render(
    body: &str,
    format: ContentFormat,
    context: Context<'_>,
    warnings: &mut Vec<String>,
) -> Result<String, String> {
    let filtered = visibility::filter_body(body, format, visibility::Audience::All)
        .map_err(|e| e.to_string())?;
    expand(&filtered, format, context, warnings).map_err(|e| e.to_string())
}

// ── Frontmatter → context ───────────────────────────────────────────────────

/// The page-level half of a context: this page's frontmatter, its file
/// metadata, and the viewer it is being rendered for.
///
/// Frontmatter keys land at the top level *and* under `page`, which is not
/// redundancy for its own sake: `:val[title]` is what every body written
/// against the old Handlebars context says, and `:val[page.title]` is what the
/// format's own vocabulary says. Both name one value.
pub fn page_values(
    frontmatter: &IndexMap<String, YamlValue>,
    file_path: &Path,
    workspace_root: Option<&Path>,
    viewer_audiences: &[&str],
) -> JsonMap<String, JsonValue> {
    let mut map = JsonMap::new();

    for (key, value) in frontmatter {
        map.insert(key.clone(), yaml_to_json(value));
    }

    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
        map.insert("filename".to_string(), JsonValue::String(stem.to_string()));
    }
    if let Some(ext) = file_path.extension().and_then(|s| s.to_str()) {
        map.insert("extension".to_string(), JsonValue::String(ext.to_string()));
    }
    let filepath = match workspace_root {
        Some(root) => file_path.strip_prefix(root).unwrap_or(file_path),
        None => file_path,
    };
    map.insert(
        "filepath".to_string(),
        JsonValue::String(filepath.to_string_lossy().to_string()),
    );

    if !viewer_audiences.is_empty() {
        map.insert(
            "viewer_audience".to_string(),
            JsonValue::String(viewer_audiences.join(", ")),
        );
        map.insert(
            "viewer_audiences".to_string(),
            JsonValue::Array(
                viewer_audiences
                    .iter()
                    .map(|a| JsonValue::String((*a).to_string()))
                    .collect(),
            ),
        );
    }

    map
}

/// Convert a metadata [`YamlValue`] to a `serde_json::Value`, so a path lookup
/// can address frontmatter fields.
///
/// `prov::Value` is deliberately serde-free (it walks fig's native value tree),
/// so the bridge is spelled out rather than derived. A float with no JSON
/// representation (NaN, infinity) degrades to null, which is what
/// `serde_json::Number::from_f64` reports for it.
pub fn yaml_to_json(value: &YamlValue) -> JsonValue {
    match value {
        YamlValue::Null => JsonValue::Null,
        YamlValue::Bool(b) => JsonValue::Bool(*b),
        YamlValue::Int(i) => JsonValue::Number((*i).into()),
        YamlValue::Float(f) => {
            serde_json::Number::from_f64(*f).map_or(JsonValue::Null, JsonValue::Number)
        }
        YamlValue::String(s) => JsonValue::String(s.clone()),
        YamlValue::Sequence(items) => JsonValue::Array(items.iter().map(yaml_to_json).collect()),
        YamlValue::Mapping(map) => JsonValue::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), yaml_to_json(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn site_with(entries: JsonValue, groups: JsonValue) -> SiteContext {
        SiteContext::new(
            json!({"title": "A site", "lang": "en", "base_url": ""}),
            entries.as_array().cloned().unwrap_or_default(),
            groups.as_array().cloned().unwrap_or_default(),
        )
    }

    fn run(body: &str, site: &SiteContext, page: JsonValue) -> (String, Vec<String>) {
        let page = page.as_object().cloned().unwrap_or_default();
        let mut warnings = Vec::new();
        let out = expand(
            body,
            ContentFormat::Markdown,
            Context::new(site, &page),
            &mut warnings,
        )
        .unwrap();
        (out, warnings)
    }

    #[test]
    fn a_value_directive_is_replaced_by_its_text() {
        let site = site_with(json!([]), json!([]));
        let (out, _) = run(
            "# :val[page.title]\n",
            &site,
            json!({"page": {"title": "Hi"}}),
        );
        assert_eq!(out.trim(), "# Hi");
    }

    #[test]
    fn a_value_inside_a_code_span_is_left_alone() {
        let site = site_with(json!([]), json!([]));
        let (out, _) = run("Write `:val[page.title]` for it.\n", &site, json!({}));
        assert!(out.contains(":val[page.title]"), "{out:?}");
    }

    #[test]
    fn each_repeats_its_body_once_per_entry() {
        let site = site_with(
            json!([{"title": "One", "href": "one.html"}, {"title": "Two", "href": "two.html"}]),
            json!([]),
        );
        let body = ":::each{of=entries as=entry}\n- [:val[entry.title]]({{entry.href}})\n:::\n";
        let (out, warnings) = run(body, &site, json!({}));
        assert!(out.contains("[One](one.html)"), "{out:?}");
        assert!(out.contains("[Two](two.html)"), "{out:?}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_destination_brace_outside_a_link_is_reported_not_substituted() {
        let site = site_with(json!([]), json!([]));
        let (out, warnings) = run(
            "Hello {{page.title}}.\n",
            &site,
            json!({"page": {"title": "X"}}),
        );
        assert!(out.contains("{{page.title}}"), "{out:?}");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    #[test]
    fn a_brace_in_a_code_block_is_neither_substituted_nor_reported() {
        let site = site_with(json!([]), json!([]));
        let body = "```\n{{page.title}}\n```\n";
        let (out, warnings) = run(body, &site, json!({"page": {"title": "X"}}));
        assert!(out.contains("{{page.title}}"), "{out:?}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_brace_in_a_link_label_is_not_a_destination() {
        let site = site_with(json!([]), json!([]));
        let (out, warnings) = run("[{{a}}](x.html)\n", &site, json!({"a": "no"}));
        assert!(out.contains("[{{a}}](x.html)"), "{out:?}");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    #[test]
    fn nested_each_binds_the_inner_name() {
        let site = site_with(
            json!([]),
            json!([{"key": "2026", "entries": [{"title": "Post"}]}]),
        );
        let body = "::::each{of=groups as=g}\n## :val[g.key]\n\n:::each{of=g.entries as=e}\n- :val[e.title]\n:::\n::::\n";
        let (out, _) = run(body, &site, json!({}));
        assert!(out.contains("## 2026"), "{out:?}");
        assert!(out.contains("- Post"), "{out:?}");
    }

    #[test]
    fn group_is_each_over_the_sites_own_groups() {
        let site = site_with(json!([]), json!([{"key": "2026", "entries": []}]));
        let (out, _) = run(":::group{as=g}\n## :val[g.key]\n:::\n", &site, json!({}));
        assert!(out.contains("## 2026"), "{out:?}");
    }

    #[test]
    fn if_keeps_a_body_whose_field_is_present_and_drops_one_whose_is_not() {
        let site = site_with(json!([]), json!([]));
        let page = json!({"page": {"date": "2026-08-26", "draft": ""}});
        let (kept, _) = run(":::if{has=page.date}\nDated\n:::\n", &site, page.clone());
        assert!(kept.contains("Dated"), "{kept:?}");
        let (dropped, _) = run(":::if{has=page.draft}\nDraft\n:::\n", &site, page);
        assert!(!dropped.contains("Draft"), "{dropped:?}");
    }

    #[test]
    fn if_reads_several_conditions_as_an_and() {
        let site = site_with(json!([]), json!([]));
        let page = json!({"page": {"date": "2026-08-26", "draft": true}});
        let (out, _) = run(
            ":::if{has=page.date not=page.draft}\nBoth\n:::\n",
            &site,
            page,
        );
        assert!(!out.contains("Both"), "{out:?}");
    }

    #[test]
    fn an_unknown_condition_is_an_error_naming_it() {
        let site = site_with(json!([]), json!([]));
        let page = JsonMap::new();
        let mut warnings = Vec::new();
        let err = expand(
            ":::if{equals=page.title}\nX\n:::\n",
            ContentFormat::Markdown,
            Context::new(&site, &page),
            &mut warnings,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("equals"), "{err}");
    }

    #[test]
    fn an_absent_value_is_empty_and_a_collection_is_an_error() {
        let site = site_with(json!([{"title": "One"}]), json!([]));
        let page = JsonMap::new();
        let mut warnings = Vec::new();
        let out = expand(
            "a:val[page.nope]b\n",
            ContentFormat::Markdown,
            Context::new(&site, &page),
            &mut warnings,
        )
        .unwrap();
        assert_eq!(out.trim(), "ab");

        let err = expand(
            ":val[entries]\n",
            ContentFormat::Markdown,
            Context::new(&site, &page),
            &mut warnings,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("collection"), "{err}");
    }

    #[test]
    fn each_over_a_missing_collection_renders_nothing() {
        let site = site_with(json!([]), json!([]));
        let (out, _) = run(
            ":::each{of=page.tags as=t}\n- :val[t]\n:::\n",
            &site,
            json!({}),
        );
        assert_eq!(out.trim(), "");
    }

    #[test]
    fn a_djot_body_is_not_templated() {
        let site = site_with(json!([]), json!([]));
        let page = JsonMap::new();
        let mut warnings = Vec::new();
        let body = ":::each{of=entries as=e}\nx\n:::\n";
        let out = expand(
            body,
            ContentFormat::Djot,
            Context::new(&site, &page),
            &mut warnings,
        )
        .unwrap();
        assert_eq!(out, body);
    }

    #[test]
    fn frontmatter_is_addressable_bare_and_under_page() {
        let fm = prov::meta::parse_value("title: Hi\n", prov::Format::Yaml)
            .unwrap()
            .as_mapping()
            .cloned()
            .unwrap_or_default();
        let mut page = page_values(&fm, Path::new("a/b.md"), None, &[]);
        page.insert("page".into(), JsonValue::Object(page.clone()));
        let site = site_with(json!([]), json!([]));
        let mut warnings = Vec::new();
        let out = expand(
            ":val[title] / :val[page.title] / :val[filename]\n",
            ContentFormat::Markdown,
            Context::new(&site, &page),
            &mut warnings,
        )
        .unwrap();
        assert_eq!(out.trim(), "Hi / Hi / b");
    }
}
