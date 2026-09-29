---
title: Follow prov's vocabulary shape, term holds and field stamps
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
author: adammharris
created: 2026-09-28
updated: 2026-09-28
audience: public
---

# Follow prov's vocabulary shape, term holds and field stamps

prov's next release changes three things plates reads, and plates does not
compile against it until they are followed. None is released yet: the
commits are prov's `ca99ba6`, `6953dfd` and `cee829b` on `main`, and this
task starts when a prov version carrying them is on crates.io.

## What changed in prov

**A vocabulary's store says whether it is reified.** `FieldSpec::reify` is
gone. A `vocabulary:` pointer at a document carrying the `vocabulary:` marker
is a flat `terms:` store; any other document is a reified index whose
spanning children are the terms. `Workspace::vocabulary_shape` answers which,
and a `reify:` key still written in a config is ignored.

**A vocabulary term can hold a document back.** An export's `hold` field now
also holds a document whose value there is a term marked `holds: true` in the
vocabulary governing it — `hold: status` keeps a `status: draft` page home.
`prov::exports::plan` and `compose` take the answer as a required
`&TermHolds`, so a caller that never asks the vocabulary cannot publish every
draft by accident. `Workspace::export_plan` is the whole plan with it in.

**A field declares its own stamp.** The top-level `updated:` and `created:`
config keys are no longer read; the stamped fields are declared as
`fields.<name>.stamp: edit` and `create`, and `WorkspaceConfig` answers
`updated_field()` and `created_field()`. A workspace still writing the old
keys stamps nothing.

## The work

1. Move the `prov` pin in `Cargo.toml` to the release carrying all three
   commits.
2. [`plan.rs`](/plates/src/plan.rs) calls `prov::exports::plan(ws.graph(), …)`;
   call `ws.export_plan(root_doc, &to_export(spec), views)` instead, so a site
   and every mounted peer hold drafts back by term as well as by `true`.
3. [`term.rs`](/plates/src/term.rs) filters on `spec.reify` before reading a
   term node. Drop the filter: `reified_term_path` already finds no node in a
   flat store. Where the reason matters — the [unread site
   declaration](/docs/tasks/unread-site-declaration.md) task tells a field with
   no reified vocabulary apart from a term with no node — ask
   `ws.vocabulary_shape`, and reword that task's `reify: true` remedy as
   "point the field at an index of term documents".
4. Remove `reify: true` from plates' own `prov.yaml`, the example configs in
   [`plates-cli/README.md`](/plates-cli/README.md) and
   [`plates-cli/src/config.rs`](/plates-cli/src/config.rs), the wording in
   [`plates/README.md`](/plates/README.md), and the test fixtures in
   `mount.rs` and `collect.rs`. The proposals keep theirs; they are the
   record of what was argued.
5. Say in the site spec's `hold` documentation ([`spec.rs`](/plates/src/spec.rs))
   that a term can hold as well as the literal `true`.
6. Replace the top-level `updated: updated` and `created: created` in plates'
   own `prov.yaml` with `fields.updated.stamp: edit` and
   `fields.created.stamp: create`, so the repository keeps stamping.
7. A page's dates are read by literal name — `created` and `updated` in
   [`plates-render/src/site.rs`](/plates-render/src/site.rs) — where the
   workspace now says which fields it stamps. Read them through
   `created_field()`/`updated_field()`, keeping the literal names as the
   fallback for a workspace that declares no stamp, so an archive stamping
   `modified` shows its dates.

## Done when

plates builds and its CI passes against the new prov, a site whose export
holds on `status` leaves a `status: draft` page out of the build and lists it
as held, `reify` appears nowhere in plates outside `docs/proposals/`, plates'
own `prov.yaml` declares its stamps on the fields, and a page in an archive
stamping `modified` shows that date.
