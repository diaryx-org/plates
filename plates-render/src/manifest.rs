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
}
