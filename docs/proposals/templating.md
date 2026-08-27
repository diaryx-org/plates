---
title: 'Proposal: the plates template format'
part_of: '[plates](/README.md)'
status: implemented
author: adammharris
created: 2026-08-26
audience: public
---

# The plates template format

## Summary

A plates template is **Markdown**. Block structure is spelled with twig's
generic directives (`:::each{…}`), and so are inline values (`:val[…]`).
`{{ }}` survives in **one position only** — a link or image destination, the one
place in Markdown that cannot hold a node — and even there it is resolved by
reading the destination off the AST, never by scanning text. Templates are
ordinary vault documents, read the way a shell and a stylesheet are read; the
HTML shell is unchanged.

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
`argument` and an attributes side-table (`twig/src/ast/ast.zig:286`;
`twig/src/languages/markdown/attributes.zig`). All three arities are
implemented: `:name[label]{attrs}` text, `::name[label]{attrs}` leaf,
`:::name{attrs}…:::` container.

The editor support is not incidental — it was built for this use case:

- `Editor::new_ext` takes `MarkdownExtensions { directives: true }`, and reparses
  with them after every edit "so a directive-bearing document stays parseable —
  needed before `Editor::filter` can match `directive[...]` selectors"
  (`twig/bindings/rust/twig/src/lib.rs:1371`).
- `Editor::unwrap_node`'s documented example is "peel a `:::vis{...}` container".
- `twig/docs/COOKBOOK.md:126` ships plates' audience filter as a recipe.

Twig also ships a prebuilt `libtwig.a` for `wasm32-unknown-unknown`, and
`plates-render` already links twig transitively through prov. Using more of it
costs nothing against the portability contract the `wasm` CI job enforces.

### Why not Handlebars block helpers

`{{#each}}` is invisible to the AST. A body full of block helpers is, to twig
and to any editor over it, one undifferentiated paragraph — which forfeits the
visual-editing constraint entirely.

There is a second reason, and it is the one that turned out to matter more.
Handlebars was handed the **whole body text** (`render_template`), so a
`{{title}}` inside a fenced code block was substituted. That is the same
grammar-blindness `plates-render/src/visibility.rs` was rewritten to delete — a
document *explaining* the syntax is the only kind that quotes it, and quoting is
not writing. A text-substitution engine reintroduces that bug through a second
door no matter how good its own syntax is.

## Values are directives too

The first draft of this proposal made inline values `{{ }}` and argued the cost
was worth paying: "the escape alphabet is the WYSIWYG budget, and `{{` costs
one." That charged the wrong ledger.

The moment `directives: true` is on — which `visibility.rs` now *requires*,
since it locates `:vis[…]` as a real AST node — **every `:word[…]` in every body
in the vault is already a directive**. That alphabet is spent. `{{` would be a
*second* alphabet on top of it, not the only one. So a value directive costs
nothing that has not already been paid, and buys three things text substitution
cannot: it does not exist inside a code span, it is addressable by every
operation twig has, and an editor cannot let someone split it in half.

Inline values are therefore `:val[path]`.

### Why `{{ }}` survives in link destinations

One position defeats this, and it is exactly one:

```markdown
- [:val[entry.title]](:val[entry.href]) — :val[entry.date]
```

Link and image **destinations** are not inline-parsed. Twig stores a destination
as a byte run (`Link = struct { destination: ?[]const u8, reference: ?[]const u8 }`,
`twig/src/ast/ast.zig:282`) and carries a *positional escape alphabet* for that
position (`Syntax.link_dest_escapes`, `twig/src/syntax.zig:128`), which
`Editor.insertLink` uses and the implemented `literal-text-insertion.md`
proposal builds on. A destination is not content twig has yet to parse; it is a
position twig decided is characters.

The scope of the exception is narrower than "inline values", though. Heading
text, list items, prose and table cells are all inline-parsed and take a text
directive fine, and directive *attributes* never needed interpolation at all —
attributes are already the value language. It is destinations, and nothing else.

So:

> **Block structure and inline values are directives. `{{ }}` survives in link
> and image destinations only, and is resolved from the `link`/`image` node's
> `destination` field, never by text substitution.**

That last clause is what keeps the exception from being a leftover. The
destination is located as the stretch of the link's span *after* its label
(`content_span.end .. span.end`), so a `{{` in the label, in a code span, or in
a paragraph is never in range. The escape hatch stays AST-driven, which was the
point of the format.

### Why not change twig instead

Considered, and declined. Making destinations inline-parsed so a directive could
live in one costs four things at once:

