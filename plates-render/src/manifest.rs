//! A manifest node's page: the files it covers, listed.
//!
//! A manifest node is the bulk form of an attachment sidecar — one node
//! standing for a directory of opaque files rather than for one — and it
//! publishes the way a sidecar does: as a page in the site frame, listed by its
//! parent, whose body is what the files can be shown as. Where a sidecar's page
//! is its payload, a manifest's is a listing: the pictures as a gallery, each
//! linking to the file, and everything else as a list of links.
//!
//! Nothing here reads a file, the manifest included. `plates::collect` puts the
//! rows in the node's collected frontmatter, in place of the path to the
//! document holding them, with `root` written from the site's root:
//!
//! ```yaml
//! manifest:
//!   root: /photos/
//!   files:
//!     - path: 2019/IMG_0001.jpg
//!       title: The lake, at dawn
//! ```
//!
//! A row's `title` labels it, and its file name does when it has none. Every
//! reference is a `src` or an `href`, for the reason the attachment module
//! gives: those are what an encrypted site's reader shell decrypts.

use std::collections::HashSet;
use std::path::Path;

use prov::Mapping;

use crate::page::html_escape;

/// One covered file, as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The file's path below the site root, `/`-separated — the coordinate a
    /// shipped attachment is named by.
    pub path: String,
    /// What the listing calls it: the row's `title`, else its file name.
    pub label: String,
}

/// The files a manifest node's page lists, or `None` when `frontmatter` is not
/// a manifest node's collected copy — a page, a sidecar, or a node whose
/// `manifest:` is still the path of a document this crate cannot read.
///
/// `page` is the node's own source path, which `root` resolves against when it
/// is written relative to it rather than from the site root. A row whose file
/// is not in `published` — the site's shipped files, when the caller says what
/// they are — is left out, because a listed file that 404s is worse than one
/// not listed.
pub fn rows_of(
    frontmatter: &Mapping,
    page: &Path,
    published: Option<&HashSet<String>>,
) -> Option<Vec<Row>> {
    let manifest = frontmatter.get("manifest")?.as_mapping()?;
    let root = manifest.get("root")?.as_str()?;
    let root = prov::link::resolve(page, root);
    let rows = match manifest.get("files") {
        Some(files) => files.as_sequence()?,
        None => &[],
    };
    Some(
        rows.iter()
            .filter_map(|row| {
                let rel = row.get("path")?.as_str()?;
                let path = prov::link::normalize(root.join(rel))
                    .to_string_lossy()
                    .replace('\\', "/");
                let label = row
                    .get("title")
                    .and_then(|t| t.as_str())
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| file_name(&path).to_string());
                Some(Row { path, label })
            })
            .filter(|row| published.is_none_or(|files| files.contains(&row.path)))
            .collect(),
    )
}

/// The page body for a manifest node listing `rows`, on a page whose
/// [root prefix](crate::root_prefix) is `prefix`. Returns the HTML and the
/// Markdown a template reads as the page's source body — the images and links
/// an author would have written by hand.
///
/// A manifest with nothing to list renders as nothing, rather than an empty
/// gallery a stylesheet would draw a frame around.
pub fn render(rows: &[Row], prefix: &str) -> (String, String) {
    let (pictures, others): (Vec<&Row>, Vec<&Row>) = rows
        .iter()
        .partition(|row| crate::attachment::is_drawable(&row.path));

    let mut html = String::new();
    let mut markdown = String::new();
    if !pictures.is_empty() {
        html.push_str(r#"<div class="manifest-gallery">"#);
        for row in &pictures {
            let href = html_escape(&format!("{prefix}{}", row.path));
            let label = html_escape(&row.label);
            // Lazily, because the directory this exists for is a photo archive
            // and the page would otherwise fetch all of it to show a screenful.
            html.push_str(&format!(
                r#"<figure><a href="{href}"><img src="{href}" alt="{label}" loading="lazy"></a><figcaption>{label}</figcaption></figure>"#
            ));
            markdown.push_str(&format!("![{}]({prefix}{})\n", row.label, row.path));
        }
        html.push_str("</div>\n");
    }
    if !others.is_empty() {
        html.push_str(r#"<ul class="manifest-files">"#);
        for row in &others {
            let href = html_escape(&format!("{prefix}{}", row.path));
            html.push_str(&format!(
                r#"<li><a href="{href}">{}</a></li>"#,
                html_escape(&row.label)
            ));
            markdown.push_str(&format!("- [{}]({prefix}{})\n", row.label, row.path));
        }
        html.push_str("</ul>\n");
    }
    (html, markdown)
}

/// A *held* manifest node, as its page shows it: its files are kept on another
/// machine, and none is shipped.
///
/// `plates::collect` writes its collected copy without a `files` list, and
/// with what the page needs instead:
///
/// ```yaml
/// manifest:
///   root: /photo/
///   held: {by: Harris NAS}
///   count: 80857
///   rows: /photo.rows.json
///   sample:
///     - {path: 2026-10-27/IMG_0412.HEIC, sha256: 9f86…}
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// Who keeps the files, named for a person: `Harris NAS`.
    pub by: String,
    /// How many files the rows list.
    pub count: u64,
    /// Where the rows were shipped, below the site root, `/`-separated; `None`
    /// when the copy names none or the site does not ship it.
    pub rows: Option<String>,
    /// The rows the page shows, last by path first: each its path below the
    /// manifest's root and its SHA-256 as hex.
    pub sample: Vec<(String, String)>,
}

/// The held manifest a node's page shows, or `None` when `frontmatter` is not
/// a held manifest node's collected copy. `page` and `published` are as for
/// [`rows_of`].
pub fn held_of(
    frontmatter: &Mapping,
    page: &Path,
    published: Option<&HashSet<String>>,
) -> Option<Held> {
    let manifest = frontmatter.get("manifest")?.as_mapping()?;
    let by = manifest.get("held")?.get("by")?.as_str()?.to_string();
    let count = match manifest.get("count") {
        Some(prov::Value::Int(n)) => u64::try_from(*n).unwrap_or(0),
        _ => 0,
    };
    let rows = manifest
        .get("rows")
        .and_then(|r| r.as_str())
        .map(|rows| {
            prov::link::resolve(page, rows)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|rows| published.is_none_or(|files| files.contains(rows)));
    let sample = manifest
        .get("sample")
        .and_then(|s| s.as_sequence())
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let path = row.get("path")?.as_str()?;
            let sha256 = row.get("sha256")?.as_str()?;
            Some((path.to_string(), sha256.to_string()))
        })
        .collect();
    Some(Held {
        by,
        count,
        rows,
        sample,
    })
}

