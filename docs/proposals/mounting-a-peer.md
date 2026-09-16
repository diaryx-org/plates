---
title: 'Proposal: mounting a peer'
part_of: '[Proposals](/docs/proposals/proposals.md)'
status: implemented
author: adammharris
created: 2026-09-15
updated: 2026-09-15
audience: public
---

# Mounting a peer

## Summary

A site built from one archive can carry the sites of the archives it names. When
a published page draws a foreign spanning edge — `[fig](id:fig/b9j9zgk)` — into
a workspace the device knows where to find, plates opens that workspace, plans
the export there that answers to the same gate, and **mounts** what it collects
under the peer's name: `/fig/`, `/prov/`, `/twig/`. Each peer keeps building its
own site at `/` exactly as before; the mounting site is the union, and the
mount point is the same page a reader of the peer's own site would land on.

Nothing new is declared. The edge is the ordinary foreign reference prov
already carries, the peer's site is the export it already has, and where the
peer lives is the map `prov peer add` already writes. What is new is one rule —
*a followed edge from a published page mounts the peer's export for the same
audience at `/<name>/`* — and the collection that applies it.

## The shape it serves

An organization with one front door and many repositories. `diaryx.org` is one
site: a front page and an About page that belong to the organization, and a
page each for `fig`, `prov`, `twig`, `leaf`, `flower` and `historica` that
belong to those projects. Today all eight pages live in one repository's `www/`
directory, so the project that owns `fig` cannot edit its own page without a
commit to a repository it does not otherwise touch, and the page describing a
release ships on whoever next publishes the app.

The org repository is already a prov workspace whose root names each
repository as a sub-workspace by foreign reference, and each repository is
already a workspace that names its root's parent as the org. The dependency
direction is already written down. This proposal lets the site follow it.

## What a mount is

1. **An edge from a published page.** The origin site is planned as today: a
   gate, a view, a front page. Its spanning tree is then walked with
   `prov::descend`, and a foreign leaf is a mount candidate only when the page
   that wrote the edge is one the site publishes — an entry, or the front page.
   A private page's foreign edge mounts nothing, for the same reason its path
   links publish nothing: the site's outline reaches the peer through a
   published page or it does not reach it.

2. **A confirmed peer.** Only a workspace the device's peer map names *and*
   that calls itself by that name is followed — prov's `Trust::Confirmed`, the
   default. An unknown, unconfirmed or mismatched peer is a warning naming the
   refusal, and the edge stays what an unfollowed edge already is: a link to a
   page this site does not publish.

3. **The export that answers to the gate.** In the peer's own config, the
   export whose gate field and value equal the origin site's — preferring one
   with the origin site's *name*, then the only one — is the site to mount. A
   peer with no such export mounts nothing, with a warning. The audience name is
   the whole contract between the two workspaces, which is the contract prov's
   gate already makes: a document declares who may read it, and the value
   travels with the file.

4. **Planned and collected as its own site.** The peer's export is planned by
   `plan_site` against the *peer's* workspace, with the peer's views, term node
   and front page, and collected by `collect_site` with the peer's own anchor —
   the same calls `plates build` inside the peer makes. What comes out is the
   peer's site, in the peer's site coordinates.

5. **Prefixed by the peer's name.** Every collected coordinate — source path,
   destination, attachment key, inbound and outbound edge, outline node — is
   prefixed with `<name>/`. The peer's front page lands at `<name>/index.html`.
   A `serve_at:` claim in a mounted page is **not** prefixed: it is written from
   the site root, and a page that claims `/privacy` means the mounting site's
   `/privacy`. Two pages claiming one destination across a mount are refused
   as they are within one site.

6. **One outline.** The origin's spanning tree with the followed leaf replaced
   by the peer's collected outline, so the render's navigation shows the
   mounted pages where the edge was drawn.

The peer's *theme* is not mounted. One site, one frame: the origin's shell,
stylesheet, header and footer surround every page, mounted ones included. A
peer's page-level `shell:` is not honoured on a mount in this first cut, and
says so in a warning. This is the smaller of two wrong answers — a site that
changes chrome at `/fig/` is not one site — and a peer's own build still
renders it in its own shell.

## Links across the boundary

A link from an origin page to a peer page is a foreign reference, `id:fig/…`.
The render layer resolves links by *path*, and a foreign id is not a path in
any workspace it can see. Collection therefore rewrites id-form body links —
local `id:<id>` and foreign `id:<peer>/<id>` alike — to site-root-absolute
path links to the target's collected source (`/fig/index.md`), which the
render already resolves and rebases to the page's depth. The map is built from
the origin's registry and each mounted peer's registry, in site coordinates,
before any body is collected.

This is a change for single-workspace sites too, and a deliberate one. An
`id:` link in prose used to reach the render untouched and ship as a dead
`href="id:…"`; now it resolves to the page when the page is published, and to
the same unclickable span every unpublished path link becomes when it is not.
A vault whose reference style is `target: id` gets working prose links for the
first time.

Links written from a peer page *upward* — a breadcrumb to the org's About
page — are `id:org/…` from the peer's point of view, and resolve through the
same map when the org is the origin. Inside the peer's own build they are
foreign references to a workspace the build did not mount, and strip as
unpublished, which is correct: the peer's site has no About page.

## What this is not

- **Not a merge of archives.** Nothing is written across the boundary, no
  registry is read as if it were the origin's, and a mounted document's
  identity stays the peer's. `CollectedSite` gains no new field; a mounted
  source is a `SourceFile` whose path happens to start with the peer's name.
- **Not recursive by accident.** A mounted peer's own foreign edges are
  followed to the depth the caller asked for, prov's `Descent::depth`, counted
  in crossings. The default is prov's. A peer that names the origin back is a
  cycle prov already refuses.
- **Not a change to what leaves.** The gate is still the only thing that admits
  a document. A mount can only add documents the *peer's* gate admits to the
  *same* audience, and a peer's private page is exactly as unpublished under a
  mount as under its own build.
- **Not a URL scheme.** `/<name>/` is the workspace's declared name, which is
  what a foreign reference is spelled with, so a reader who sees `id:fig/…` in
  a source and `/fig/` in the address bar is looking at one fact twice.

## The cost

A `CollectOptions` field (`id_links`), a new module in `plates` for the
federated collection, a `--follow[=DEPTH]` and `--peers FILE` on the CLI's
three verbs, and `prov::PeerFile` upstream so the CLI and any other host read
the same map. The single-workspace path through `collect_site` is unchanged
except for the id-link rewrite, which is the behavioural change above.

## Status

Implemented 2026-09-15, as argued: `plates::mount::collect_mounted` over
prov's `descend`, `CollectOptions::{id_links, mount}`, `plates --follow`, and
`prov::PeerFile` upstream. The diaryx.org site is the first mounting site;
`fig`, `prov`, `twig`, `leaf`, `flower` and `historica` are its first peers.