- **A closed payload opens.** A node holds one children list, which for a link
  is its label. A destination with nodes in it needs a second children list or a
  new kind — and `ast.zig:315` is explicit that a new kind cannot exist without
  declaring `level`, `contentModel` and `structuralChildren`, the invariant the
  whole vocabulary rests on.
- **Four serializers must agree** — Markdown, Djot, HTML, canonical round-trip.
  The HTML one percent-encodes destinations
  (`twig/src/languages/html/serializer.zig:219`), so an unresolved directive in
  one renders as `%3Aval%5B…`: every consumer that is not plates emits a mangled
  href.
- **It makes twig's Markdown a dialect**, at the bottom of a stack that fig,
  prov, leaf and diaryx all sit on, to serve one consumer's templating.
- **There is already a home for it.** If a destination should hold nodes, that is
  a `.sprig` question — `twig/docs/proposals/twig-native-language.md` exists
  because the AST is that language's model. Markdown's job in twig is to be
  Markdown.

And the need mostly is not real: twig already exposes the destination as a
readable, writable field on a node. This was never a parsing problem. It is a
resolution problem, and resolution is plates' job.

## The format

```markdown
:::each{of=entries as=entry}
- [:val[entry.title]]({{entry.href}}) — :val[entry.date]
:::
```

### Nesting

Directive fences nest by length, like code fences: an outer fence must be
**longer** than the fence it contains.

