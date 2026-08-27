---
title: plates
part_of: '[plates](/README.md)'
audience: public
---

# plates

Turning an archive into a website: **where a vault's documents land, and what
ships alongside them**.

The middle of three crates.
[`prov`](https://github.com/diaryx-org/prov) says *which documents* a site
holds — the gate, the view, and the one-way valve between them.
[`plates-render`](https://github.com/diaryx-org/plates/tree/main/plates-render)
says *what HTML a document becomes*, reading nothing and resolving nothing.
Between them sits this crate.

```
  prov            plates                   plates-render
  ─────           ──────                   ─────────────
  which           where it lands,          what it looks
  documents  ──▶  what ships with it  ──▶  like
```

API documentation: [docs.rs/plates](https://docs.rs/plates). The repository,
and what the other two crates do, is
[here](https://github.com/diaryx-org/plates).

## What that means concretely

- **The gate.** A site's document set is bounded by the audience it declares.
  A document is published to exactly the audiences it names, and an undeclared
  one is private; visibility is never inherited, so a published entry under a
  private parent is the ordinary case rather than the edge case. A view may
  narrow that set and order it. A view may never admit a document the gate held
  out — `prov::exports::compose` enforces that structurally, by seeding from
  what the gate admits and only ever calling `retain`.
- **The anchor.** A site's front page's *directory* becomes the site's root,
  and every published coordinate — a source key, a page's destination, an
  attachment — is written relative to it. A site fronted by `www/index.html`
  publishes its siblings at `/about/`, not `/www/about/`. It is also the one
  place two correct answers disagree: a leading `/` means the *vault* root to
  `prov::link::resolve` and the *site* root to `plates-render`, and
  `collect::anchor_of` is where they are reconciled.
- **The reference scan.** Which files a page drags along: link targets,
  `src`/`href`/`srcset` attributes, and the frontmatter `attachments:`,
  `styles:` and `scripts:` lists. Grammar-blind, so Markdown, Djot and HTML all
  work through one scanner.
- **Destinations.** Path-to-URL shaping, `serve_at:` claims, and refusing two
  documents that claim one address rather than letting one quietly overwrite the
  other.
- **The front page.** `SitePlan` on top of prov's export plan: an index resolved
  through the spanning relation so it survives a rename, a move and a retitle;
  the rule that an index need not be among the entries but must be admitted by
  the gate; and `IndexDirectory`, a manifest node fronting a site with a whole
  covered directory, rebased onto the site root.
- **The spanning outline.** Which document contains which, materialized by prov
  through the relation the *workspace configures* rather than through one
  dialect's `contents:`/`part_of:` spelling. It rides on `CollectedSite::outline`
  and is what `plates-render` builds a site's navigation from, because reading a
  vault's `spanning:` needs a vault and the renderer has none. Walked from
  `CollectOptions::spanning_root`; naming no root collects no outline.
- **The theme.** Reading a declaration's shell and stylesheet into *text*,
  because the renderer cannot open a file. A missing one is reported in
  `SiteTheme::warnings` and ignored — a vault that cannot publish because a
  theme file was renamed has paid its existence for its styling.
- **The link report.** A render demotes every link it cannot publish to the same
  unclickable span, which is right for the page and useless as a report: a link
  to a page the gate holds back is the gate working, and a link to a renamed
  file is a mistake. prov's census tells them apart, and
  `SitePlan::link_diagnostics` carries only the second kind — named, never
  fatal, and never a reason a document is added to or dropped from a site.

## Why a site is neither an audience nor a view

An audience answers *may this document leave the vault*. A view answers *how
what stays is arranged*. Neither derives the other, and a published site needs
both.

The temptation is to collapse the two — to make an audience "just a filter on a
view", since both select documents by the value of a declared field. Three
things stop it. A wrong view is a wrong grouping you fix in the picker; a wrong
gate is a file in a stranger's hands, which is why a gate field is closed where
a user-declared field is open. A view with no `under:` covers the whole vault,
while a document with no gate value is visible to no one — open-by-default
against closed-by-default, and one primitive cannot hold both. And the gate
value is written *in the document*, so it travels with the file into another
vault and still means what it meant, where view membership is a property of the
vault and cannot be.

So a gate is not a *kind* of filter. It is a *position*: the domain every view
runs over once the corpus leaves the vault.

A site is named separately from its gate for a related reason. A site's name is
its path segment in every published URL — public surface that outlives any one
page — while a gate value is private vocabulary, chosen to be precise about
*who*, and the honest name for a readership is routinely one its members should
never read off a URL.

## The dependency list is the design

`prov`, `plates-render` and `thiserror`. That is the whole list, and it is meant
to stay that way: a site is planned and collected from a `prov::Workspace` and
nothing else, and `plates-render` is taken *without* its `templating` feature,
so no template engine is linked here — running the render pipeline is the
caller's job.

What that buys is that a caller's several commands cannot drift. Building to a
directory, serving a live preview and uploading to a host are one collector with
different options bolted to it (`CollectOptions`), rather than three walks that
agree until one of them is fixed.

## Using it

A `SiteSpec` arrives already built — how a vault *spells* a site declaration is
the caller's business, and `plates-cli` is one answer to it.

```rust
use std::collections::HashMap;

use plates::{CollectOptions, NoDigests, NoStamp, collect_site, plan_site, read_theme};
use plates_render::site::{SiteOptions, render_site};

// Every link in the archive, resolved against the archive — one walk per build,
// since the answer is the same for every site planned from it. It is what tells
// a link to an unpublished page from a link to nothing at all, and `&[]` is a
// caller that does not want `SitePlan::link_diagnostics`.
let census = workspace.census(root_doc).await?;

// Which gate admits this site, which view arranges it, what fronts it.
let plan = plan_site(&workspace, &spec, &views, root_doc, &census).await?;
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
        // Who links here, and — read forwards, the same walk — what each page
        // links to through a relation.
        backlinks: &workspace.backlinks(root_doc).await?,
        census: &census,
        // Where the spanning walk starts, so the collected site carries the
        // archive's own hierarchy for the nav to be built from.
        spanning_root: Some(root_doc),
        digests: &NoDigests,
        digest: sha256_hex,
    },
)
.await?;

// What they look like — plates-render's half.
let render = render_site(
    &collected.sources,
    &SiteOptions {
        site_title: Some(theme.title),
        outline: collected.outline,
        template: theme.template,
        arrangement: theme.arrangement,
        lang: theme.lang,
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

`CollectedSite` is the boundary between the half of the work that needs a vault
and the half that does not: a renderer, a publisher and a content diff all start
from the same one, and none of them opens the workspace again.

`plates` re-exports both layers below it — `plates::prov` and
`plates::plates_render` — so a downstream caller names one `prov` and one
`plates_render` rather than resolving two versions of the `Workspace` it is
about to hand back.

### Who links here, and what this links to

`CollectOptions::backlinks` is prov's census inverted and `CollectOptions::census`
is the same census read forwards — the whole vault's, both taken once per run
because a document reached by two sites should be censused once. What comes out
is `SourceFile::inbound` and `SourceFile::outbound`: the edges at each end of a
page, every one carrying **the name of the relation it is written in**, which is
the vault's own (`sequel`, `translation-of`, whatever its configuration declares)
and is never interpreted here. A link written in prose carries no name and is
inbound-only; nothing invents one for it.

Narrowing is this crate's, and it is a disclosure control rather than tidiness: a
document the gate refused sits in that census like any other, and an edge is
published as a titled link on the page at the far end. So collection intersects
with the set the plan admits — **both ends of every edge**, because a private
target is disclosed by being named exactly as a private source is — spells what
survives in the same coordinates as `SourceFile::source_rel_path`, and names each
(relation, document) pair once however many times it is written.

### Not reading what you already know

`CollectOptions::digests` is a port, not a cache. For an attachment the
published bytes *are* the file's bytes, so the only reason to read a 40 MB video
during a preview is to compute a digest that was computed last time the same
unchanged file was looked at. A caller that already keeps a stat-validated index
answers from it (`DigestMemo`); one with nowhere to keep an answer passes
`NoDigests` and pays what it paid before. An answer is served only when length
*and* modification time both still match, which is the same test prov's fixity
cache applies.

## Features

Defaults to `yaml`, and forwards its metadata-format features to `prov`, which
forwards them to `fig`. With a format off, its parser is left out of the build
and `prov` stops recognizing it, so at least one must be on.

| Feature | |
|---|---|
| `yaml` *(default)* | `---` frontmatter, `registry.yaml` |
| `json`, `toml`, `fig-lang` | the other metadata dialects |

## What a caller still owns

- **The config vocabulary.** Which block a vault declares its sites and front
  pages in is not read here, deliberately: that is a vault format's dialect, not
  a site's shape. Two applications with different config formats can compose the
  same `SiteSpec` and get the same site.
- **Spelling the gate field.** `SiteSpec::gate_field` names the field the gate
  is judged on and defaults to `AUDIENCE_FIELD` — `audience`. Which field a
  vault's disclosure control lives in is a dialect like the rest, so the value
  arrives with the spec rather than being read here.
- **Running the renderer.** This crate hands back sources and attachments.
  Turning them into pages is `plates-render`'s, with the caller choosing when
  and where.
- **Saying the warnings out loud.** A theme that would not load is returned,
  never logged.

## Status

`0.1`, and the API is expected to move before `1.0`. Known limitations, rather
than surprises:

- A page claiming a destination with `serve_at:` does not get its *relative*
  body asset references re-based onto the claimed path. Root-absolute references
  work.
- An attachment whose file has vanished is skipped rather than raised: refusing
  to build the site over one missing photograph is the wrong trade for every
  caller. Errors here are site *declaration* problems — a front page that
  resolves to nothing, an index nobody may read, two documents claiming one
  address.

## License

MIT or Apache-2.0, at your option.
