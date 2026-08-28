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
- '[Changelog](/docs/CHANGELOG.md)'
- '[Proposal: the plates template format](/docs/proposals/templating.md)'
- '[Proposal: a site is an export](/docs/proposals/site-declaration.md)'
- '[Audiences](/vocab/audiences.md)'
fronts:
- '[Public](/vocab/public.md)'
audience: public
---

# plates

A static site generator over a [`prov`](https://github.com/diaryx-org/prov)
archive.

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

## Status

`0.1`. The engine has been in production use for some time inside a larger
application; this is its first release as its own thing, so the API is expected
to move before `1.0`. What each crate cannot do yet is listed under its own
**Status** heading, beside the code that would have to change.

## Releasing

`release <command>`, from diaryx-org/devtools, which must be on PATH. `release
bump <spec>` moves the one version in `Cargo.toml` that all three crates
inherit; `release changelog [--write|--check]` writes the generated region of
[`docs/CHANGELOG.md`](docs/CHANGELOG.md); `release release <spec>` does the whole
cut — bump, `cargo xtask ci`, changelog, commit, tag — and stops there unless
given `--push`. What this repository states for itself is in
[`.config/release.toml`](.config/release.toml) and nothing else; the rest is that
tool's defaults.

Nothing runs on the tag. Publishing is `cargo publish --workspace`, run
deliberately, which orders the three crates by dependency and waits on the index
between them.

## License

MIT or Apache-2.0, at your option.
