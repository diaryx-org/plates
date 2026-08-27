---
title: plates
author: adammharris
config: prov.yaml
about: about.md
contents:
- '[plates](/plates/README.md)'
- '[plates-render](/plates-render/README.md)'
- '[plates-cli](/plates-cli/README.md)'
- '[Kitchen sink](/docs/kitchen-sink.md)'
- '[Proposal: the plates template format](/docs/proposals/templating.md)'
audience: public
---

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

Each one documents itself. This page is the map; the detail lives beside the
code it describes.

| Crate | What it answers |
|---|---|
| [`plates`](plates/README.md) | Which documents a site holds, where each one lands, and what ships alongside it. Reads a `prov::Workspace`. |
| [`plates-render`](plates-render/README.md) | What HTML a document becomes. Reads nothing, resolves nothing, and compiles for `wasm32-unknown-unknown`. |
| [`plates-cli`](plates-cli/README.md) | The `plates` command: `build`, `watch`, `serve`, `clean`. Where a build lands, how a declaration is spelled, and when to build. |

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
  an undeclared one is private, and visibility is never inherited.
- **`:vis[…]` regions.** Per-audience directives decide which *parts* of a
  document leave, filtered against the same audience name the gate used.
- **The archive's own hierarchy.** The navigation is the spanning tree `prov`
  materializes through the relation the vault *configures*, not a
  `contents:`/`part_of:` spelling of it re-derived here. A document the site
  does not publish is pruned out of the sidebar and what hung below it hoists
  up, so a published entry under a private parent keeps its place.
- **Anchoring.** A site's front page's directory becomes the site's root, and
  every published path is written relative to it.
- **One collector.** Building to a directory, serving a preview and uploading to
  a host are the same walk with different options.
- **Markdown, Djot and HTML**, read off each source's own extension and parsed
  by [`twig`](https://github.com/diaryx-org/twig) — the same parser an editor
  over the same archive would use.
- **Shell templates**, per-site and per-page, plus arrangements, sitemaps,
  feeds, canonical links and Open Graph metadata.

Each of those is written up where it lives: the gate, the anchor and the
collector in [`plates`](plates/README.md), the grammars, regions, shells and
feeds in [`plates-render`](plates-render/README.md).

## The command

```
cargo install plates-cli
```

installs `plates`, which finds the archive by walking up from the current
directory the same way `prov` does.

| | |
|---|---|
| `plates build` | Render every site into `_site`. |
| `plates watch` | The same, then again on every change. |
| `plates serve` | A dev server on `http://127.0.0.1:4321`, reloading when the archive moves. |
| `plates clean` | Remove what a build wrote. |

The flags, the `sites:` block a site is declared in, and what a build records
about itself are [`plates-cli`'s](plates-cli/README.md).

## Using it as a library

`plates` plans and collects a site out of a workspace; `plates-render` turns
what it collected into HTML. Both halves are shown, with runnable code, in
[`plates`](plates/README.md#using-it) and
[`plates-render`](plates-render/README.md#using-it).

## Features

All three crates default to `yaml` and forward their metadata-format features to
`prov`, which forwards them to `fig`. With a format off, its parser is left out
of the build and `prov` stops recognizing it, so at least one must be on.

| Feature | |
|---|---|
| `yaml` *(default)* | `---` frontmatter, `registry.yaml` |
| `json`, `toml`, `fig-lang` | the other metadata dialects |
| `templating` *(`plates-render` only)* | Handlebars in bodies, resolved at render time |

## Status

`0.1`. The engine has been in production use for some time inside a larger
application; this is its first release as its own thing, so the API is expected
to move before `1.0`. What each crate cannot do yet is listed under its own
**Status** heading, beside the code that would have to change.

## This repository is a prov archive

The documentation you are reading is itself a prov workspace, which is the
shortest honest test of the thing this repository builds.

```
prov tree README.md      # the document tree these pages form
prov check               # links, inverses, case drift, dangling ids
```

`README.md` is the root document; each crate's `README.md` is a child of it,
linked in both directions. `prov.yaml` is the workspace's configuration and
[`about.md`](about.md) is generated from it by `prov about` — nobody wrote that
page, and editing it by hand is pointless.

It is also a `plates` site, declared in `prov.yaml` beside prov's own config:

```yaml
sites:
  docs:
    label: plates
    audience: public
    index: '[plates](/README.md)'
```

The `audience: public` in each page's frontmatter is the gate that admits it, so

```
cargo run -p plates-cli -- serve
```

renders these very pages — the generator's documentation is one of its own
outputs.

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
