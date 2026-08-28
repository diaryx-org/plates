---
title: 'Proposal: a site is an export'
part_of: '[plates](/README.md)'
status: implemented
author: adammharris
created: 2026-08-27
audience: public
---

# A site is an export

## Summary

Delete the `sites:` block. A site **is** a `prov` export, read from `exports:`
with nothing renamed and nothing re-derived. The render-facing half that has no
home in an export — the shell, the stylesheet, the language, the extra grammars
— moves onto the `audience` vocabulary's **term node**, where `prov`'s own spec
already says an audience's config belongs. And the front page stops being a
config string: with the vocabulary reified, an audience is a real node, so its
index is an **edge** from that node to a document, maintained and checked like
every other relation.

The result is that `plates` declares no config vocabulary at all. What a site is
becomes a question the archive answers.

## What is redundant

`sites.<name>` carries nine keys. Four of them, plus the entry name, are
`prov`'s `EXPORT_KEYS` under different spellings:

| `sites.<name>` | `exports.<name>` |
|---|---|
| *(entry name)* | *(entry name)* — a path segment in both, deliberately not the gate value in both |
| `label` | `label`, down to the humanize-the-name fallback |
| `audience` | `gate.value` |
| `gate_field` | `gate.field` |
| `view` | `view` — same key, same referent under `views:` |
| `index` | — refused by name |
| `shell`, `stylesheet`, `lang`, `syntaxes` | — |

The gate is reshaped rather than merely renamed: `prov` requires both halves and
offers no default and no shorthand, where `plates` splits the pair into a
required `audience` and an optional `gate_field` defaulting to `audience`. That
one difference is a default, and it is the whole reason a second block existed
for the gate at all.

The five uncovered keys are uncovered on purpose. `prov-exports` names `index:`
specifically and declines it: which page greets a reader is a rendering
decision, an export is a set, and "a publish layer that wants one declares it in
its own block, where its render exists" (`prov-exports/src/spec.rs`). The other
four fall out the same way — an export's consumer is a copy-out, which has no
shell to wear.

## Why the render keys cannot simply move into `exports:`

`prov` lints unknown keys inside an export entry: `ExportIssueKind::UnknownKey`,
non-fatal but reported, offering `EXPORT_KEYS` as the accepted spellings
(`prov-exports/src/lint.rs`). Writing `exports.blog.shell:` would make every
`prov check` emit a config issue for as long as the key existed.

This is the mirror image of why `sites:` sits at top level. `SITES_KEY` was
chosen to be far enough from `prov`'s own axes that the near-miss linter reads
it as a key belonging to someone else (`plates-cli/src/config.rs`). Top level is
unlinted; inside `prov`'s block is not. So the render half has to live
somewhere that is neither.

## The design

### 1. The gate half is read from `exports:`

`read_sites` and `specs_from` go away. `SiteSpec` survives as the library's
internal shape — the argument for it in `plates/src/spec.rs` is about the crate
API, not the config surface, and stays true — but it is built from an
`ExportSpec` plus the term node's payload rather than from a parallel block.

`specs_from_exports` stops being a fallback and becomes the only path. Its
restriction — skip any export whose `gate.field` is not `audience`, and warn —
is **deleted**. That rule exists only because a derived site had no way to say
`gate_field`; reading exports directly, a gate on `clearance` is not a special
case, it is the gate.

### 2. The audience field declares a reified vocabulary

```yaml
prov:
  fields:
    audience:
      values: closed
      vocabulary: '[Audiences](/vocab/audiences.md)'
      reify: true
```

`closed` is the point as much as `reify` is: it makes `audience: pubic` an
`UnknownTerm` finding from `prov check` on the field where a typo is a privacy
failure. The `sites:` block never checked its own audience value against
anything.

### 3. A term node carries the render half

Reified, the vocabulary is an index node whose `contents` are term nodes, each
an ordinary content document with a stable id, a prose body, and backlinks
(`prov/docs/spec.md` §3). The render keys ride on the term node as tier-3
payload — which is precisely the case `prov`'s spec names when it explains why
term payload is carried and never read: "how a diaryx audience hangs gate/theme
config off a term prov still validates membership in."

```yaml
# vocab/audiences/public.md
---
title: Public
part_of: '[Audiences](/vocab/audiences.md)'
term: public
front_page: '[plates](id:7f3a91c)'
shell: .config/sites/blog/shell.html
stylesheet: .config/sites/blog/style.css
lang: en
syntaxes:
  - .config/sites/blog/wat.sublime-syntax
---

Anyone; safe to publish. Everything here has left the archive on purpose.
```

