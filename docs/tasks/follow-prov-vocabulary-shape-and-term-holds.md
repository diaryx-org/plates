---
title: Follow prov's vocabulary shape and term holds
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
author: adammharris
created: 2026-09-28
updated: 2026-09-28
audience: public
---

# Follow prov's vocabulary shape and term holds

prov's next release changes two things plates reads, and plates does not
compile against it until both are followed. Neither is released yet: the
commits are prov's `ca99ba6` and `6953dfd` on `main`, and this task starts
when a prov version carrying them is on crates.io.

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

## The work

1. Move the `prov` pin in `Cargo.toml` to the release carrying both commits.
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

## Done when

plates builds and its CI passes against the new prov, a site whose export
holds on `status` leaves a `status: draft` page out of the build and lists it
as held, and `reify` appears nowhere in plates outside `docs/proposals/`.