/// The page body for a held manifest node, on a page whose
/// [root prefix](crate::root_prefix) is `prefix`, and its Markdown.
///
/// Nothing to show but what it is and where its rows are: a reader with a
/// way to ask the keeper (a space's reader shell, by the hashes) draws the
/// sample from the `data-` attributes and the rest from the rows.
pub fn render_held(held: &Held, prefix: &str) -> (String, String) {
    let by = html_escape(&held.by);
    let files = if held.count == 1 { "file" } else { "files" };
    let note = format!("{} {files}, kept by {}.", thousands(held.count), held.by);
    let mut html = format!(
        r#"<section class="held" data-held-by="{by}" data-count="{}">"#,
        held.count
    );
    html.push_str(&format!(
        r#"<p class="held-note">{}</p>"#,
        html_escape(&note)
    ));
    html.push_str(r#"<ul class="held-sample">"#);
    for (path, sha256) in &held.sample {
        html.push_str(&format!(
            r#"<li data-sha256="{}" data-path="{}"></li>"#,
            html_escape(sha256),
            html_escape(path)
        ));
    }
    html.push_str("</ul>");
    let mut markdown = format!("{note}\n");
    if let Some(rows) = &held.rows {
        let href = format!("{prefix}{rows}");
        html.push_str(&format!(
            r#"<p><a class="held-rows" href="{}">Every file, listed</a></p>"#,
            html_escape(&href)
        ));
        markdown.push_str(&format!("\n[Every file, listed]({href})\n"));
    }
    html.push_str("</section>\n");
    (html, markdown)
}

/// `80857` as `80,857`.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    out
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prov::Value;

    fn node(root: &str, rows: &[(&str, Option<&str>)]) -> Mapping {
        let files = rows
            .iter()
            .map(|(path, title)| {
                let mut row = Mapping::new();
                row.insert("path".into(), Value::String(path.to_string()));
                if let Some(title) = title {
                    row.insert("title".into(), Value::String(title.to_string()));
                }
                Value::Mapping(row)
            })
            .collect();
        let mut manifest = Mapping::new();
        manifest.insert("root".into(), Value::String(root.into()));
        manifest.insert("files".into(), Value::Sequence(files));
        let mut fm = Mapping::new();
        fm.insert("title".into(), Value::String("Photos".into()));
        fm.insert("manifest".into(), Value::Mapping(manifest));
        fm
    }

    #[test]
    fn only_an_inlined_manifest_is_listed() {
        let mut referenced = Mapping::new();
        referenced.insert(
            "manifest".into(),
            Value::String("photos.manifest.yaml".into()),
        );
        assert_eq!(rows_of(&referenced, Path::new("photos.yaml"), None), None);
        assert_eq!(rows_of(&Mapping::new(), Path::new("a.md"), None), None);
    }

    #[test]
    fn a_row_resolves_below_the_root_and_is_labelled_by_its_title() {
        let fm = node(
            "/archive/photos/",
            &[("2019/a.png", Some("The lake")), ("scan.pdf", None)],
        );
        let rows = rows_of(&fm, Path::new("archive/photos.yaml"), None).unwrap();
        assert_eq!(
            rows,
            vec![
                Row {
                    path: "archive/photos/2019/a.png".into(),
                    label: "The lake".into(),
                },
                Row {
                    path: "archive/photos/scan.pdf".into(),
                    label: "scan.pdf".into(),
                },
            ]
        );
        // Written relative to the node, the root resolves against it.
        let relative = node("photos/", &[("scan.pdf", None)]);
        let rows = rows_of(&relative, Path::new("archive/photos.yaml"), None).unwrap();
        assert_eq!(rows[0].path, "archive/photos/scan.pdf");
    }

    #[test]
    fn a_file_the_site_does_not_ship_is_not_listed() {
        let fm = node("/photos/", &[("a.png", None), ("gone.png", None)]);
        let published = HashSet::from(["photos/a.png".to_string()]);
        let rows = rows_of(&fm, Path::new("photos.yaml"), Some(&published)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "photos/a.png");
    }

    #[test]
    fn pictures_are_a_gallery_and_the_rest_a_list() {
        let rows = rows_of(
            &node(
                "/photos/",
                &[("a.png", Some("A & B")), ("b.heic", None), ("c.pdf", None)],
            ),
            Path::new("photos.yaml"),
            None,
        )
        .unwrap();
        let (html, markdown) = render(&rows, "../");
        assert_eq!(
            html,
            concat!(
                r#"<div class="manifest-gallery"><figure><a href="../photos/a.png"><img src="../photos/a.png" alt="A &amp; B" loading="lazy"></a><figcaption>A &amp; B</figcaption></figure></div>"#,
                "\n",
                // A HEIC is a picture most browsers cannot draw.
                r#"<ul class="manifest-files"><li><a href="../photos/b.heic">b.heic</a></li><li><a href="../photos/c.pdf">c.pdf</a></li></ul>"#,
                "\n",
            )
        );
        assert_eq!(
            markdown,
            "![A & B](../photos/a.png)\n- [b.heic](../photos/b.heic)\n- [c.pdf](../photos/c.pdf)\n"
        );
        assert_eq!(render(&[], ""), (String::new(), String::new()));
    }

    fn held_node(count: i64, sample: &[(&str, &str)]) -> Mapping {
        let mut held = Mapping::new();
        held.insert("by".into(), Value::String("Harris & Co NAS".into()));
        let sample = sample
            .iter()
            .map(|(path, sha256)| {
                let mut row = Mapping::new();
                row.insert("path".into(), Value::String(path.to_string()));
                row.insert("sha256".into(), Value::String(sha256.to_string()));
                Value::Mapping(row)
            })
            .collect();
        let mut manifest = Mapping::new();
        manifest.insert("root".into(), Value::String("/photo/".into()));
        manifest.insert("held".into(), Value::Mapping(held));
        manifest.insert("count".into(), Value::Int(count));
        manifest.insert("rows".into(), Value::String("/photo.rows.json".into()));
        manifest.insert("sample".into(), Value::Sequence(sample));
        let mut fm = Mapping::new();
        fm.insert("manifest".into(), Value::Mapping(manifest));
        fm
    }

    #[test]
    fn a_held_manifest_says_who_keeps_it_and_where_its_rows_are() {
        let fm = held_node(
            80857,
            &[("2026-10-27/b\"c.HEIC", "9f"), ("2026-10-26/a.jpg", "0a")],
        );
        // An unheld node is not one.
        assert_eq!(
            held_of(&node("/photos/", &[]), Path::new("photos.yaml"), None),
            None
        );
        let held = held_of(&fm, Path::new("photo.yaml"), None).unwrap();
        assert_eq!(held.rows.as_deref(), Some("photo.rows.json"));
        let (html, markdown) = render_held(&held, "");
        assert_eq!(
            html,
            concat!(
                r#"<section class="held" data-held-by="Harris &amp; Co NAS" data-count="80857">"#,
                r#"<p class="held-note">80,857 files, kept by Harris &amp; Co NAS.</p>"#,
                r#"<ul class="held-sample">"#,
                r#"<li data-sha256="9f" data-path="2026-10-27/b&quot;c.HEIC"></li>"#,
                r#"<li data-sha256="0a" data-path="2026-10-26/a.jpg"></li></ul>"#,
                r#"<p><a class="held-rows" href="photo.rows.json">Every file, listed</a></p>"#,
                "</section>\n",
            )
        );
        assert_eq!(
            markdown,
            "80,857 files, kept by Harris & Co NAS.\n\n[Every file, listed](photo.rows.json)\n"
        );
        // From a page further down, the link climbs to the rows.
        let (html, _) = render_held(&held, "../");
        assert!(html.contains(r#"href="../photo.rows.json""#), "{html}");
    }

    #[test]
    fn one_held_file_is_one_and_unshipped_rows_are_not_linked() {
        let fm = held_node(1, &[]);
        let published = HashSet::from(["photo/a.jpg".to_string()]);
        let held = held_of(&fm, Path::new("photo.yaml"), Some(&published)).unwrap();
        assert_eq!(held.rows, None);
        let (html, _) = render_held(&held, "");
        assert!(html.contains(">1 file, kept by"), "{html}");
        assert!(!html.contains("held-rows"), "{html}");
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