```markdown
::::each{of=groups as=g}
## :val[g.key]

:::each{of=g.entries as=entry}
- [:val[entry.title]]({{entry.href}})
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
| `:val[path]` | Insert a value. |
| `:::each{of=X as=Y}` | Repeat the body once per item of `X`, binding each to `Y`. |
| `:::if{…}` | Include the body when the conditions hold. |
| `:::group{as=Y}` | `:::each` over the site's own groups. |
| `:::vis{.audience}` | Publish this region to these audiences only. Unchanged in meaning; see [Migration](#migration-the-vis-attribute-spelling). |

`:::group` takes **no `by=`**, which is a deviation from this proposal's first
draft and a deliberate one: the arrangement a site's view declares is what
decides its groups, and a `by=` that disagreed with it would be a second
grouping nothing reconciles.

Anything beyond these needs the argument twig applies to a new AST kind: **a
concrete lens that cannot otherwise be said**, not a shape that seems likely to
be wanted.

### `:::if`'s conditions

`has=` and `not=`, which are `prov_views`' `Condition::Has` and
`Condition::Not(Has)` read the same way — present means *usable*, so an empty
string, an empty list and a `false` are all absent. Several attributes are an
implicit **and**, which is prov's rule for a multi-key `where:` block.

`equals`, `any-of` and `all-of` are **not** implemented. They need a value
position, and an attribute gives one key one value; designing that spelling is
an open question below, and guessing at it now would settle it by accident. A
condition this does not know is an error naming it, not a silently false one.

### The value language

Dotted-path lookup, with a numeric segment indexing a sequence
(`entries.0.title`). No expressions, no filters, no arithmetic.

An absent path is the empty string, so an optional field is writable without
wrapping every mention in `:::if`. A path naming a *collection* is an error:
there is no reading of "insert these forty entries here" that an author meant.

Formatting is served by pre-computed fields rather than by a filter syntax —
`entry.date`, `entry.date_year`, `entry.date_month` — because a filter language
is the thing that turns a template format into a template *engine*, and
`plates-render/src/dates.rs` already knows how to spell a date. A field that
turns out to be wanted is one line of context; a filter grammar is permanent.

### The context

| Name | Is |
|---|---|
| `site` | `title`, `lang`, `base_url` |
| `page` | the current page, as an entry |
| `entries` | the site's pages, in its own order |
| `groups` | `{key, entries}` per group, when the arrangement is grouped |
| `children` | the current page's `contents:` links |
| `parent` | the current page's `part_of` link, or null |
| `breadcrumbs` | root-to-here trail, this page last |
| `backlinks` | the entries that link *to* this page, by path |
| `relations` | this page's own relation edges, by relation name |
| `inbound` | the same, inverted: who names this page, by relation name |

An entry is `path`, `title`, `href`, `date`, `date_year`, `date_month`, `id`,
`description`, `group_keys`, `is_root`. Frontmatter keys are also addressable
bare (`:val[title]`) and under `page`, which is not redundancy for its own sake:
the first is what every body written against the old context says, the second is
what the format's own vocabulary says, and both name one value.

`entries` is in **source order with `nav_order` overriding** — the rule
`plates-render/src/nav.rs` sorts siblings by, restated in the context assembler
so a template listing entries and a nav listing them cannot disagree.

Every collection is built from the sources `build_pages` receives, which are
already the gate-admitted set. A template cannot reach a withheld document
because the data was never assembled — a property of *where* the context is
built rather than of a check, and tested as such
(`a_template_cannot_reach_a_withheld_document`).

The last three are the names the render layer cannot answer for itself. Finding
what a page is linked from — or what it links to, beyond what its own
frontmatter happens to say — means reading every document in the archive, and
`plates-render` reads nothing. So `plates` takes prov's census once per build,
reads it forwards and inverted, hands each collected source both edge lists on
`SourceFile::outbound`/`SourceFile::inbound`, and the context assembler resolves
those names against the entries it already built.

### The vocabulary is the vault's

`relations` and `inbound` are **mappings keyed by relation name**, so
`relations.sequel.0.title` is a plain dotted path and `:::each{of=inbound.author}`
is an ordinary repetition. The names come from the archive: a vault declares its
own relations (prov's `relation_defs`), and every one it declares is a key here.
Nothing in `plates` or `plates-render` holds a list of them, which is the whole
point — a vault that spells its cross-references `translation-of` gets
`inbound.translation-of` without either crate learning the word.

The relation the site's navigation is built from is in there too, unexceptional
and un-special-cased: `relations.contents` and `inbound.part_of` are edges like
any other, and the nav already publishes that same structure by another route.

A link written in **prose** is in neither. It carries no relation name, and there
is no reserved key for it — `body` is a name a vault may legitimately give a
relation, so taking it would be taking something that is not ours. Those links
reach a reader through `backlinks`, which is unchanged: the flat union of the
typed and the untyped, each linking document named once.

Both keys are always present, an empty mapping included, on the precedent
`backlinks` set — a template naming a relation this archive does not declare
renders nothing rather than failing to publish.

### Which puts the gate somewhere new

Worth saying out loud. `entries` is safe because it *is* the render set; these
three arrive from outside it, and prov's census is the vault's — a document the
gate refused links out of it, and is linked to from it, like any other.

So collection intersects with the set the plan admits before a name crosses the
boundary (`a_linker_this_site_does_not_admit_is_not_named`,
`a_relation_across_the_gate_is_named_from_neither_end`), and the assembler drops
a name no entry answers to
(`a_backlink_to_a_document_outside_this_render_is_not_published`). The first is
the guarantee; the second is what stops a caller's mistake becoming a dead link.

An edge has **two** ends, and the outbound direction is where that starts to
matter. An unfiltered backlink discloses the document that links here; an
unfiltered relation discloses the document this one points at — the same leak,
arriving the other way round — so both ends of every edge are checked. And a
relation whose every target is filtered out produces no key at all rather than an
empty list: the list renders as nothing either way, but the key would still be a
statement that the edge exists.

prov counts link *sites*, so a document naming this one in a relation and again
in a sentence is two inbound references. `backlinks` wants the document once and
deduplicates by path; `inbound` deduplicates within each relation. Both sort by
path — a rendered page is a build artifact, and two builds of one archive have to
be the same bytes.

## The shell is unchanged

The shell is an HTML document from `<!DOCTYPE html>` to `</html>`, so it cannot
be Markdown, and `ShellTemplate` keeps it.

`shell.rs`'s "Why not handlebars" section was corrected while this landed. Two
of its three reasons did not hold — escaping is refutable by our own
`register_escape_fn` call, and typo detection by `Template::elements` being
`pub`. The reason that actually decides it was unlisted: **handlebars-rust has
no configurable delimiters**, and `shell.rs`'s
`braces_that_are_not_slots_pass_through` test guarantees that
`<style>a{b:c}</style>` and `<script>if(x){{y()}}</script>` survive a shell
verbatim. Handlebars would parse `{{y()}}` as an expression, breaking every
existing theme in favour of `\{{`.

Bodies pay no such cost, which is the asymmetry that lets a body spell its
values with a directive while the shell keeps its own substitutor.

## Migration

Two migrations land with this, and both follow one discipline: **detect the old
spelling and name the document**. A report, never a fallback — the answer to
drift is to fix the document.

### The Handlebars bodies

A `{{ }}` outside a link destination is no longer a template. It publishes as
the literal text it is and is reported on `SiteRender::body_template_errors`,
identified by the page that wrote it.

The failure direction is safe in a way the `vis` one is not: an unmigrated
`{{title}}` is *visible on the page* rather than silently absent. So this warns
and lets the site out, rather than refusing it. Code is excluded via
`prov::code_spans`, the same code-awareness the visibility residue check uses.

### The `vis` attribute spelling

plates writes `:::vis{public}`. Under twig's attribute grammar a bare `name` is
a **key** with value `""`, so that parses as `[public=""]` — not a class. Twig's
selectors and its documented filter recipe expect `:::vis{.public}` and match
with `class~=public`, and classes accumulate space-joined, which is exactly what
a multi-audience region needs.

**Decision: migrate hard to `.public`.** One spelling, matching twig exactly, no
dual-path matcher. An unmigrated `:::vis{public}` matches no audience, so its
region is **dropped** — content disappears rather than leaks
(`the_old_bare_key_spelling_drops_rather_than_leaks`). That is the right
direction and still not good enough on its own: silent disappearance is
precisely what `SitePlan::case_drift` exists to prevent, so the report belongs
beside it. **That report is not written yet** — see [Not
implemented](#not-implemented).

> One open dependency. Twig's deferred native-language proposal says a companion
> `twig-attributes.md` "drops the `.class` sigil." That file is not in the twig
> repo and could not be read. It is scoped to the future `.sprig` format rather
> than to Markdown's directive extension — but confirm it before rewriting a
> vault's worth of content onto `.class`.

## What changed

1. **prov re-exports twig** and exposes its Markdown options, so `directives`
   can be turned on downstream. Landed in prov, ahead of this.
2. **`visibility.rs` retired onto twig's parser**, which is what made the
   directive vocabulary available to bodies at all.
3. **The body context widened.** `pages_from`'s pre-pass over every source now
   assembles the collection context from frontmatter alone — which is what
   breaks the circularity of a page whose template lists the pages. This is the
   change that makes templates able to see anything, and it is independent of
   syntax.
4. **`:val` / `:::each` / `:::if` / `:::group` are implemented** by locating
   `directive[name=…]` containers, expanding each one's interior *per binding*
   in a recursive call, and splicing the finished text back with `edit_range`.
   One directive per pass, re-reading the tree each time, for the reason
   `visibility.rs` records: twig reparses after every edit, so every span but
   the one just used is stale.
5. **`{{ }}` narrowed to destinations**, resolved off the node.
6. **Body template errors are surfaced.** `site.rs` used to do
   `.unwrap_or_else(|_| parsed.body.clone())`, so a broken template published
   its own source with nothing reported. It still publishes its own source —
   there is no better body — but it names itself on
   `SiteRender::body_template_errors` now, on the discipline `template_error`
   and `page_shell_errors` already set.
7. **Handlebars is gone** from the dependency tree. The `templating` feature
   pulls in `serde_json` and `indexmap` and no template engine at all.

## Not implemented

**The bare-`vis` drift report.** A `:::vis{public}` whose audience parses as a
key rather than a class should name its document in the publish preflight,
alongside `SitePlan::case_drift`. The drop behaviour is right already; the
report is what turns a silent disappearance into a named one.

**`template:` resolution.** A template document named by frontmatter and shared
across pages — `read_templates` mirroring `read_page_shells`, the text carried
to the renderer because the render crate opens no files, a template excluded
from the entry set so it never publishes as a page of its own. The format works
without it: any body can use the vocabulary today, and `template:` is about
*reuse*. It stays named by a new key rather than by `layout:`, for the reason
the first draft gave — `PageLayout::parse` treats anything unrecognized as
`Site`, so a typo in `layout: archive-index` would be undetectable.

## Open questions

- **Per-page reparse cost.** Expansion parses once per scope, and a `:::each`
  parses its interior once per item. A page with several `each` blocks over a
  large `entries` pays for that, and plates-render runs in an edge worker as
  well as a CLI. Measure before assuming it is fine.
- **`:::if`'s value position.** `equals` needs a key *and* a value, and an
  attribute gives one key one value. Reusing `views::Condition`'s vocabulary
  wholesale would keep one condition spelling across views, exports and
  templates — worth doing, and still not designed.
- **`entries` ordering under a grouped arrangement.** It is the nav's rule
  today, which is right for containment. A grouped site may want the grain's
  order instead; `groups` already carries the grouping, so this is a question
  about which of the two an ungrouped `:::each{of=entries}` should follow.

  Half of it is settled: `groups` itself is prov's grouping now, so its buckets
  and their order are `prov_views::group`'s — ascending by key, with the entries
  inside each in the site's own order. What is left is the flat `entries` list,
  which still follows the nav.

  It leaves one visible seam. prov groups ascending only, and the *synthesized*
  index reverses date grains so a calendar reads newest-first. A templated site
  over a dated view therefore lists its groups oldest-first while its nav lists
  them newest-first. Reversing in the template is one line, but the two ought
  not to disagree by default, and choosing which one moves is the open half of
  this question.
