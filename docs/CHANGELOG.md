---
title: Changelog
part_of: '[plates](/README.md)'
author: adammharris
audience: public
---

# Changelog

What has changed in plates, release by release, for someone deciding whether to
move to a newer one.

Two halves, written two different ways.

The bulleted groups below — **Added**, **Fixed**, **Changed**, and a
**Behavioural changes** section under them — are **generated** from the commit
log by `release changelog --write`, which reads the shared `cliff.toml` in
diaryx-org/devtools: the same file, and the same style, in every repository here.
Anything inside a `git-cliff:begin` / `git-cliff:end` pair is rewritten on every
run, so an edit made there is an edit thrown away.

Everything else is handwritten and stays: this prose, and any intro a release
needs under its own heading, below the end marker where regeneration cannot
reach it.

**Behavioural changes** are collected from `Behavioural-change:` trailers on the
commits themselves, not from their subjects — because "would a reader who
upgrades without editing a line of their own code observe a difference" is a
judgment about the change that no subject can carry. Write one trailer per
observable difference, as prose someone can act on.

plates publishes three crates — `plates-render`, `plates`, and `plates-cli` —
which move together on the one version in `Cargo.toml`. Nothing is published on
a tag: `cargo publish --workspace` is a command somebody runs deliberately.

## Unreleased

<!-- git-cliff:begin — generated; edits here are overwritten -->

_No commits since the last tag._

<!-- git-cliff:end -->

## v0.3.0 — 2026-08-27

### Added