The prose body is not decoration. An audience is the one piece of vault config
whose meaning is a judgment about people, and a term node is somewhere to write
that judgment down next to the setting it governs.

A term node is a content node, so it is in the census and therefore a candidate
for a gate. It declares no `audience:` of its own, so it is published to nobody
— the default, and the right one. An archive that wants its audiences described
on its own site opts in per term, deliberately.

### 4. The front page is an edge

```yaml
prov:
  relations:
    front_page:
      cardinality: one
      inverse: fronts
      means: 'the page that greets a reader of this audience'
```

The term node writes `front_page:` and `prov` maintains `fronts:` on the page.
Either end may be authored — the inverse is maintained in both directions — so a
document can equally claim its own role, which is the spelling that travels with
the file if it is ever moved to another vault.

Resolution is what `resolve_index` already does (`plates/src/plan.rs`): the
target resolves through `prov`'s link layer, so a rename, a move and a retitle
all survive. What is new is everything around it:

- **It is linted.** A missing inverse and a broken target are `prov check`
  findings. Today a front page that stops existing is `plates`' private
  diagnosis, discovered at build time by whoever happens to run a build.
- **It is id-addressable and backlinked** — the term node's backlinks say what
  fronts it without a scan.
- **Templates already see it.** `relations` and `inbound` landed in the page
  context in `1047cb0`. The front-page edge appears there with no new plumbing.
- **It is not containment.** `front_page` is not the spanning relation, so
  nothing about the front page changes where a document is filed.

## What this costs

**Render config keys on the term, not the export.** `prov` allows one gate to
carry two exports — the same `audience: public` behind two different views, a
full site and a photo index. Under `sites:` those wear different shells; under
term payload they cannot. This proposal accepts the limitation rather than
reintroducing a per-export block to dodge it, on the grounds that the case is
hypothetical here and the block is the thing being deleted. If it turns out to
be real, the answer is a `prov` change that opens `exports.<name>` to declared
payload, not a second config surface in `plates`.

**Custom styling now requires a vocabulary.** Today four lines under `sites:`
buys a stylesheet. After, the vault needs `fields.audience.vocabulary` and a
term node before there is anywhere to hang one. Defaults still cover a vault
that declares nothing, so this bites only the vault that wants exactly one
custom thing — and what it gets in exchange is a checked audience value.

**`plates` loses its dialect.** `plates-cli/src/config.rs` argues that the
spelling lives in the binary on purpose, so a different host over the same
archive could replace it wholesale and still produce the same website. That
property goes: after this, the archive itself says how a site is spelled. This
is a real trade and the proposal makes it knowingly — one vocabulary that
`prov` checks is worth more than two that agree by convention.

## Prerequisite: `reify` is declared in `prov`, and implemented nowhere

This proposal cannot ship against `prov` 0.9.2. `reify` is parsed
(`prov-config/src/config.rs`), round-tripped back out, diagnosed as a bool axis,
and documented in both `DESIGN.md` and `spec.md` — and no code path reads the
field. Concretely, pointing `fields.audience.vocabulary` at a reified index node
today does one of two broken things:

- **If the index node carries no `vocabulary:` marker**, `Vocabulary::from_meta`
  returns `None`, so `vocabulary_findings` produces nothing and term checking
  silently stops on the closed, privacy-critical field. That is the fail-open
  direction. Meanwhile `store_findings` pushes every `fields.*.vocabulary`
  pointer onto its store list **without consulting `reify`** and emits
  `MalformedStore` for the markdown carrier (`prov/src/validate.rs`), so every
  `check` reports a problem that is not one.
- **If it carries the marker but no `terms:`**, `from_meta` returns a vocabulary
  with an empty term set, and on a closed field every `audience: public` in the
  archive becomes an `UnknownTerm`.

So the sequencing is: `prov` first, `plates` after. The `prov` change is three
things.

1. **A reified loader.** `load_vocabulary` gains the branch for `reify: true`:
   resolve the pointer to a content index node and read its `contents` children
   as terms. The term key needs deciding — proposed: an explicit `term:` field,
   falling back to the node's `title`, so a term can be retitled for a reader
   without silently renaming the value every document declares. `id` is the
   node's own prov id rather than a key in a map, which is the whole point of
   reifying. `retired:` stays a field, on the node.
2. **`store_findings` must not require a whole-file carrier for a reified
   vocabulary.** A reified vocabulary is content, not machinery, so the
   whole-file store rule does not apply to it. It gains something better in
   exchange: term nodes are in the reachable set, so they are orphan-checked,
   which a machinery store deliberately is not.
