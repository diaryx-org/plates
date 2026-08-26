---
title: 'Proposal: the plates template format'
part_of: '[plates](/README.md)'
status: accepted
author: adammharris
created: 2026-08-26
audience: public
---

# The plates template format

## Summary

A plates template is **Markdown**. Block structure is spelled with twig's
generic directives (`:::each{…}`), inline values with `{{ }}` text
substitution. Templates are ordinary vault documents, read the way a shell and
a stylesheet are read; the HTML shell is unchanged.

The format is chosen so that a template is a document twig can parse, an editor
can edit visually, and an author can read. Nothing here is a plates dialect:
the directive family is `micromark-extension-directive`'s, which twig already
implements and already edits.

## The problem

plates has three template surfaces and only one of them is any good.

- **Bodies** run Handlebars (`plates-render/src/template.rs`) over a context of
  frontmatter plus `filename`/`filepath`/`extension`. Nothing site-level is in
  scope, so a body cannot list other pages. There is no way to write an archive
  index, a tag page, or a "recent entries" block.
- **The shell** is ten named slots (`plates-render/src/shell.rs`), of which
  `site_nav`, `breadcrumbs` and `head` arrive as pre-rendered HTML. A theme
  author can restyle a site with CSS and cannot change its markup.
- **Layouts** do not exist.

The gap is usually described as "no real template language." That is half
right. The engine is not the constraint — the *context* is, and no engine
change fixes a template that can only see its own frontmatter.

## Why directives, and not another engine

Twig already has this format, and its editor already edits it.

Generic directives parse to `container` nodes carrying `name`, `form`,
`argument` and an attributes side-table (`twig/src/ast/ast.zig:213`;
`twig/src/languages/markdown/attributes.zig`). All three arities are
implemented: `:name[label]{attrs}` text, `::name[label]{attrs}` leaf,
`:::name{attrs}…:::` container.

The editor support is not incidental — it was built for this use case:

- `Editor::new_ext` takes `MarkdownExtensions { directives: true }`, and reparses
  with them after every edit "so a directive-bearing document stays parseable —
  needed before `Editor::filter` can match `directive[...]` selectors"
  (`twig/bindings/rust/twig/src/lib.rs:1346`).
- `Editor::unwrap_node`'s documented example is "peel a `:::vis{...}` container"
  (`:1430`).
- `twig/docs/COOKBOOK.md:126` ships this recipe:

  ```sh
  twig filter doc.md --directives \
    --drop 'directive[name=vis]' --keep 'directive[class~=public]' --unwrap
  ```

That last one is plates' audience filter, exactly, as a shipped twig feature —
while `plates-render/src/visibility.rs` reimplements it in 290 lines of
hand-rolled string scanning, and prov's `parse()`
(`prov-graph/src/content.rs:108`) calls `twig::Document::parse_str` with default
options, so `directives` is off and the markers are stripped before twig ever
sees a directive node.

Twig also ships a prebuilt `libtwig.a` for `wasm32-unknown-unknown`
(`twig/bindings/rust/twig-sys/build.rs:65`), and `plates-render` already links
twig transitively through prov. Using more of it costs nothing against the
portability contract the `wasm` CI job enforces.

### Why not Handlebars block helpers

