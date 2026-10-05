---
title: 'An id embed of a card draws its payload'
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
author: adammharris
created: 2026-10-05
updated: 2026-10-05
audience: public
---

# An id embed of a card draws its payload

## The problem

An attachment is a node: its sidecar (`photo.jpg.yaml`) carries the title, the
id, the place in the tree, and its `content:` names the payload (`photo.jpg`).
A body can embed the picture two ways. By a path to the payload,
`![](photo.jpg)`, which publishes today. Or by the card's id, `![](id:abc1234)`,
which names the node — and an embed of a node that is an attachment means the
bytes it stands for, not the node's YAML or the page plates renders for it.

plates reads the id form as a link to a page. In
[`collect`](/plates/src/collect.rs), `rewrite_id_links` asks
[`RegistryLinks::resolve`](/plates/src/collect.rs) for the id and gets the
card's own source path back (`/trip/photo.jpg.yaml`), so the collected body
says `![](/trip/photo.jpg.yaml)`. The reference scan then reaches
`push_canonical_ref` with that path, finds it among the site's `pages` (a card
publishes as a page of its own since attachments became pages) or among the
paths whose extension is a document's, and collects nothing. The published
page draws an `<img>` whose source is a page or a YAML file, and the payload is
not shipped at all unless the card happens to be published itself.

A path link to the card, `[the scan](scan.pdf.yaml)`, has the same shape: it
is right for a link (it lands on the card's page) and wrong for an embed.

Found by reading, not yet by a build: the host this came from now treats an
id reference to a card as a reference to its payload everywhere else (its
tray, its share bundles), and prov is gaining a resolver for exactly this
question — see prov's `resolve_payload` / `attachment_payload` on the
`attachments-as-nodes` branch.

## Repro

A vault with `trip.md` publishing to `public`, holding the card
`trip/photo.jpg.yaml` (`attachment: true`, `content: photo.jpg`, no
audience of its own) and its payload `trip/photo.jpg`, both registered, and
`trip.md`'s body:

```markdown
![](id:<the card's id>)
```

Build the `public` site. Expected: `trip.html` draws `photo.jpg`, and
`trip/photo.jpg` is among the shipped attachments. Seen (by the code path
above): the image source is the card's path, and no attachment is shipped.
The same body written `![](photo.jpg)` publishes correctly.

## Done when

- An image embed whose target resolves — by `id:` or by path — to an
  attachment sidecar is rewritten to the payload's site path, and the payload
  is collected as an attachment, gated as any body-referenced payload is.
- A plain link (`[…](id:…)`, not `![…]`) to a card still lands on the card's
  page when the card is published, and on the payload when it is not.
- Tests for both, with the card published and unpublished.
