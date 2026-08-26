//! Site layout helpers: root-relative prefixes, percent decoding, rewriting
//! internal `.md` links to their published `.html` targets, and absolutizing a
//! rendered body for consumers that read it away from the site.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Compute the relative prefix to get from a page back to the site root.
///
/// `index.html` → `""`, `a/b.html` → `"../"`, `a/b/c.html` → `"../../"`.
pub fn root_prefix(dest_filename: &str) -> String {
    let depth = dest_filename.matches('/').count();
    if depth == 0 {
        String::new()
    } else {
        "../".repeat(depth)
    }
}

/// Rewrite internal `.md` hyperlinks in rendered HTML to their published
/// `.html` destinations, resolving relative/workspace-root paths via the
/// `path_to_filename` map. External links, anchors, and non-`.md` hrefs are
/// left untouched.
///
/// The lookup key is sanitized the same way `path_to_filename`'s keys are (see
/// `sanitize_rel_path`), so a link like `First post!.md` resolves to the
/// stored `First post.html` instead of a fabricated `First post!.html`.
///
/// A link whose target is **not** in this render set (excluded by audience
/// visibility, or simply missing) is stripped: the `<a>` becomes a
/// `<span class="unpublished-link">` that keeps the link text but isn't
/// clickable, so the page never points at something that 404s.
///
/// Everything that is *not* a document link — an image, a PDF, an HTML
/// attachment's island `<iframe>` — is then put through
/// `rebase_root_absolute`, because a vault writes those paths from the vault
/// root (`/img/photo.png`, prov's `path_style: root`) and a site is not always
/// served from a domain root.
pub fn transform_links(
    html: &str,
    current_path: &Path,
    path_to_filename: &HashMap<PathBuf, String>,
    workspace_dir: &Path,
    dest_filename: &str,
) -> String {
    let prefix = root_prefix(dest_filename);
    let html = &rewrite_document_links(
        html,
        current_path,
        path_to_filename,
        workspace_dir,
        dest_filename,
    );
    rebase_root_absolute(html, &prefix)
}

/// The document-link half of [`transform_links`]: `.md`/`.dj`/`.html` hrefs to
/// their published destinations, and unpublished targets to marked spans.
///
/// Runs *before* [`rebase_root_absolute`] on purpose. A document link is
/// resolved against the page holding it, so `/post.md` has to still be
/// recognizable as vault-root-absolute when it gets here; rebasing first would
/// hand it over as the page-relative `post.md` and resolve it one directory too
/// deep.
fn rewrite_document_links(
    html: &str,
    current_path: &Path,
    path_to_filename: &HashMap<PathBuf, String>,
    workspace_dir: &Path,
    dest_filename: &str,
) -> String {
    let prefix = root_prefix(dest_filename);
    // to_canonical expects workspace-relative paths
    let current_relative = current_path
        .strip_prefix(workspace_dir)
        .unwrap_or(current_path);

    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(tag_start) = remaining.find("<a ") {
        // Emit everything before the anchor verbatim.
        result.push_str(&remaining[..tag_start]);
        let after = &remaining[tag_start..];

        // Find the end of the opening tag. comrak escapes `>` inside attribute
        // values, so the first `>` reliably closes the tag.
        let Some(gt) = after.find('>') else {
            result.push_str(after);
            remaining = "";
            break;
        };
        let open_tag = &after[..=gt];
        let tail = &after[gt + 1..];

        // Only internal `.md` links are candidates for rewrite/strip.
        let canonical =
            extract_href(open_tag).and_then(|href| document_link_canonical(href, current_relative));

        match canonical {
            None => {
                // External link, anchor, or non-`.md` target — leave untouched.
                result.push_str(open_tag);
                remaining = tail;
            }
            Some((canonical, suffix)) => {
                // Anchors can't nest, so the next `</a>` closes this one.
                let Some(close) = tail.find("</a>") else {
                    result.push_str(open_tag);
                    remaining = tail;
                    continue;
                };
                let inner = &tail[..close];
                let after_close = &tail[close + "</a>".len()..];

                let key = workspace_dir.join(sanitize_rel_path(&canonical));
                match path_to_filename.get(&key) {
                    Some(html_path) => {
                        // Published target — rewrite the href, keep the anchor.
                        result.push_str(&replace_href(
                            open_tag,
                            &format!("{prefix}{html_path}{suffix}"),
                        ));
                        result.push_str(inner);
                        result.push_str("</a>");
                    }
                    None => {
                        // Not in this render set — strip to a marked span.
                        result.push_str(
                            r#"<span class="unpublished-link" title="This page isn’t published">"#,
                        );
                        result.push_str(inner);
                        result.push_str("</span>");
                    }
                }
                remaining = after_close;
            }
        }
    }
    result.push_str(remaining);

    result
}

