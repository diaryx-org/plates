---
title: plates-cli
part_of: '[plates](/README.md)'
audience: public
---

# plates-cli

The `plates` command: a static site generator over a
[`prov`](https://github.com/diaryx-org/prov) archive.

```
cargo install plates-cli
```

installs `plates`, which finds the archive by walking up from the current
directory the same way `prov` does. The crate is `plates-cli`; the installed
command is `plates`.

This is the application layer, and the only one in
[the workspace](https://github.com/diaryx-org/plates) allowed to have the
opinions a library must not: **where a build lands, what a site declaration is
spelled like, and when to build**. What a site *is* lives in
[`plates`](https://github.com/diaryx-org/plates/tree/main/plates); what a page
*looks like* lives in
[`plates-render`](https://github.com/diaryx-org/plates/tree/main/plates-render).
Another application over the same archive format replaces this crate and keeps
the other two.

## The four verbs

| | |
|---|---|
| `plates build` | Render every site into `_site`. |
| `plates watch` | The same, then again on every change. |
| `plates serve` | A dev server on `http://127.0.0.1:4321`, each site under its own name, reloading when the archive moves. |
| `plates clean` | Remove what a build wrote. |

They are four views of one build. `build` writes it to a directory, `watch`
keeps writing it, `serve` hands it to a browser, and `clean` takes back exactly
what `build` put down — with one collector and one renderer underneath all four,
which is the property that stops a preview and a deploy drifting apart.

| Flag | Verbs | |
|---|---|---|
| `-C`, `--root DIR` | all | Run as if `plates` had started in `DIR` — the `git -C` model, entered once before anything resolves. Also `PLATES_ROOT`. |
| `-o`, `--out DIR` | `build`, `watch`, `clean` | Where the site lands. Defaults to `_site`; also `PLATES_OUT`. |
| `--site NAME` | `build`, `watch`, `serve` | Render one site. `build`/`watch` write it *at* the destination root and `serve` answers it at `/`, because someone who named one site asked for one site. |
| `--base-url URL` | `build`, `watch`, `serve` | The absolute URL the finished site will live at. |
| `--force` | `build`, `watch`, `clean` | Write into (or empty) a directory holding files no build of ours wrote. |
| `--host`, `-p`, `--port` | `serve` | Defaults `127.0.0.1` and the first free port from 4321, so a second archive served alongside the first just works. |
| `--open` | `serve` | Open the site in a browser once it is up. |

`--base-url https://example.org` is what canonical links, the sitemap,
`robots.txt` and the feeds are written against. Without one they are skipped,
which is the right default for a preview whose address is `localhost`.

`_site` is underscore-prefixed by the static-site convention, which is not
decoration: it sorts away from the archive's own directories, and the hosts that
auto-publish a repository skip it, so a build committed by accident does not
become a second copy of the site.

## Declaring a site

`plates` takes a `SiteSpec` already built and never reads a config vocabulary —
which block an archive declares its sites in is a dialect, not a site's shape.
The spelling below is this crate's, and lives here alone, so two applications
over one archive format can disagree about it and still produce the same website.

It is read from the same two surfaces prov reads its own config from, in the same
order: the root document's frontmatter first, then the config document the root
links to, which wins. So a declaration sits beside prov's own `views:` and
`exports:`:

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
    syntaxes:                    # grammars for languages the built-in 213 miss
      - .config/sites/blog/wat.sublime-syntax
```

| Key | |
|---|---|
| `audience` | **Required.** The gate: a site that does not say who it is for is not a site. |
| `label` | What a person calls it. Defaults to the name, humanized. |
| `view` | A prov view, by its key under `views:`. Its arrangement becomes the site's; absent, the gate's whole set arranged by containment. |
| `index` | The front page, as a link resolved through the spanning relation — so it survives a rename, a move and a retitle. Absent, an index is synthesized from the site's entries. |
| `shell` | An HTML file with named slots, as an archive-relative path. `.config/sites/<name>/` is the recommended home, not a requirement. |
| `stylesheet` | A CSS file that *replaces* the built-in sheet rather than layering over it. |
| `lang` | BCP 47, for every page's `<html lang="…">`. Defaults to `en`. |
| `syntaxes` | `.sublime-syntax` files for languages the built-in grammars do not cover, as archive-relative paths. A list, or a bare path for the one-item case. |

Everything but `audience` has a defensible default, and the defaults are
`plates::SiteSpec`'s. A misspelled key costs a site one setting and is reported;
refusing to build the other four sites over it would be a worse answer.

### The fallback, and why it is not a guess

An archive that declares no `sites:` block gets one site per prov `exports:`
entry gated on `audience` — same name, same label, same view, same gate value.
That is not plates inventing a site: an export already *is* a named, closed set
of documents that may leave the archive, which is the whole of what a site needs
to exist. What it lacks is only the render-facing half — a front page, a shell, a
stylesheet — and every one of those has a default.

An export gated on some *other* field is skipped rather than published under a
rule nothing showed anyone, and named in the warnings so the omission is visible.

## What a build remembers

A build records every path it wrote in `.plates-build` at the destination root.

Two problems, one answer. A rebuild after a document is deleted, renamed or taken
off a site leaves the old `.html` sitting in the destination, and it will be
deployed alongside the new ones: a page that is no longer part of the site, still
reachable, still in the sitemap of whatever indexed it. And `clean` given a
directory has no way to tell the site it built from a directory somebody typed
one character wrong.

So the next build removes what the last one wrote and this one did not, and
`clean` removes exactly the listed set. **Nothing ever deletes a file no build of
ours recorded writing** — which is what makes it safe to point `--out` at a
directory that also holds something else, and why `clean` refuses a destination
with no record rather than guessing. `--out` typed one character wrong is not a
way to delete somebody's work.

It is a dotfile because it ships with the site: a build directory *is* the
deployable artifact, and a memory kept somewhere else would be gone at exactly
the moment the next build needed it. Every static host and every web server
already declines to serve dot-prefixed files, so the record of the site is not
part of the site.

## `plates serve`

Three threads' worth of machinery, and no more. A **builder** thread owns the
archive — every prov future is `!Send`, so the workspace never crosses a thread
boundary; what crosses is the finished bytes. It re-opens the archive for each
build rather than reusing one, because a rebuild has to see files that did not
exist when the server started, including a changed config document, which is
where views and sites are declared. The **accept loop** hands each connection to
a short-lived thread serving from the current snapshot, so serving never blocks
on a build and a build never blocks a request; a request mid-rebuild is answered
from the previous snapshot, which is what a static host would do.

**Change detection** is a stat walk compared against the last one — no
filesystem-watch dependency, since the platform APIs disagree about what an event
is and need a fallback poll anyway for the network and synced volumes an archive
often lives on, and the whole of what is wanted here is one bit. It runs only
while a browser is actually watching, so an unattended `plates serve` costs
nothing, which matters when the archive is tens of thousands of files rather than
a blog.

Served HTML carries one thing a built page does not: a small script that polls a
build number and reloads when it moves. It is injected at serve time and never
written into a build, so `plates build` output is byte-identical to what the
server renders.

Attachments are never pulled through memory to render a page of text: collection
is told they are already accounted for and carries each one's path, length and
MIME type forward, so `build` copies them and `serve` reads one when a browser
asks for it.

## Features

| Feature | |
|---|---|
| `yaml` *(default)* | `---` frontmatter, `registry.yaml` |
| `json`, `toml`, `fig-lang` | the other metadata dialects |

Forwarded to `plates` and `plates-render`, which forward to `prov`, which
forwards to `fig`. With a format off, its parser is left out of the build and
`prov` stops recognizing it, so at least one must be on. `plates-render` is taken
*with* its `templating` feature here: running the render pipeline is the caller's
job, and this is the caller — without it, a body's template directives would be
published as literal text.

`prov` is deliberately not a dependency of this crate. Workspace discovery, the
config document and the id registry are all reached through `plates::prov`, which
`plates` re-exports for exactly this reason: one prov in the tree, and no way for
this crate to resolve a different version of the `Workspace` it hands to
`collect_site`.

## License

MIT or Apache-2.0, at your option.
