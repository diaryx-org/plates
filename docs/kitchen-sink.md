---
title: Kitchen sink
part_of: '[plates](/README.md)'
author: adammharris
audience: public
---

# Kitchen sink

Every element the default stylesheet styles, on one page, in a document that is
itself published by the engine it demonstrates.

It is two things at once. As a **demo**, it is what a page looks like with no
theme, no shell template and no custom CSS — the whole of
[`plates-render`](/plates-render/README.md)'s built-in appearance, with nothing
layered over it. As a **fixture**, it is the page to open in a light browser and
a dark one when `html_format_css.css` changes, because a stylesheet regression is
not something the test suite can see.

## Prose, and the links in it

The gate decides what leaves. This paragraph links to a sibling that is
published — [`plates-cli`](/plates-cli/README.md) — and to one that is not:
[how this workspace is organized](/about.md) declares no `audience`, so it stays
private, and the link to it renders as inert text rather than a promise that
breaks. Nothing about the source distinguishes them; the archive does.

An [external link](https://github.com/diaryx-org/plates) leaves the site
entirely. A URL written out in full is one long word with nowhere to break, and
has to be allowed to wrap anyway rather than widen the page:
<https://github.com/diaryx-org/plates/blob/main/plates-render/src/html_format_css.css>

Inline `code_span()` sits in the run of text without disturbing its rhythm, and
so does a =={green}green highlight== and a ||spoiler||, which stays covered
until it is clicked.

> A blockquote is set upright rather than italic. A pulled quote is often
> several sentences long, and a run of italic that long is harder to read than
> the text around it — so the rule and the tint do the marking instead.

## Headings

The scale runs the whole way down, because a document that reaches for `h5` has
earned it.

## Heading level two

### Heading level three

#### Heading level four

##### Heading level five

###### Heading level six

Body text after the smallest heading, which is set apart by weight, colour and
letter-spacing rather than by size — left to the browser, `h5` and `h6` come out
*smaller* than the paragraphs they introduce.

## Lists

- A list item.
- One with a nested list under it:
  - the nested item,
  - and its neighbour.
- A third item, long enough to wrap onto a second line so that the hanging
  indent can be judged at more than one line of text.

1. Ordered, for a sequence.
2. Ordered, second.

- [ ] An unchecked task.
- [x] A checked one.

## Tables

The eleven colour variables a caller may override, which is also the contract
[`appearance::ColorPalette`](/plates-render/README.md) writes:

| Variable | What it paints | Also used for |
|---|---|---|
| `--bg` | The page | Sidebar, islands |
| `--text` | Body copy | Covered spoilers |
| `--text-muted` | Breadcrumbs, captions, footnotes | Nav items at rest |
| `--accent` | Links | Focus rings, tints, task checkboxes |
| `--accent-hover` | Links under the cursor | — |
| `--border` | Table rules, code borders | Disclosures |
| `--code-bg` | Code blocks and spans | Nav hover |
| `--surface-bg` | The floating nav button | — |
| `--surface-border` | Its edge | — |
| `--surface-shadow` | Its shadow | — |
| `--divider-color` | Rules that separate rather than enclose | Footer, endnotes, nav |

A table too wide for the measure scrolls inside itself rather than widening the
page. This one does — drag it sideways:

| Grammar | Extensions | Canonical | Marked region | Highlight | Embedded island | Diaryx spellings |
|---|---|---|---|---|---|---|
| Markdown | `.md`, `.markdown` | `md` | `:::vis{.audience}` … `:::` | `=={colour}text==` | `![alt](page.html){height=400}` | rewritten before parsing |
| Djot | `.dj`, `.djot` | `dj` | `{.vis .audience}` above `:::` | `=={colour}text==` | `![alt](page.html){height=400}` | rewritten before parsing |
| HTML | `.html`, `.htm` | `html` | `<div class="vis audience">` | — | — | returned untouched |

## Code

A fenced block, tagged with its language, is coloured by the grammar that tag
names:

```rust
pub fn render_page(&self, page: &PublishedPage, site_title: &str) -> String {
    let prefix = root_prefix(&page.dest_filename);
    let breadcrumb = render_breadcrumb(page, single_file);
    format!("<!DOCTYPE html>\n<html lang=\"en\">\n{prefix}{breadcrumb}")
}
```

213 grammars ship with the renderer, which is most of what anyone fences a
block in — the languages the rest of this organisation is written in included:

```zig
pub fn parse(allocator: Allocator, source: []const u8) !Document {
    var doc = Document.init(allocator);
    errdefer doc.deinit();
    return doc; // the caller owns it now
}
```

```swift
struct EntryView: View {
    @State private var entry: Entry
    var body: some View {
        Text(entry.title).font(.headline)  // and its date below
    }
}
```

```toml
[workspace.package]
edition = "2024"
rust-version = "1.88"   # the floor the `msrv` job enforces
```

Nothing is *styled* twice over, though: the colours come from the stylesheet
rather than from the markup, so a site that replaces the sheet replaces the
palette with it, and both the light and the dark reading of this page are the
same HTML.

A tag no grammar answers to is not a failure. The block publishes exactly as it
would have before any of this existed:

```not-a-language
{ this is tagged, but with nothing anyone has a grammar for }
```

And one whose lines are longer than the measure, which scrolls horizontally
inside its own box instead of pushing the page sideways:

```
$ plates build --site docs --out _site --base-url https://example.com/docs --shell .config/shells/wide.md --force
```

## Highlights

Ten colours, spelled `==like this==` for the default and `=={colour}like this==`
for the rest:

=={red}red==, =={orange}orange==, =={yellow}yellow==, =={green}green==,
=={cyan}cyan==, =={blue}blue==, =={violet}violet==, =={pink}pink==,
=={brown}brown==, =={grey}grey==.

Each keeps the page's own text colour, so the set is readable in both modes —
the browser's own rule for `<mark>` would force near-black on all twenty
backgrounds.

## `:vis[…]` regions

The gate decides which *documents* leave; a region decides which *parts* of one
does, filtered against the same audience name.

:::vis{.public}
This paragraph is inside a region declared for `public`, which is the audience
this site publishes to, so it survives — and its marker does not. A kept region
is unwrapped, never rendered as a wrapper element you would then have to style.
:::

:::vis{.family}
This paragraph is declared for `family`. It is in the source of this document
and it is not in the page you are reading.
:::

Between that paragraph and this one, the source carries a second region — the
same shape, declared for `family` instead — and you are not reading it. The same
holds inline, where `:vis[text]{.family}` closes over its own gap rather than
leaving one in the sentence.[^grammars]

[^grammars]: All three grammars parse to the same AST node, so the predicate is
    uniform: a container named `vis`, or one whose classes contain it. Filtering
    goes through the parser rather than a text scan, which is why the code spans
    in this very section are not mistaken for real directives.

---

That was a horizontal rule, which separates without enclosing — the one place
`--divider-color` is doing visible work in the body.