/// Extract the raw (still percent-encoded) `href="…"` value from an opening tag.
fn extract_href(open_tag: &str) -> Option<&str> {
    let start = open_tag.find("href=\"")? + 6;
    let rest = &open_tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// If `raw_href` is an internal link to another *document*, return its
/// workspace-relative canonical path and the `?query#fragment` that rode along
/// with it; otherwise `None` (external, anchor and attachment links are
/// skipped).
///
/// "Document" is [`prov::ContentFormat`]'s judgement, not a `.md` test: a vault
/// links `.dj` and `.html` pages the same way it links `.md` ones, and each of
/// them is rewritten to its published `.html` destination.
///
/// The suffix is split off **before** the extension test, and returned so it can
/// be put back on the rewritten href. Testing the whole value meant a link to a
/// heading — `about/index.md#projects`, which is how one page points at a
/// section of another — had the extension `md#projects`, matched no content
/// format, and was left as a `.md` href pointing at a file the site does not
/// publish. It rides along unchanged rather than being re-resolved: a fragment
/// names something inside the target document, which is the same fragment
/// whatever the target's published filename turns out to be.
fn document_link_canonical<'h>(
    raw_href: &'h str,
    current_relative: &Path,
) -> Option<(String, &'h str)> {
    if raw_href.starts_with("http://")
        || raw_href.starts_with("https://")
        || raw_href.starts_with('#')
    {
        return None;
    }
    let (path, suffix) = raw_href.split_at(raw_href.find(['?', '#']).unwrap_or(raw_href.len()));
    let decoded = percent_decode(path);
    // The extension test runs on the decoded href: a link written
    // `My%20Note.md` is a document link, and `.md` is not what it ends with.
    prov::ContentFormat::from_extension(Path::new(decoded.trim()))?;
    let target = prov::Link::parse_path_only(decoded.trim()).target;
    Some((
        prov::link::resolve(current_relative, &target)
            .to_string_lossy()
            .into_owned(),
        suffix,
    ))
}

/// Replace the `href="…"` value in an opening tag, preserving other attributes.
fn replace_href(open_tag: &str, new_value: &str) -> String {
    let Some(start) = open_tag.find("href=\"") else {
        return open_tag.to_string();
    };
    let value_start = start + 6;
    let rest = &open_tag[value_start..];
    let Some(end) = rest.find('"') else {
        return open_tag.to_string();
    };
    format!("{}{}{}", &open_tag[..value_start], new_value, &rest[end..])
}

/// Rewrite vault-root-absolute `href`/`src` values into page-relative ones.
///
/// A vault names its own files from its root — `![photo](/img/photo.png)`, the
/// `path_style: root` prov writes links in — and a published site is *not*
/// always served from a domain root: the namespace serves each site under
/// `…/sites/<ns>/<site>/`, and a local preview server mounts every declared
/// site under its own name. A `/img/photo.png` left as written escapes the site and 404s
/// in both, while the attachment it means sits at `img/photo.png` below the
/// site root. Rebasing through [`root_prefix`] is what the document links
/// beside it already get.
///
/// Left alone: anything with a scheme, protocol-relative `//host/x` (absolute
/// despite the leading slash), and a bare `/` (no path to rebase, and a site
/// root is what it already means).
///
/// So a leading slash always means the *vault* root here, never the domain
/// root. That is what a vault writes — this repo's own `config.yaml` sets
/// `references: path_style: root`, so prov generates vault-root-absolute paths
/// as the ordinary spelling of a link — and the costs are not symmetric: **an
/// author who means the domain root can write a full URL**, which
/// [`absolutize_html`] and this function both leave alone, while an author who
/// means a vault path would have no way to say so. That escape hatch is the
/// documented way to point outside the site.
///
/// This runs on rendered HTML, so by the time it sees a tag the document links
/// in it are relative already — [`rewrite_document_links`] resolved them — and
/// what is left holding a leading slash is the attachment case this is for.
fn rebase_root_absolute(html: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(lt) = remaining.find('<') {
        result.push_str(&remaining[..lt]);
        let after = &remaining[lt..];

        // The first `>` closes the tag — the same assumption the rest of this
        // module makes about the renderer's attribute escaping.
        let Some(gt) = after.find('>') else {
            result.push_str(after);
            return result;
        };

        let mut tag = after[..=gt].to_string();
        for name in ["href", "src"] {
            let Some((start, end)) = find_attr_value(&tag, name) else {
                continue;
            };
            let value = &tag[start..end];
            if !value.starts_with('/') || value.starts_with("//") || value.len() == 1 {
                continue;
            }
            let rebased = format!("{prefix}{}", &value[1..]);
            tag.replace_range(start..end, &rebased);
        }
        result.push_str(&tag);
        remaining = &after[gt + 1..];
    }
    result.push_str(remaining);

    result
}

