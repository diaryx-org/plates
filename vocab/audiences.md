---
title: Audiences
part_of: '[plates](/README.md)'
contents:
- '[Public](/vocab/public.md)'
---

# Audiences

Who a document in this archive may be published to. The `audience:` field is
closed against this list (`fields.audience` in `prov.yaml`), so a value that is
not a term below is a `prov check` finding rather than a document that quietly
stops publishing — which is the posture a field wants when a typo in it is a
disclosure bug.

The vocabulary is *reified*: each term is a document rather than a row, so it
has a body to say who the readership is, backlinks saying what is published to
them, and somewhere to hang the settings a site rendered for them is built with.
`plates` reads the front page and the `site:` block off the term node; nothing
else here is machinery.

Nothing in this directory declares an `audience:` of its own, so none of it is
published. An archive that wants its audiences described on its own site opts in
per term, deliberately.
