---
part_of: id:dx4hxn744/kv2bv2m
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
- '[Proposals](/docs/proposals/proposals.md)'
- '[Tasks](/docs/tasks/tasks.md)'
- '[Audiences](/vocab/audiences.md)'
fronts:
- '[Public](/vocab/public.md)'
audience: public
registry: registry.yaml
id: p8ftrd5
slug: plates
updated: 2026-10-13T23:51:28.881722Z
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
- **Mounting.** A published page's foreign reference into another archive the
  device knows where to find mounts that archive's export for the same audience
  at `/<slug>/` — the `slug` the peer's root document declares, else its
  workspace name — one site out of an organization's repositories, each keeping
  its own build.
- **Markdown, Djot and HTML**, read off each source's own extension and parsed
  by [`twig`](https://github.com/diaryx-org/twig) — the same parser an editor
  over the same archive would use.
- **Shell templates**, per-site and per-page, plus arrangements, sitemaps,
  feeds, canonical links and Open Graph metadata.
- **Identity in the head.** Every page says which *document* it is — a
  `DC.identifier` carrying the archive-qualified `prov` reference, or whatever
  identifier the publisher supplies — so what anchors to a page anchors to the
  document rather than to the address it was served at.

Each of those is written up where it lives: the gate, the anchor and the
collector in [`plates`](plates/README.md), the grammars, regions, shells and
feeds in [`plates-render`](plates-render/README.md).

## Status

`0.1`. The engine has been in production use for some time inside a larger
application; this is its first release as its own thing, so the API is expected
to move before `1.0`. What each crate cannot do yet is listed under its own
**Status** heading, beside the code that would have to change.

## Releasing

[`ship`](https://github.com/diaryx-org/ship), which must be on PATH. `ship
bump <spec>` moves the one version in `Cargo.toml` that all three crates
inherit; `ship changelog [--write|--check]` writes the generated region of
[`docs/CHANGELOG.md`](docs/CHANGELOG.md); `ship <spec>` does the whole cut —
CI, bump, changelog, commit, tag — and stops there unless given `--push`. What
this repository states for itself is in [`.config/ship.figl`](.config/ship.figl)
and nothing else; the rest is that tool's defaults.

CI is the `job` list in the same file: `ship ci` runs every job in the order
the workflow does, and `ship ci <job>` runs one.

plates is not on crates.io — it is consumed from git, by commit — so a tag is a
name for a commit and sets nothing off.

## License

MIT or Apache-2.0, at your option.