/// Rewrite a page's relative `href`/`src` values into absolute URLs under
/// `base_url` — the rendition a reader gets *away from* the site.
///
/// A rendered body carries links relative to the page holding them
/// (`../notes/target.html`, `_attachments/scan.jpg`), which is right for the
/// published HTML and wrong everywhere the body travels without its page: a
/// feed reader resolves them against the feed's own URL, and an email client
/// against nothing at all. Syndicating a body unchanged turns every internal
/// link and every image in it into a dead one.
///
/// Resolution is against the page's own directory, not the site root, because
/// that is what the body's `../` prefixes were written relative to (see
/// [`root_prefix`]).
///
/// Left alone, deliberately:
///
/// - anything carrying a scheme (`https:`, `mailto:`) or protocol-relative
///   (`//host/x`) — already absolute;
/// - fragment-only links (`#section`), which still resolve within the entry;
/// - root-relative links (`/about`). `base_url` may itself carry path segments
///   (a site is served at `…/sites/<ns>/<site>/`), so rebasing one would
///   silently move it somewhere else. A body that came through
///   [`transform_links`] has none left to worry about — the vault's own
///   root-absolute paths were made page-relative by [`rebase_root_absolute`]
///   before the feed ever saw them, and what reaches here with a leading slash
///   is something this function did not write and should not move.
///
/// A path that climbs above the site root (`../../../etc`) is left alone too:
/// it has no correct absolute form, and inventing one is worse than passing
/// through a link that was already broken.
///
/// An empty `base_url` returns the html untouched — the callers that have no
/// base skip syndication entirely.
pub fn absolutize_html(html: &str, dest_filename: &str, base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.is_empty() {
        return html.to_string();
    }
    let dir = dest_filename.rsplit_once('/').map_or("", |(dir, _)| dir);

    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(lt) = remaining.find('<') {
        result.push_str(&remaining[..lt]);
        let after = &remaining[lt..];

        // comrak escapes `>` inside attribute values, so the first `>` closes
        // the tag — the same assumption `transform_links` makes above.
        let Some(gt) = after.find('>') else {
            result.push_str(after);
            return result;
        };

        result.push_str(&absolutize_tag(&after[..=gt], dir, base));
        remaining = &after[gt + 1..];
    }
    result.push_str(remaining);

    result
}

/// Rewrite the `href` and `src` values of a single tag.
fn absolutize_tag(tag: &str, dir: &str, base: &str) -> String {
    let mut out = tag.to_string();
    for name in ["href", "src"] {
        let Some((start, end)) = find_attr_value(&out, name) else {
            continue;
        };
        let Some(absolute) = absolutize_url(&out[start..end], dir, base) else {
            continue;
        };
        out.replace_range(start..end, &absolute);
    }
    out
}

/// Byte range of the value in `name="…"`, requiring the name to begin at a
/// word boundary so `src` does not also match `data-src`.
fn find_attr_value(tag: &str, name: &str) -> Option<(usize, usize)> {
    let pattern = format!("{name}=\"");
    let mut from = 0;
    while let Some(offset) = tag[from..].find(&pattern) {
        let at = from + offset;
        let start = at + pattern.len();
        let end = start + tag[start..].find('"')?;
        if at == 0
            || tag[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            return Some((start, end));
        }
        from = end + 1;
    }
    None
}

/// Resolve one attribute value against the page's directory and the site base,
/// or `None` to leave it as written. See [`absolutize_html`] for the cases.
fn absolutize_url(value: &str, dir: &str, base: &str) -> Option<String> {
    if value.is_empty() || value.starts_with('#') || value.starts_with('/') || has_scheme(value) {
        return None;
    }

    // Only the path resolves; any `?query` / `#fragment` rides along unchanged.
    let (path, suffix) = value.split_at(value.find(['?', '#']).unwrap_or(value.len()));
    if path.is_empty() {
        return None;
    }

    let joined = if dir.is_empty() {
        path.to_string()
    } else {
        format!("{dir}/{path}")
    };
    Some(format!("{base}/{}{suffix}", normalize_rel_path(&joined)?))
}

