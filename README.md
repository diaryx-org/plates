# plates

A static site generator over a [`prov`](https://github.com/diaryx-org/prov)
archive.

`prov` keeps a body of writing so that it lasts: provenance, fixity, hierarchy,
and a gate that decides what may leave. `plates` is how what leaves becomes a
website — the same archive, published, with nothing re-derived and nothing
invented on the way out.

The name is for the gold plates: a record engraved on a durable thing so that
someone much later can read it. It is also what a printing plate is — the master
you take impressions from. An archive is the plate; a site is the impression.

```
  prov            plates                   plates-render
  ─────           ──────                   ─────────────
  which           where it lands,          what it looks
  documents  ──▶  what ships with it  ──▶  like
```

## The three crates

| Crate | What it answers |
|---|---|
| [`plates`](plates) | Which documents a site holds, where each one lands, and what ships alongside it. Reads a `prov::Workspace`. |
| [`plates-render`](plates-render) | What HTML a document becomes. Reads nothing, resolves nothing, and compiles for `wasm32-unknown-unknown`. |
| [`plates-cli`](plates-cli) | The `plates` command: `build`, `watch`, `serve`, `clean`. Where a build lands, how a declaration is spelled, and when to build. |

The split is the point. `plates-render` is handed text and a description of the
site that text belongs to, and gives back HTML — so one rendering can run in a
command-line build, in a sync server, and in an edge worker without three
implementations quietly disagreeing about what a site looks like. `plates` is
the half that has a disk.

`plates` depends on `prov`, `plates-render` and `thiserror`, and that is the
whole list. A site is planned and collected from a `prov::Workspace` and nothing
else, which is what keeps it usable by any application over any vault dialect.

Everything a library must *not* decide lives in `plates-cli` instead: the
`sites:` vocabulary, the destination directory, and the socket. Another
application over the same archive format replaces that crate and keeps the
other two.

## What it does

- **Gate first.** A document is published to exactly the audiences it declares;
  an undeclared one is private. Visibility is never inherited, so a published
  entry under a private parent is the ordinary case rather than the edge case,
  and the navigation is built to survive it.
- **`:vis[…]` regions.** The gate decides which documents leave. Per-audience
  directives decide which *parts* of one does, filtered against the same
  audience name so a body and its site can never disagree.
- **Anchoring.** A site's front page's directory becomes the site's root, and
  every published path is written relative to it. A site fronted by
  `www/index.html` publishes its siblings at `/about/`, not `/www/about/`.
- **One collector.** Building to a directory, serving a live preview and
  uploading to a host are the same walk with different options, rather than
  three that agree until one of them is fixed.
- **Markdown, Djot and HTML**, read off each source's own extension and parsed
  by [`twig`](https://github.com/diaryx-org/twig) — the same parser an editor
  over the same archive would use.
- **Shell templates**, per-site and per-page: a site supplies an HTML file with
  named slots (`content`, `site_nav`, `breadcrumbs`, `head`, `footer`, …) and
  its own stylesheet. `layout: verbatim` ships an authored page byte for byte.
- **Arrangements.** Containment (the archive's own hierarchy) or grouped (by
  date at a chosen grain, or by any field), cut by the same view spec `prov`
  cuts by.
- **Sitemaps, feeds, canonical links** and Open Graph metadata, all dated off
  one chain — `date_of_document` → `created` → `updated` — so a journal of
  scanned letters syndicates by the date each letter was *written*, not the
  afternoon it was scanned.
- **HTML attachments as islands**: an authored HTML file embedded in a page
  ships verbatim in a sandboxed `<iframe>` that reports its own height.

## The command

```
cargo install plates-cli
```

installs `plates`, which finds the archive by walking up from the current
directory the same way `prov` does.

| | |
|---|---|
| `plates build` | Render every site into `_site` (`--out` to move it, `--site NAME` to write one site at the destination root). |
| `plates watch` | The same, then again on every change. |
| `plates serve` | A dev server on `http://127.0.0.1:4321`, each site under its own name, reloading when the archive moves. `--site NAME` serves that one at `/`. |
| `plates clean` | Remove what a build wrote. |

`--base-url https://example.org` is what canonical links, the sitemap,
`robots.txt` and the feeds are written against. Without one they are skipped,
which is the right default for a preview whose address is `localhost`.

A build records every path it wrote in `.plates-build` at the destination root.
That record is what lets the next build take back a page whose document was
deleted, and what lets `clean` be exact: **nothing removes a file no build of
ours wrote**, so a destination with no record is refused rather than emptied,
and `--out` typed one character wrong is not a way to delete somebody's work.

### Declaring a site

`plates` takes a `SiteSpec` already built and never reads a config vocabulary —
that is a vault dialect, not a site's shape. The spelling below is
`plates-cli`'s, and lives in that crate alone. It goes in the archive's config
document, beside prov's own `views:` and `exports:`, or in the root document's
frontmatter beside its `prov:` block:

```yaml
sites:
  blog:
    label: Field notes           # what a reader sees; defaults to the name, humanized
    audience: public             # the gate — the only required key
    view: daily                  # a prov view, for the arrangement; default is containment
    index: '[Home](id:7f3a91c)'  # the front page, as a link that survives a rename
    shell: .config/sites/blog/shell.html
    stylesheet: .config/sites/blog/style.css
    lang: en
```

An archive with no `sites:` block gets one site per prov `exports:` entry gated
on `audience` — same name, same label, same view. That is not a guess: an export
already *is* a named, closed set of documents that may leave the archive, and
what it lacks is only a shell, which has a default. An export gated on some
other field is skipped and said out loud rather than published under a rule
nothing showed anyone.

## Using it as a library

```rust
use std::collections::HashMap;

use plates::{CollectOptions, NoDigests, NoStamp, collect_site, plan_site, read_theme};
use plates_render::site::{SiteOptions, render_site};

// A site declaration: which gate admits it, which view arranges it, what fronts
// it. How a vault *spells* that declaration is the caller's business — a
// `SiteSpec` arrives here already built.
let plan = plan_site(&workspace, &spec, &views, root_doc).await?;
let theme = read_theme(&workspace, &spec, &views).await;

// Which documents leave, and what rides along with them.
let collected = collect_site(
    &workspace,
    &plan,
    &CollectOptions {
        audience: "public",
        strip_keys: &["publish"],
        stamp: &NoStamp,
        id_by_path: &HashMap::new(),
        digests: &NoDigests,
        digest: sha256_hex,
    },
)
.await?;

// What they look like.
let render = render_site(
    &collected.sources,
    &SiteOptions {
        site_title: Some(theme.title),
        // …plus the shell, stylesheet and arrangement `theme` resolved.
        ..SiteOptions::default()
    },
);

for page in &render.pages {
    std::fs::write(out.join(&page.dest_filename), &page.html)?;
}
for attachment in &collected.attachments {
    // attachment.dest_rel, attachment.source_path, attachment.hash
}
```

`render.template_error` and `render.page_shell_errors` are how a render reports
a theme it could not compile. It falls back to a shell that works rather than
failing, so a caller that can show those to a person should: silently serving
the wrong design is how a broken theme survives a release.

## Features

All three crates default to `yaml` and forward their metadata-format features to
`prov`, which forwards them to `fig`. With a format off, its parser is left out
of the build and `prov` stops recognizing it, so at least one must be on.

| Feature | |
|---|---|
| `yaml` *(default)* | `---` frontmatter, `registry.yaml` |
| `json`, `toml`, `fig-lang` | the other metadata dialects |
| `templating` *(`plates-render` only)* | Handlebars in bodies, resolved at render time. Off by default so a consumer that only needs markdown, HTML and nav does not compile a template engine it will not call. |

## Status

`0.1`. The engine has been in production use for some time inside a larger
application; this is its first release as its own thing, so the API is expected
to move before `1.0`. Known limitations, rather than surprises:

- The gate field is fixed to `audience`. A vault that names its visibility field
  something else cannot say so yet.
- A page claiming a destination with `serve_at:` does not get its *relative*
  body asset references re-based onto the claimed path. Root-absolute references
  work.
- `layout: verbatim` skips all rewriting, link rewriting included. A verbatim
  page's hrefs are final URLs by contract.
- Theme compilation warnings are returned, never logged. A caller that drops
  them shows a broken design to its readers.

## Building

```
cargo xtask ci
```

runs everything CI runs, in the same order: format, clippy, tests, docs,
a `wasm32-unknown-unknown` check of `plates-render`, and an MSRV check. Adding
or changing a job is an edit to `xtask/src/main.rs`; the workflow reads the list
from there.

Compiling needs [Zig](https://ziglang.org) on `PATH` — `fig` and `twig` reach
this workspace through `prov`, and their `build.rs` runs `zig build`.

## License

MIT or Apache-2.0, at your option.
