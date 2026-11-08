---
title: An Atom summary is escaped twice
part_of: '[Tasks](/docs/tasks/tasks.md)'
created: 2026-11-08
status: open
author: adammharris
audience: public
updated: 2026-11-08T07:45:09.510170Z
description: The Atom feed builds an entry’s <summary> by stripping tags from the rendered HTML, which keeps its entities, and then escapes the text again, so “Tom & Jerry” reaches a feed reader as “Tom &amp; Jerry”
---

# An Atom summary is escaped twice

## The bug

`generate_atom_feed` in `plates-render/src/page.rs` writes each entry's
`<summary>` from `strip_html_truncate(&page.rendered_body, 280)`, passed
through `xml_escape`. The rendered body is HTML, so its text already holds
`&amp;`, `&lt;` and `&gt;` for every `&`, `<` and `>` the author wrote.
`strip_html_truncate` drops the tags and keeps those entities as they are, and
`xml_escape` then escapes their `&` again. A feed reader decodes once and
shows the entity, not the character.

The `<content>` beside it is not affected: it is the rendered HTML inside
CDATA, decoded once by the reader as HTML. The RSS feed writes no summary.

## Repro

A page whose Markdown body is

```markdown
Tom & Jerry <3
```

renders (`render_body`, Markdown) to `<p>Tom &amp; Jerry &lt;3</p>`, and its
Atom entry carries

```xml
<summary>Tom &amp;amp; Jerry &amp;lt;3</summary>
```

which a reader shows as `Tom &amp; Jerry &lt;3`. Checked on `main` at
134b3b0 by calling `generate_atom_feed` with one page whose `rendered_body`
is that HTML.

## Beside it

`strip_html_truncate` says it truncates to `max_len` characters, and compares
`text.len()`, which counts bytes, so a summary in a script outside ASCII is cut
shorter than one in English. Its cut can also land inside an entity
(`&am`), which, once the entities are decoded first, stops mattering.

## Done when

The summary's text is the body's text decoded once, then escaped once: the
repro's entry reads `<summary>Tom &amp; Jerry &lt;3</summary>`. A test in
`page.rs` holds that, and the 280 counts characters, as its doc comment says.