`{{#each}}` is invisible to the AST. A body full of block helpers is, to twig
and to any editor over it, one undifferentiated paragraph — which forfeits the
visual-editing constraint entirely. Handlebars also has no configurable
delimiters (`{{` is hardcoded in `handlebars-6.4.0/src/grammar.pest:59`), which
is what rules it out for the shell; see [The shell is unchanged](#the-shell-is-unchanged).

### Why not directives all the way down

Tried, and it breaks on the most common template there is:

```markdown
- [:f[entry.title]](:f[entry.href]) — :f[entry.date]
```

Link destinations are not inline-parsed in Markdown or Djot — they are opaque
text. **No AST-level inline construct can live in a link destination**: not a
text directive, not a `symb`. A list of links is the single most common thing a
template produces, so this is disqualifying rather than awkward.

Inline interpolation therefore has to be plain-text substitution. That is a
constraint, not a preference, and it is what makes the format a hybrid.

## The format

**Block structure is a directive. An inline value is `{{ }}`.**

```markdown
:::each{of=entries as=entry}
- [{{entry.title}}]({{entry.href}}) — {{entry.date}}
:::
```

An author learns two constructs, and the one they write most often is the one
that already works today.

### Nesting

Directive fences nest by length, like code fences: an outer fence must be
**longer** than the fence it contains.

```markdown
::::each{of=groups as=g}
## {{g.key}}

:::each{of=g.entries as=entry}
- [{{entry.title}}]({{entry.href}})
:::
::::
```

This is twig's rule ("three or more of the marker … a closer of at least the
opening length"), not one of ours. It is stated here because it is the first
thing an author gets wrong.

### The directive vocabulary

Deliberately four. A vocabulary that grows one directive per wish becomes a
programming language nobody chose to design.

| Directive | Means |
|---|---|
| `:::vis{.audience}` | Publish this region to these audiences only. Unchanged in meaning; see [Migration](#migration-the-vis-attribute-spelling). |
| `:::each{of=X as=Y}` | Repeat the body once per item of `X`, binding each to `Y`. |
| `:::if{…}` | Include the body when the condition holds. |
| `:::group{by=…}` | Sugar for the nested-`each` shape above, over the site's arrangement. |

`:::group` earns its place because grouped arrangement is the case plates
exists to serve and the nested form is the one authors get wrong. Anything
beyond these four needs the argument twig applies to a new AST kind: **a
concrete lens that cannot otherwise be said**, not a shape that seems likely to
be wanted.

### The value language

Dotted-path lookup. No expressions, no filters, no arithmetic.

Formatting is served by pre-computed fields rather than by a filter syntax —
`entry.date`, `entry.date_year`, `entry.date_month` — because a filter language
is the thing that turns a template format into a template *engine*, and
`plates-render/src/dates.rs` already knows how to spell a date. A field that
turns out to be wanted is one line of context; a filter grammar is permanent.

### The context

What is in scope, and gate-scoped by construction: every collection below is
built from the sources `build_pages` receives, which are already the
gate-admitted set. A template cannot reach a withheld document because the
data was never assembled. **This property needs a test**, not just a paragraph.

| Name | Is |
|---|---|
| `site` | `title`, `lang`, `base_url` |
| `page` | the current page: `title`, `href`, `date`, `id`, `group_keys` |
| `entries` | the site's pages, in arrangement order |
| `groups` | `{key, entries}` per group, when the arrangement is grouped |
| `children` | the current page's `contents` links |
| `parent` | the current page's `part_of` link, if any |
| `breadcrumbs` | root-to-here trail |

`backlinks` joins this list when prov's `graph().backlinks_to()` is wired up;
it is named here so the context has a place for it rather than growing an
inconsistent one later.

### The escape-alphabet cost, stated

Twig's native-language proposal argues that "the escape alphabet is the WYSIWYG
budget" (`twig/docs/proposals/twig-native-language.md`, Part 6) — an argument
against adding significant character sequences to bodies. `{{` costs one.

It is worth paying here and not in the shell: bodies are Markdown, where a
literal `{{` is vanishingly rare, while a shell is an HTML document, which is
exactly where inline `<style>` and `<script>` braces live. That asymmetry is
the whole reason the shell keeps its own substitutor.

## The shell is unchanged

The shell is an HTML document from `<!DOCTYPE html>` to `</html>`, so it cannot
be Markdown, and `ShellTemplate` keeps it.

`plates-render/src/shell.rs`'s "Why not handlebars" section should be corrected
while this lands. Two of its three reasons no longer hold:

- **Escaping** — refuted by our own code. `register_escape_fn` is called at
  `template.rs:34`; installing `page::html_escape` gives one rule.
- **Typo detection** — refuted by `Template::elements` being `pub`. Walking the
  compiled AST to validate slot names is ~40 lines.

The third (feature gating) is real but self-imposed. The reason that actually
decides it is unlisted: **handlebars-rust has no configurable delimiters**, and
`shell.rs`'s `braces_that_are_not_slots_pass_through` test guarantees that
`<style>a{b:c}</style>` and `<script>if(x){{y()}}</script>` survive a shell
verbatim. Handlebars would parse `{{y()}}` as an expression, breaking every
existing theme in favour of `\{{`. That reason should replace the two that are
wrong.

## Templates are vault documents

A template is a prov document with a Markdown body — gated, versioned, editable
in the same editor as everything else. The archive holds its own plate.

**Named by a new `template:` key, not by `layout:`.** `PageLayout::parse`
treats anything unrecognized as `PageLayout::Site`
(`plates-render/src/types.rs`), so `layout: archive-index` would silently mean
"the ordinary layout" and a typo would be undetectable. `layout:` keeps its
three modes; `template:` names a document.

**Read off the workspace, not through the gate.** `read_templates` mirrors
`read_page_shells` (`plates/src/theme.rs`): each distinct path read once, texts
carried to the renderer because the render crate opens no files, unreadable
files reported as warnings rather than fatal. A template is not disclosed — its
*output* is — so reading one outside the gate is correct, and worth saying out
loud since it is disclosure-adjacent.

**A template is never itself a page.** A template document that is also
gate-admitted would otherwise publish as an entry, rendering its own directives
as prose. Excluded by path from the entry set, the way `collect_site` already
drops a manifest index node.

## Migration: the `vis` attribute spelling

plates writes `:::vis{public}`. Under twig's attribute grammar a bare `name` is
a **key** with value `""`, so that parses as `[public=""]` — not a class. Twig's
selectors and its documented filter recipe expect `:::vis{.public}` and match
with `class~=public`, and classes accumulate space-joined, which is exactly what
a multi-audience region needs.

**Decision: migrate hard to `.public`.** One spelling, matching twig exactly, no
dual-path matcher.

The failure direction is safe. An unmigrated `:::vis{public}` matches no
audience, so its region is **dropped** — content disappears rather than leaks.
That is the right direction and still not good enough on its own: silent
disappearance is precisely what `SitePlan::case_drift` exists to prevent.

So the migration carries the same discipline: **detect the old spelling and name
the files**. A bare `vis` attribute that is not a class is reported as a
warning identifying the document, alongside `case_drift` in the publish
preflight. A report, never a fallback — the answer to drift is to fix the
document.

> One open dependency. Twig's deferred native-language proposal says a companion
> `twig-attributes.md` "drops the `.class` sigil." That file is not in the twig
> repo and could not be read. It is scoped to the future `.sprig` format rather
> than to Markdown's directive extension — but confirm it before rewriting a
> vault's worth of content onto `.class`.

## What has to change, in order

1. **prov re-exports twig.** `prov` depends on `twig-doc = "3"`
   (`prov-graph/Cargo.toml:37`) and re-exports nothing. `pub use twig_doc as
   twig;` keeps plates' dependency list at prov + plates-render + thiserror and
   makes version skew structurally impossible — the same reason plates
   re-exports prov and plates-render.
2. **prov exposes twig's Markdown options.** `content.rs:108` hardcodes
   defaults, so nothing downstream can turn `directives` on. This gates
   everything below it.
3. **`visibility.rs` retires onto `Editor::filter`.** Deletes ~290 lines and
   fixes a real bug class for free: the hand-rolled scanner has no notion of
   code spans, so a `:vis[` inside backticks is treated as a directive. Twig's
   filter also re-parses until it converges, which handles nesting a single
   pass does not.
4. **The body context widens.** `build_pages`
   (`plates-render/src/site.rs:204`) already makes a pre-pass over every source
   to build `path_to_filename` and `title_map`; the collection context is
   assembled in that same pass. This is the change that makes templates able to
   see anything, and it is independent of syntax.
5. **`:::each` / `:::if` / `:::group` are implemented** by locating
   `directive[name=…]` containers, substituting over each one's interior
   source, and splicing the result back with `Editor::replace_content`.
6. **`template:` resolution and `read_templates`** land alongside.

Steps 1–2 are prov changes and belong in prov's release, not plates'.

## Open questions

- **Per-page reparse cost.** `Editor` reparses after every edit by design. A
  page with several `each` blocks pays for several reparses, and plates-render
  runs in an edge worker as well as a CLI. Measure before assuming it is fine;
  a whole-document splice pass may be needed instead of per-node edits.
- **Body template errors are still swallowed.** `site.rs:457` does
  `.unwrap_or_else(|_| parsed.body.clone())`, so a broken template publishes its
  own source with nothing reported — against the discipline `template_error` and
  `page_shell_errors` set, and against the README's own "silently serving the
  wrong design is how a broken theme survives a release." This must be fixed
  **before** authors get control flow to get wrong, not after.
- **`:::if`'s condition grammar.** prov has `views::Condition` (`has`, `equals`,
  `not`, `any-of`, `all-of`) with a settled spelling. Reusing it would keep one
  condition vocabulary across views, exports and templates. Worth doing, and not
  yet designed.