3. **`spec.md` §4's target-kind table needs a row.** A flat vocabulary is
   machinery — reached one-way from the root, no inverse, no `part_of`, not in
   the spanning tree. A reified one is the opposite on every count. The table
   currently describes only the first, and the "controlled term" row's contract
   ("resolved against a vocabulary store, checked — not traversed") stops being
   true when the terms are nodes.

None of that is large, but all of it is `prov`'s, and it is a spec change rather
than an implementation detail — which is the right place for it, since what is
being settled is what a reified vocabulary *is*.

## Migration

`plates` 0.2 reads both surfaces: a `sites:` block still builds sites and warns
that it is deprecated, naming the export and term node that would replace it.
`plates` 0.3 removes the block, `SITES_KEY`, `SITE_KEYS`, `read_sites`,
`specs_from` and the `Source::Declared`/`Source::Exports` distinction, which
stops being a distinction.

An archive with no vocabulary at all keeps working throughout: exports become
sites, every render key takes its default, and the front page is synthesized
from the site's entries exactly as it is today when `index:` is absent.

## Open questions

- **The term key.** `term:` with a `title` fallback is proposed above; the
  alternative is the node's filename stem, which is one less thing to write and
  one more thing that breaks on a rename.
- **Should a term node's own body be the front page?** It would save a document
  for the simplest site. Proposed answer: no. A front page is a page *of* the
  site, and a term node is configuration that happens to be content — conflating
  them means an audience cannot be described without publishing the description.
- **`label`.** An export has one and so does a term node. The export's wins, on
  the grounds that the site's name is the site's, but this is worth a second
  look — the term is where a reader-facing name would naturally be written.

## Amendments

What shipped, and where it differs from the design above.

### The render keys are namespaced under `site:`

A term node carries them as one mapping — `site: {shell, stylesheet, lang,
syntaxes}` — rather than as the bare top-level keys the example in §3 writes.

Tier-3 payload is unnamespaced by nature: a term node's frontmatter is the
author's, and prov carries every key on it without claiming any. So a bare
`shell:` is plates' by convention only. It collides with a field the archive
already had, and with the second consumer that wants one — and the collision is
silent, because neither side is wrong. One obviously-someone's-block key mirrors
how prov's own config nests under `prov:` in a document, and costs a line.

`front_page:` stays at top level and is exempt on principle rather than by
exception: it is not payload but a **relation**, and a declared relation is its
own namespace registration. In prov 0.10 declaring one is all-or-nothing — a
`relations:` block replaces the `contents`/`part_of` preset rather than
extending it — so the edge costs restating the whole vocabulary, which is what
this repository's own `prov.yaml` now does: `front_page`/`fronts` declared
beside the four defaults, both halves authored, and a broken front page is a
`prov check` finding here exactly as §4 promised. An archive that does not want
to spell out its vocabulary carries `front_page` as an ordinary field and gets
plates' resolution without prov's lint — an additive way to extend the preset
would make the edge cost one stanza instead of seven, and is worth lifting in
prov.

The keys plates reads inside `site:` are `plates::TERM_SITE_KEYS`, and one that
is not costs the site a setting and a warning.

### 0.2 ships exports-as-primary; `sites:` is deprecated, not deleted

The migration in §"Migration" is split. **0.2** — this change — makes `exports:`
the ordinary path, deletes the rule that skipped an export gated on some field
other than `audience`, and reads the term node. A `sites:` block still wins
outright where one exists, and every site it declares now names the export entry
and the term node that replace it, and the version it stops working in.

**0.3** removes the block, `SITES_KEY`, `SITE_KEYS`, `read_sites`, `specs_from`
and the `Source` distinction.

Two smaller resolutions, both as proposed: the export's `label` wins, and a term
node's own body is not the front page.

### The prerequisite landed in prov 0.10.0

`reify` is implemented rather than merely parsed. What plates reads it through is
`Workspace::reified_term_path(root_doc, pointer, term)` — term value in, the path
of the node declaring it out, with the term key `term:` falling back to `title:`
and a retired term still returning its path. `Workspace::load_reified_vocabulary`
is the membership half, and `check` no longer reports `MalformedStore` for a
reified vocabulary's markdown index node.

Front-page links are resolved against the **term node**, which is where they are
written — `front_page: '[Home](daily.md)'` on `vocab/public.md` means
`vocab/daily.md` — and travel to the planner in prov's root-absolute spelling, so
one resolution happens rather than two that could disagree.