/// Collapse `.` and `..` segments. `None` when the path climbs above its root.
fn normalize_rel_path(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return None;
    }
    let mut joined = segments.join("/");
    if path.ends_with('/') {
        joined.push('/');
    }
    Some(joined)
}

/// Whether `value` opens with a URL scheme. A relative path that merely
/// contains a colon (`notes/9:15.html`) is not one: a scheme is letters,
/// digits, `+`, `-` and `.`, starting with a letter.
fn has_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    let scheme = &value[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Sanitize a single path component for safe use in URLs. Keeps alphanumerics,
/// spaces, dots, hyphens, and underscores; strips URL-unsafe characters. Mirrors
/// the publish client's dest-name sanitization so links resolve to the stored
/// filenames.
pub fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Sanitize each component of a relative path, preserving its extension. Used to
/// normalize both stored source paths and resolved frontmatter/body links to a
/// common key form.
pub fn sanitize_rel_path(path: &str) -> String {
    let sanitized: PathBuf = Path::new(path)
        .components()
        .map(|c| match c {
            std::path::Component::Normal(s) => {
                std::ffi::OsString::from(sanitize_path_component(&s.to_string_lossy()))
            }
            other => other.as_os_str().to_owned(),
        })
        .collect();
    sanitized.to_string_lossy().into_owned()
}

/// Decode percent-encoded characters in a URL string (e.g. `%20` → ` `).
pub fn percent_decode(input: &str) -> String {
    let mut result = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            result.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| input.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_prefix_depth() {
        assert_eq!(root_prefix("index.html"), "");
        assert_eq!(root_prefix("a/b.html"), "../");
        assert_eq!(root_prefix("a/b/c.html"), "../../");
    }

    #[test]
    fn percent_decode_cases() {
        assert_eq!(percent_decode("hello"), "hello");
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(
            percent_decode("Message%20for%20my%20family.md"),
            "Message for my family.md"
        );
        assert_eq!(percent_decode("%2Fpath%2Fto%2Ffile"), "/path/to/file");
        // Incomplete sequences are left as-is
        assert_eq!(percent_decode("hello%2"), "hello%2");
        assert_eq!(percent_decode("hello%"), "hello%");
        // Invalid hex chars left as-is
        assert_eq!(percent_decode("hello%ZZ"), "hello%ZZ");
    }

    #[test]
    fn transform_links_rewrites_known_md_target() {
        let workspace = Path::new("/ws");
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("/ws/notes/target.md"),
            "notes/target.html".to_string(),
        );

        let html = r#"<a href="target.md">x</a>"#;
        let current = Path::new("/ws/notes/source.md");
        let out = transform_links(html, current, &map, workspace, "notes/source.html");
        // depth 1 → prefix "../"
        assert_eq!(out, r#"<a href="../notes/target.html">x</a>"#);
    }

    #[test]
    fn transform_links_unknown_md_is_stripped_and_marked() {
        // A link to a page that isn't in the render set (excluded/missing) must
        // not become a dead .html link — it's stripped to a marked span that
        // keeps the text but isn't clickable.
        let workspace = Path::new("/ws");
        let map = HashMap::new();
        let html = r#"<a href="missing.md">link text</a>"#;
        let current = Path::new("/ws/source.md");
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert_eq!(
            out,
            r#"<span class="unpublished-link" title="This page isn’t published">link text</span>"#
        );
    }

    /// A link to a heading of another page. The extension test used to run on
    /// the whole href, so `about/index.md#projects` had the "extension"
    /// `md#projects`, matched no content format, and was published as a `.md`
    /// link to a file the site does not serve.
    #[test]
    fn transform_links_rewrites_a_link_carrying_a_fragment() {
        let workspace = Path::new("");
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("about/index.md"),
            "about/index.html".to_string(),
        );

        let html = r##"<a href="about/index.md#projects">p</a>"##;
        let out = transform_links(html, Path::new("index.md"), &map, workspace, "index.html");
        assert_eq!(out, r##"<a href="about/index.html#projects">p</a>"##);

        // A query rides along too, and the fragment is not re-resolved against
        // anything — it names a heading inside the target either way.
        let html = r##"<a href="/about/index.md?v=2#sec">p</a>"##;
        let out = transform_links(
            html,
            Path::new("notes/deep.md"),
            &map,
            workspace,
            "notes/deep.html",
        );
        assert_eq!(out, r##"<a href="../about/index.html?v=2#sec">p</a>"##);
    }

    /// …and one whose target is not in the render set is still stripped, rather
    /// than the fragment making it look like an anchor link.
    #[test]
    fn transform_links_strips_an_unpublished_target_with_a_fragment() {
        let workspace = Path::new("");
        let map = HashMap::new();
        let html = r##"<a href="gone.md#sec">text</a>"##;
        let out = transform_links(html, Path::new("index.md"), &map, workspace, "index.html");
        assert_eq!(
            out,
            r#"<span class="unpublished-link" title="This page isn’t published">text</span>"#
        );
    }

    #[test]
    fn transform_links_resolves_sanitized_target() {
        // The link text references "First post!.md" but the stored/published key
        // is the sanitized "First post.md" → "First post.html". The '!' must not
        // leak into the href (regression for the sanitization-mismatch bug).
        let workspace = Path::new("");
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("First post.md"),
            "First post.html".to_string(),
        );
        let html = r#"<a href="First%20post!.md">x</a>"#;
        let current = Path::new("source.md");
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert_eq!(out, r#"<a href="First post.html">x</a>"#);
    }

    #[test]
    fn transform_links_preserves_inner_markup_when_stripping() {
        let workspace = Path::new("");
        let map = HashMap::new();
        let html = r#"<a href="gone.md">see <em>this</em></a>"#;
        let current = Path::new("source.md");
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert!(out.contains(r#"<span class="unpublished-link""#));
        assert!(out.contains("see <em>this</em></span>"));
        assert!(!out.contains("<a "));
    }

    #[test]
    fn absolutize_rewrites_href_and_src_from_the_root() {
        let html = r#"<a href="post.html">x</a><img src="_attachments/a.jpg">"#;
        let out = absolutize_html(html, "index.html", "https://ex.com");
        assert_eq!(
            out,
            r#"<a href="https://ex.com/post.html">x</a><img src="https://ex.com/_attachments/a.jpg">"#
        );
    }

    #[test]
    fn absolutize_resolves_against_the_pages_own_directory() {
        // The body of `a/b/c.html` writes `../` prefixes relative to itself, so
        // resolving against the site root instead would land a segment too high.
        let html = r#"<a href="../sibling.html">s</a><a href="deeper/d.html">d</a>"#;
        let out = absolutize_html(html, "a/b/c.html", "https://ex.com/");
        assert!(out.contains(r#"href="https://ex.com/a/sibling.html""#));
        assert!(out.contains(r#"href="https://ex.com/a/b/deeper/d.html""#));
    }

    #[test]
    fn absolutize_rebases_under_a_base_url_that_has_a_path() {
        // A site is served at `…/sites/<ns>/<site>/`, so the base is not an origin.
        let html = r#"<img src="../_attachments/scan.jpg">"#;
        let out = absolutize_html(html, "notes/entry.html", "https://ex.com/sites/ns/letters");
        assert_eq!(
            out,
            r#"<img src="https://ex.com/sites/ns/letters/_attachments/scan.jpg">"#
        );
    }

    #[test]
    fn absolutize_leaves_absolute_root_relative_and_fragment_links() {
        let html = r##"<a href="https://x.com/a">e</a><a href="//cdn/x.png">p</a><a href="/about">r</a><a href="#sec">f</a><a href="mailto:a@b.c">m</a>"##;
        assert_eq!(absolutize_html(html, "index.html", "https://ex.com"), html);
    }

    #[test]
    fn absolutize_keeps_query_and_fragment_suffixes() {
        let html = r##"<a href="post.html#note-1">n</a><a href="p.html?v=2">q</a>"##;
        let out = absolutize_html(html, "index.html", "https://ex.com");
        assert!(out.contains(r#"href="https://ex.com/post.html#note-1""#));
        assert!(out.contains(r#"href="https://ex.com/p.html?v=2""#));
    }

    #[test]
    fn absolutize_leaves_a_path_that_climbs_above_the_root() {
        // No correct absolute form exists; passing it through beats inventing one.
        let html = r#"<a href="../../nope.html">x</a>"#;
        assert_eq!(absolutize_html(html, "a/b.html", "https://ex.com"), html);
    }

    #[test]
    fn absolutize_does_not_match_a_suffixed_attribute_name() {
        let html = r#"<img data-src="a.jpg" src="b.jpg">"#;
        let out = absolutize_html(html, "index.html", "https://ex.com");
        assert!(out.contains(r#"data-src="a.jpg""#));
        assert!(out.contains(r#"src="https://ex.com/b.jpg""#));
    }

    #[test]
    fn absolutize_leaves_a_colon_in_a_filename_alone() {
        let html = r#"<a href="notes/9:15.html">t</a>"#;
        let out = absolutize_html(html, "index.html", "https://ex.com");
        assert_eq!(out, r#"<a href="https://ex.com/notes/9:15.html">t</a>"#);
    }

    #[test]
    fn absolutize_without_a_base_is_a_no_op() {
        let html = r#"<a href="post.html">x</a>"#;
        assert_eq!(absolutize_html(html, "index.html", ""), html);
    }

    #[test]
    fn absolutize_leaves_text_between_tags_untouched() {
        let html = r#"<p>see href="post.html" below</p><a href="post.html">x</a>"#;
        let out = absolutize_html(html, "index.html", "https://ex.com");
        assert!(out.contains(r#"see href="post.html" below"#));
        assert!(out.contains(r#"<a href="https://ex.com/post.html">"#));
    }

    /// The vault writes attachment paths from its own root, and a site is not
    /// always served from a domain root — so a root-absolute `src` has to come
    /// down to the page it sits on, exactly like the document links beside it.
    #[test]
    fn transform_links_rebases_root_absolute_attachments() {
        let workspace = Path::new("");
        let map = HashMap::new();
        let html = r#"<img src="/img/photo.png" alt="a">"#;

        // At the site root the prefix is empty, so the slash simply goes.
        let out = transform_links(html, Path::new("post.md"), &map, workspace, "post.html");
        assert_eq!(out, r#"<img src="img/photo.png" alt="a">"#);

        // A page one directory down has to climb back out first.
        let out = transform_links(
            html,
            Path::new("notes/deep.md"),
            &map,
            workspace,
            "notes/deep.html",
        );
        assert_eq!(out, r#"<img src="../img/photo.png" alt="a">"#);
    }

    /// An island `<iframe>` and a plain link to a non-document attachment are
    /// the same case — this is not an `<img>` rule.
    #[test]
    fn transform_links_rebases_every_root_absolute_src_and_href() {
        let workspace = Path::new("");
        let map = HashMap::new();
        let html = r#"<iframe class="diaryx-island" src="/att/page.html"></iframe><a href="/att/scan.pdf">s</a>"#;
        let out = transform_links(
            html,
            Path::new("notes/deep.md"),
            &map,
            workspace,
            "notes/deep.html",
        );
        assert!(out.contains(r#"src="../att/page.html""#), "got {out}");
        assert!(out.contains(r#"href="../att/scan.pdf""#), "got {out}");
    }

    /// A root-absolute link to a *document* is resolved through the render set
    /// first, so it lands on the target's published page rather than being
    /// rebased into a path that only looks right.
    #[test]
    fn transform_links_resolves_a_root_absolute_document_before_rebasing() {
        let workspace = Path::new("");
        let mut map = HashMap::new();
        map.insert(PathBuf::from("post.md"), "post.html".to_string());
        let html = r#"<a href="/post.md">x</a>"#;
        let out = transform_links(
            html,
            Path::new("notes/deep.md"),
            &map,
            workspace,
            "notes/deep.html",
        );
        assert_eq!(out, r#"<a href="../post.html">x</a>"#);
    }

    /// A leading slash does not always mean a vault path: `//host/x` is
    /// absolute, and a bare `/` is the site root already.
    #[test]
    fn transform_links_leaves_protocol_relative_and_bare_slash() {
        let workspace = Path::new("");
        let map = HashMap::new();
        let html = r#"<img src="//cdn.example/x.png"><a href="/">home</a>"#;
        let out = transform_links(
            html,
            Path::new("notes/deep.md"),
            &map,
            workspace,
            "notes/deep.html",
        );
        assert_eq!(out, html);
    }

    #[test]
    fn transform_links_leaves_external_and_anchors() {
        let workspace = Path::new("/ws");
        let map = HashMap::new();
        let current = Path::new("/ws/source.md");
        let html =
            r##"<a href="https://x.com/a.md">e</a><a href="#frag">f</a><a href="img.png">g</a>"##;
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert_eq!(out, html);
    }
}