- **sites** — a site is an export ([`7a71460`](https://github.com/diaryx-org/plates/commit/7a71460e3235fbad0492701608489c47b5a138ec))

### Behavioural changes

- an export whose gate names a field other than
  `audience` now becomes a site (it was previously skipped with a
  warning; its set is still exactly what its gate admits). A `sites:`
  block still builds but warns it is deprecated, naming its replacement.
  A page declaring `lang:` in frontmatter now renders with that language
  tag instead of the site's.


## v0.2.0 — 2026-08-27

### Breaking

- **visibility** — filter audience regions through the parser, not a scanner ([`ac54f88`](https://github.com/diaryx-org/plates/commit/ac54f88e4db89c8ee414b659c736a7b27f8f9e1f))
- **render** — colour fenced code blocks, in the languages a site writes ([`5972297`](https://github.com/diaryx-org/plates/commit/5972297a43357f671782014aa9c38e41f4625e5e))
- **render** — spell a body's template in the directives twig already parses ([`d6b223a`](https://github.com/diaryx-org/plates/commit/d6b223a7a95a12d981b02ddc6d469304c0135034))
- **render** — group a template's entries through prov's grouper ([`9667d2d`](https://github.com/diaryx-org/plates/commit/9667d2d21508830b4fb7227794fb0635c0f985b9))
- **plan** — tell a link that leads nowhere from one the gate held back ([`ec0d0cd`](https://github.com/diaryx-org/plates/commit/ec0d0cdb08a2504bfdfba4d8c69713ad736d96d3))
- **render** — tell a page who links to it, in the set its own gate admits ([`b16e236`](https://github.com/diaryx-org/plates/commit/b16e23622a6e3859a602f618d7fd1c92c5b34070))
- **nav** — build the navigation from the spanning relation a vault configures ([`72bde84`](https://github.com/diaryx-org/plates/commit/72bde845e1fccbde34fcf555d07114f1b294482c))
- **render** — publish a page's typed relations, in the names its vault declares ([`1047cb0`](https://github.com/diaryx-org/plates/commit/1047cb003627a722c42422230856be4bd05c4b6e))

### Added

- **cli** — a plates command with build, watch, serve and clean ([`b247b51`](https://github.com/diaryx-org/plates/commit/b247b51aa58040e6fa31ab1a6856555afd5a91e7))
- **plates** — let a site name the field its gate is judged on ([`b7df574`](https://github.com/diaryx-org/plates/commit/b7df5744d1db88b6c8995620e94b800f9a6eeba3))
- **release** — cut releases with the shared tooling ([`0c13314`](https://github.com/diaryx-org/plates/commit/0c13314b69e46ac425e7a946df0d79daa274afc2))

### Fixed

- **render** — repair the site-nav layout and rework the default stylesheet ([`c94464a`](https://github.com/diaryx-org/plates/commit/c94464afe88593ece3d798563468d892e36517ea))
- **render** — clear the mobile nav drawer of the toggle it opens under ([`6cc7ae1`](https://github.com/diaryx-org/plates/commit/6cc7ae138aa4bafb68597975672ab9ce7543e27e))
- **collect** — read a body's references with the parser that knows its code ([`51a1caa`](https://github.com/diaryx-org/plates/commit/51a1caaa58bb2fe563cbcad10edcdfd3f33b72f8))

### Behavioural changes

- an audience region must now be spelled with a CLASS
  (`:::vis{.public}`) rather than a bare key (`:::vis{public}`). A bare
  key declares no class, so it matches no audience and the region is
  dropped — content disappears rather than leaks, and the marker still
  goes, so this does not trip the residue check. Vault bodies must be
  migrated; a report naming the documents that still use the old
  spelling is not yet wired up.

- `filter_body_for_audience`/`filter_body_for_audiences`/
  `strip_visibility_directives` are replaced by one `filter_body(body,
  format, Audience)`. It takes the body's grammar, because the parse
  needs it — Markdown's generic directives are an opt-in extension and
  without it a region is a paragraph of literal text — and it returns
  `Result`, because refusing is now an outcome. `plates::Error` gains a
  `Visibility` variant; `Error` is exhaustive, so matches over it must
  add the arm.

- removing a block region no longer splices the
  paragraphs around it together. The scanner produced `Intro\nOutro`, a
  single paragraph with a soft break; the source had two blocks and the
  filtered body now keeps two.

- prov's floor rises to 0.9.1, where `prov::twig` is.

- every published page changes appearance. Anyone who
  forked `html_format_css.css`, or who diffs rendered HTML against a
  stored fixture, should expect the stylesheet asset to differ; the
  markup and the class names are unchanged.

- the default `--content-max-width` narrows from 48rem
  to 44rem, and the light `--accent` darkens from `#3b82f6` to `#2563eb`
  so that link text clears WCAG AA on the page background (it was 3.7:1).
  The dark palette is softened off near-black. A caller supplying a theme
  overrides all of these as before.

- the small-screen breakpoint no longer sets
  `font-size: 15px` on the body, which discarded a caller's
  `TypographySettings::base_font_size`. `--font-size` is now honoured at
  every width and only the frame tightens.

- `color-scheme: light dark` is declared on `:root`, so
  form controls, scrollbars and checkboxes follow the browser's mode. It
  follows the preference rather than the palette: a caller who overrides
  the colours for one mode only, pinning a light page in a browser set to
  dark, should pin `color-scheme` alongside it through `custom_css`.

- a table now shrinks to its content and scrolls when
  it will not fit, instead of always filling the measure. Reaching the
  old full-width look means saying so in `custom_css`.

- with `syntax-highlighting` on, which `plates-cli`
  now enables, a fenced block tagged with a language publishes as nested
  `<span>`s carrying `plates-`-prefixed classes where it was plain text
  before, and its `<code>` gains `plates-highlighted`. Anyone diffing
  rendered HTML against a stored fixture should expect those blocks to
  differ. A fence with no language, or one no grammar answers to, is
  unchanged byte for byte — including the `language-…` class twig wrote.

- the feature is off by default, so a library consumer
  who does not ask for it compiles no syntect, carries no dumps and sees
  no new markup. `plates` itself does not enable it; `plates-cli` does.

- `SiteOptions`, `SiteRender`, `SiteSpec` and
  `SiteTheme` each gain a field (`syntaxes`, `syntax_errors`, `syntaxes`,
  `syntaxes`). None is `non_exhaustive`, so a caller building one with a
  struct literal must name the new field. `SiteOptions::default()` and
  `SiteTheme::default()` are unaffected.

- the built-in stylesheet gains seven `--_syn-*`
  colour variables in both palettes and eight rules under
  `.plates-highlighted`. A caller who forked `html_format_css.css` gets
  no colour until they take them; the rules' *order* is load-bearing,
  since the selectors are single classes and a scope's atoms overlap.

- A `{{ }}` outside a link or image destination is no longer
 a template. It publishes as the literal text it is, and the page names itself
 on `SiteRender::body_template_errors` (surfaced by `plates build` as a
 warning). Rewrite `{{x}}` in text position as `:val[x]`.

- `plates_render::template`'s API changed shape. `render` and
 `render_for_audiences` take a `ContentFormat` and a `template::Context` rather
 than frontmatter and a path, and report into a `&mut Vec<String>` of warnings;
 `BodyTemplateRenderer`, `build_context` and `has_handlebars_templates` are
 gone, replaced by `SiteContext`, `Context`, `page_values` and `expand`.

- `SiteRender` gained `body_template_errors`, so a struct
 literal constructing one needs the new field.

- The `templating` feature no longer pulls in `handlebars`.
 A consumer that relied on it arriving transitively must depend on it directly.

- Body templating is Markdown-only. A Djot or HTML body is
 passed through untouched, where it used to have `{{ }}` interpolated.

- `plates::SiteSpec` gains a `gate_field: Option<String>`
 field, so every struct-literal construction of one needs a line adding.
 `None` is the previous behaviour exactly. `plates::AUDIENCE_FIELD` is
 unchanged in value and is now the default rather than the only possibility;
 `to_export` reads `SiteSpec::gate_field()` in its place.

- `groups` in a body template comes back ascending by key
 rather than in the order entries first mentioned each key. A grouped site whose
 template renders `:::each{of=groups}` will see its headings reordered — for a
 date grain, oldest-first where the nav is newest-first. The entries within each
 group are unchanged, and `entries` itself is unchanged.

- `plan_site` and `spec::finish` each take a `&[prov::CensusEntry]` as a new final argument, and `SitePlan` has a new `link_diagnostics` field. Callers pass `&[]` for the previous behaviour — an empty census yields an empty report and nothing else changes. Rendered HTML is byte-identical either way.

- `plates_render::site::SourceDoc` and
 `plates::SourceFile` each gain a `backlinks` field, and
 `plates::CollectOptions` gains one — three struct literals that no longer
 compile. `SourceDoc::backlinks` and `SourceFile::backlinks` take
 `Vec::new()` and `CollectOptions::backlinks` an empty map to keep the old
 behaviour exactly, at the cost of no page learning who links to it; pass
 `Workspace::backlinks(root_doc)` to turn the feature on. A body template
 addressing `backlinks` used to expand to nothing and now expands to a list,
 so a site that already wrote that name renders differently. A caller filling
 `SourceDoc::backlinks` itself must narrow the names to the same site: a path
 there is published as a titled link on the target's page.

- A reference written inside a code span is no longer
collected as an attachment. A `![x](img/a.png)`, `[x](img/a.png)`,
`[[img/a.png]]` or `<img src="a.png">` inside a fenced, indented or inline
code span used to be published as bytes alongside the page; it now is not, so
a site that documented its own markup will lose those attachments on the next
build. Two references start being collected that were not: a `[[file.png]]`
wikilink (prov's scanner sees it; a `(path)` scan never could), and a link
whose target holds balanced parens, `[a](/file (1).png)`, which is now read
whole rather than truncated at the first `)`. `preprocess_custom_syntax` now
leaves `==highlight==`, `||spoiler||` and `![x](y.html)` islands literal
everywhere the document's own parser calls the surrounding text code or raw
HTML — a block-level raw-HTML region included, where the old scanner rewrote
them.

- `build_site_nav_tree` takes the site's spanning outline as
 a second argument, and `SiteOptions::outline`, `CollectOptions::spanning_root`
 and `CollectedSite::outline` are new fields — a struct literal over any of the
 three stops compiling until it names them. A caller that passes no outline, and
 leaves `spanning_root` at `None`, gets exactly the nav, generated front page,
 breadcrumbs and template context it got before. A caller that passes one gets
 them built from the vault's configured spanning relation: the same tree for a
 vault that spells it `contents:`/`part_of:`, and a real hierarchy rather than a
 flat list for one that does not. One difference shows even under the diaryx
 spelling: a page whose container lists it nowhere in `contents:` is placed at
 the top of the nav as before, but its breadcrumb trail now reads from there
 rather than from the `part_of` it claims, so the trail and the sidebar can no
 longer disagree.

- every rendered page's context gains `relations` (outbound)
 and `inbound` (inbound), each a mapping of relation name to entry records,
 always present and empty when the page has none. The `backlinks` key is
 unchanged in meaning, shape and ordering. `SourceFile::backlinks` and
 `SourceDoc::backlinks` are replaced by `inbound`/`outbound`, both
 `Vec<plates_render::LinkEdge>` — a caller reading the old field wants
 `inbound.iter().map(|e| &e.path)` deduplicated. `CollectOptions` gains a
 required `census: &[prov::CensusEntry]` field, the same census `plan_site`
 takes; `&[]` keeps outbound relations empty.

## v0.1.0 — 2026-08-26

### Added

- a static site generator over a prov archive ([`6526a41`](https://github.com/diaryx-org/plates/commit/6526a4139777ba5e031f20dbe0fff0a50bca2741))

### Fixed

- long keyword ([`c59deb4`](https://github.com/diaryx-org/plates/commit/c59deb4e688ed01c68adeaf784dad7ba654afee5))

