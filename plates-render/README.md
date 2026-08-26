---
title: plates-render
part_of: '[plates](/README.md)'
audience: public
---

# plates-render

The document-to-HTML half of [`plates`](https://github.com/diaryx-org/plates):
**what a published page looks like**.

This crate reads nothing and resolves nothing. It is handed source text and a
description of the site that text belongs to, and it gives back HTML — which is
what lets one rendering run in a command-line build, in a sync server, and in an
edge worker without three implementations quietly disagreeing about what a site
looks like.

```
  prov            plates                   plates-render
  ─────           ──────                   ─────────────
  which           where it lands,          what it looks
  documents  ──▶  what ships with it  ──▶  like
```

[`plates`](https://github.com/diaryx-org/plates/tree/main/plates) is the layer
above: it walks a `prov::Workspace`, decides which documents a site holds and
where each one lands, and calls this.

API documentation: [docs.rs/plates-render](https://docs.rs/plates-render).

## Portable by contract

It must keep compiling for `wasm32-unknown-unknown`, which is a constraint
rather than a preference: no host functions, no filesystem, no entropy and no
clock. A caller that has those reads the files and passes the bytes in — a shell
arrives as *text*, never as a path, and the date formatter re-spells a time it
was given rather than asking what time it is. The `wasm` job in `cargo xtask ci`
is what enforces it.

## Three grammars, one parser

A body's grammar is its own — Markdown, Djot or HTML, read off the source
document's extension via `prov::ContentFormat` — and all three go through
[`twig`](https://github.com/diaryx-org/twig), which is the same parser an editor
over the same archive uses. One engine for three grammars is the whole reason
`content_format` can exist, and it is why a document parsed by the publisher and
the same document parsed by the editor cannot disagree.

Before parsing, `preprocess_custom_syntax` rewrites Diaryx's own spellings —
highlights (`==like this==`), spoilers (`||like this||`) and HTML embeds — into
raw HTML, skipping fenced and inline code so a document *explaining* the syntax
is not rewritten by it. The same syntax works in Markdown and Djot, deliberately:
someone who switches a vault's `content_format` should not find that
`==highlight==` stopped working. HTML bodies are returned untouched.

## Highlighting

With the `syntax-highlighting` feature, a third stage runs after twig: fenced
blocks tagged with a language are coloured, in Markdown, Djot and hand-written
HTML bodies alike. It is a pass over the *rendered HTML* rather than over an
AST, which is what lets one implementation cover all three.

The grammars are `two-face`'s, which are bat's: 213 of them, against the 75
`syntect` bundles on its own. The difference is not academic — the smaller set
has no Zig, Swift, TOML or TypeScript.

Two properties are worth knowing before you style anything:

- **The output is classed, not styled.** syntect can write `style="color:#…"`
  on every span; this does not, because the colour would then be decided at
  render time for a stylesheet that has both a light and a dark palette. What
  it writes is a class per scope atom — `keyword.control.rust` becomes
  `plates-keyword plates-control plates-rust` — and the colours live in the
  stylesheet beside the site's own. Recolouring a language never means
  recompiling anything.
- **An unknown language is not an error.** A fence tagged with something no
  grammar answers to is returned byte for byte, still carrying the
  `language-…` class twig wrote. Only blocks that were actually coloured gain
  `plates-highlighted`, which is what the built-in sheet scopes its palette to.

A site with a language none of the 213 cover supplies its own grammar, as the
*text* of a `.sublime-syntax` file — this crate opens no files, so a caller with
one on disk reads it and passes the bytes in, exactly as it already does for a
shell template. `SiteOptions::syntaxes` is where they go; one that will not
parse is reported on `SiteRender::syntax_errors` and skipped, never fatal.

## `:vis[…]` regions

The gate decides which documents leave. This decides which *parts* of one does,
filtered against the same audience name, so a body and the site holding it can
never disagree about who a paragraph is for.

Every grammar spells a marked region as one node, and twig parses all three into
the same AST kind — a container with a name, a class list and children:

| Grammar | Spelling | Container name | Classes |
|---|---|---|---|
| Markdown | `:::vis{.family}` … `:::` | `vis` | `family` |
| Markdown (inline) | `:vis[text]{.family}` | `vis` | `family` |
| Djot | `{.vis .family}` on the line above `:::` | `""` | `vis family` |
| HTML | `<div class="vis family">` | `div` | `vis family` |

So the predicate is uniform and needs no per-grammar branch: **a region is a
container named `vis`, or one whose classes contain `vis`**, and its declared
audiences are its classes, less `vis` itself.

This is done through the parser rather than by scanning text, and the difference
was a disclosure bug: a scanner treats a marker inside a code span — in a
document explaining the syntax — as a real directive, and misses a real directive
whose fence a list has indented. twig parses the body it is going to render
anyway, so the spans are free and they are the spans the renderer will agree
with.

Filtering **fails closed**. A body whose grammar cannot be parsed, a region that
cannot be accounted for, a marker left standing after the walk: all are errors,
never a body returned unfiltered.

## The shell

A shell template is the outer HTML document a page is wrapped in — everything
from `<!DOCTYPE html>` down to `</html>` — with the parts this crate computes
left as named slots. The built-in shell fills exactly the same slots, so a
template replaces that document rather than introducing a second, parallel notion
of what a page is made of.

`{{name}}` inserts a **text** slot, HTML-escaped. `{{{name}}}` inserts a **raw
HTML** slot verbatim. Each slot is one kind or the other, and writing it the
other way is an error rather than a page full of `&lt;div&gt;`. Anything that is
not a well-formed slot reference passes through literally, so a `{{` in an inline
script or a CSS block is left alone.

| Slot | Kind | What it holds |
|---|---|---|
| `lang` | text | for `<html lang="…">` |
| `document_title` | text | `"Entry - Site"`, or the site's name on the front page |
| `site_title` | text | the site's name on its own |
| `body_class` | text | `has-site-nav`, or empty — write it inside `class="…"` |
| `head` | raw | stylesheet, favicon, SEO meta, feed links, the page's `styles:` |
| `site_nav` | raw | the navigation sidebar, empty when the site has no tree |
| `breadcrumbs` | raw | the breadcrumb trail |
| `content` | raw | the rendered body, links already rewritten |
| `footer` | raw | the built-in attribution footer |
| `scripts` | raw | the built-in interactivity script, then the page's `scripts:` |

`<title>` is not part of `head`, so a template decides where its own title tag
goes. A page may name its own shell with `shell:` in frontmatter; a key the site
does not carry falls back to the site shell and says why.

The substitutor is deliberately small — named slots, no expressions, no control
flow. Handlebars was available (this crate already links it for *bodies*) and was
turned down: it escapes by its own rule rather than the one the rest of the crate
uses, a misspelled variable is either silently empty or fatal with nothing in
between, and page assembly is compiled without the `templating` feature.

### Layouts

| `layout:` | |
|---|---|
| *(absent)* | The site shell: nav, breadcrumbs, footer, site stylesheet, built-in script — or the caller's template in place of all of it. |
| `bare` | A complete document with none of the site's frame, only the page's own `styles:`/`scripts:` around its rendered body. Still in the nav, the sitemap and the feeds: bare is about what a page looks like, not about whether the site knows it. |
| `verbatim` | The body *is* the file, written out byte for byte — no wrapper, no head, no chrome, and no parse. |

`verbatim` exists because a reserialized document is a *different* document:
attribute order moves, void tags are respelled, an inline `<script>` survives or
does not depending on how the parser felt about it. A designed landing page is a
file someone wrote, not a document someone described, and the only faithful thing
to do with it is copy it.

## Navigation is a forest, not a tree

Visibility is explicit-only, so a render set is an arbitrary subset of the
containment tree: a published entry whose parent is private is the normal case.
Descending from a single root left every such entry with a URL, a sitemap row and
a feed item, but no place in the sidebar. So containment survives where it
survives — a visible parent still nests its visible children — and every page the
walk cannot reach becomes a root of its own. The invariant, pinned by a test, is
that **every page in the render set appears exactly once in the nav**.

An `Arrangement` is either `Containment` or `Grouped`, and grouping is
`prov::views`' own `Grouping`/`Grain` — by date at a chosen grain, or by any
field — so a site groups its entries the way the vault's view cuts them, not a
second way that agrees until one of them is fixed.

## Dates, feeds and metadata

A vault's frontmatter carries dates as whatever the author typed. That is right
for a document — the grain a person wrote in is information, and prov keeps it —
and wrong for syndication, where Atom requires RFC 3339 and RSS 2.0 requires
RFC 822, and a feed carrying a bare `2026-08-16` is rejected by validators and
misparsed by readers. So the loose spelling is read once here and the strict one
written, for each grammar that needs it.

Which date is dated off one chain, first key *present* wins:
`date_of_document` → `created` → `updated`. A journal of scanned letters
syndicates by the date each letter was *written*, not the afternoon it was
scanned, and `date_of_document: unknown` is the conventional marker for a
deliberately undated record — it stops the chain rather than falling through to
the day the shoebox was imported. One sort order is shared between the generated
index and the feed, so a site cannot list its entries in one order and syndicate
them in another.

Sitemaps, `robots.txt`, canonical links, Open Graph metadata and both feeds are
generated together, and only with a `base_url`: a feed needs absolute URLs, so
without one there is no feed to advertise either.

## HTML attachments as islands

An authored HTML file embedded in a page ships verbatim inside a sandboxed
`<iframe>`. The frame is cross-origin by construction, so the two sides agree by
`postMessage` — the child reports its own height and the parent sizes the frame —
and the child half is written to the site root as one well-known script rather
than copied into every island.

## Features

| Feature | |
|---|---|
| `yaml` *(default)* | `---` frontmatter, `registry.yaml` |
| `json`, `toml`, `fig-lang` | the other metadata dialects |
| `templating` | Handlebars in bodies, resolved at render time, plus the whole-site entry point (`site::render_site`) |
| `syntax-highlighting` | Colour for fenced code blocks, via `syntect` and 213 Sublime grammars |

Metadata-format features forward to `prov`, which forwards them to `fig`. With a
format off, its parser is left out of the build and `prov` stops recognizing it,
so at least one must be on.

`templating` is off by default so a consumer that only needs Markdown, HTML and
nav does not compile a template engine it will not call. With it on, body
variables come from frontmatter and raw `{{ }}` syntax is preserved in the file
and resolved on every view and publish — and when a target audience is supplied,
`:vis[…]` filtering runs *before* interpolation.

`syntax-highlighting` is off for the same reason and more of it: the grammars
travel as an embedded dump of about a megabyte. See [Highlighting](#highlighting)
for what it does with them.

## Using it

Everything below the whole-site entry point is a pure function over the value
types in `types`, so a caller can use as much or as little of it as it needs:
`render_body` for one body, `build_site_nav_tree`/`nav_for_page` for navigation,
`transform_links` to rewrite `.md` targets to their published `.html` ones.

```rust
use plates_render::site::{SiteOptions, render_site};

let render = render_site(
    &sources,                       // path + text + which one is the root
    &SiteOptions {
        site_title: Some("Field notes".into()),
        base_url: Some("https://example.org".into()),
        generate_seo: true,
        generate_feeds: true,
        template: Some(shell_html), // the shell as text; None uses the built-in
        ..SiteOptions::default()
    },
);

for page in &render.pages {
    // page.dest_filename, page.html
}
for (name, bytes) in &render.assets {
    // the stylesheet, the favicon, the island child script
}
```

A render has **no error channel** — every page in `pages` is real HTML. A
template that will not compile falls back to the built-in shell and says why on
`render.template_error`; a page naming a shell the site does not carry falls back
the same way and is reported once, on `render.page_shell_errors`. A caller that
can show those to a person should: silently serving the wrong design is how a
broken theme survives a release.

## Status

`0.1`, and the API is expected to move before `1.0`. Known limitations, rather
than surprises:

- `layout: verbatim` skips all rewriting, link rewriting included. A verbatim
  page's hrefs are final URLs by contract.
- Theme compilation warnings are returned, never logged. A caller that drops them
  shows a broken design to its readers.
- Body HTML is twig's, not comrak's, which is what this crate used to run:
  tasklists come out as `<ul class="task-list">` and footnotes as
  `role="doc-endnotes"` with `#fn1` anchors rather than `#fn-1`. The bundled
  stylesheet styles both spellings, so a site published before the change and one
  published after render the same.

## License

MIT or Apache-2.0, at your option.
