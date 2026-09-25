//! An attachment's page: what a sidecar renders as.
//!
//! A sidecar is a document — it has a title, a place in the tree, an audience
//! of its own — whose body is not prose but a payload: a photograph, a PDF, a
//! recording. It is a node like any other, so it publishes as a page like any
//! other, in the site frame, listed by its parent, walked by the pager, and
//! that page's body is the one thing the payload can be shown as. Nothing here
//! reads a file: the collector shipped the payload beside the page
//! (`plates::collect`), and the page reaches it by the sibling reference the
//! sidecar itself declares.
//!
//! The embed is chosen by the payload's extension, which is all a renderer
//! that reads no bytes can go on: an image is a figure, a video or a recording
//! is a player, a PDF is a frame with a download link beneath it for the
//! readers whose browser will not scroll one, and anything else is the link
//! alone. Every reference is a `src` or an `href`, on purpose — those are the
//! attributes an encrypted site's reader shell knows to decrypt, and an
//! `<object data>` would ship ciphertext into the viewer.

use std::path::Path;

use prov::Mapping;

use crate::page::html_escape;

/// The payload a sidecar's metadata names — its `content`, when the document
/// is an attachment (an explicit `attachment: true`, or a `content` prov
/// reads as an opaque payload rather than prose) — or `None` for a page.
///
/// The same two spellings [`prov::Document::is_attachment`] accepts, read
/// off the collected frontmatter rather than a parsed document because the
/// renderer only ever holds the former.
pub fn payload_of(frontmatter: &Mapping) -> Option<&str> {
    let content = frontmatter.get("content").and_then(|v| v.as_str())?;
    let flagged = frontmatter
        .get("attachment")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    (flagged || prov::document::is_opaque_payload(Path::new(content))).then_some(content)
}

/// The payload, when it is a picture a page can show in place of its words —
/// an image by the same extension rule the embed uses, less the formats most
/// browsers cannot draw (HEIC and HEIF), where a listing would show a broken
/// image instead of the title and description it would otherwise have.
pub fn picture_of(frontmatter: &Mapping) -> Option<&str> {
    payload_of(frontmatter).filter(|payload| {
        kind_of(payload) == Kind::Image
            && !Path::new(payload)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("heic") || e.eq_ignore_ascii_case("heif"))
    })
}

/// What kind of thing the payload is, from its name alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Image,
    Video,
    Audio,
    Document,
    Other,
}

fn kind_of(payload: &str) -> Kind {
    match Path::new(payload)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some(
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "bmp" | "heic" | "heif",
        ) => Kind::Image,
        Some("mp4" | "webm" | "mov" | "m4v" | "ogv") => Kind::Video,
        Some("mp3" | "m4a" | "aac" | "ogg" | "oga" | "wav" | "flac" | "opus") => Kind::Audio,
        Some("pdf") => Kind::Document,
        _ => Kind::Other,
    }
}

/// The page body for an attachment titled `title` whose payload sits beside
/// the page at `payload` (a sibling reference, as the sidecar declares it).
/// Returns the HTML and the Markdown a template reads as the page's source
/// body — an image or a link, which is what an author would have written.
pub fn render(title: &str, payload: &str) -> (String, String) {
    let href = html_escape(payload);
    let name = html_escape(
        Path::new(payload)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(payload),
    );
    let alt = html_escape(title);
    let download = format!(
        r#"<p class="attachment-download"><a href="{href}" download>Download {name}</a></p>"#
    );
    let html = match kind_of(payload) {
        Kind::Image => format!(
            r#"<figure class="attachment attachment-image"><img src="{href}" alt="{alt}"></figure>"#
        ),
        Kind::Video => format!(
            r#"<figure class="attachment attachment-video"><video controls src="{href}"></video></figure>
{download}"#
        ),
        Kind::Audio => format!(
            r#"<figure class="attachment attachment-audio"><audio controls src="{href}"></audio></figure>
{download}"#
        ),
        Kind::Document => format!(
            r#"<figure class="attachment attachment-document"><iframe src="{href}" title="{alt}"></iframe></figure>
{download}"#
        ),
        Kind::Other => download,
    };
    let markdown = match kind_of(payload) {
        Kind::Image => format!("![{title}]({payload})\n"),
        _ => format!("[{title}]({payload})\n"),
    };
    (html, markdown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prov::Value;

    fn fm(pairs: &[(&str, Value)]) -> Mapping {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn a_flagged_sidecar_and_an_opaque_content_are_both_attachments() {
        let flagged = fm(&[
            ("content", Value::String("notes.bin".into())),
            ("attachment", Value::Bool(true)),
        ]);
        assert_eq!(payload_of(&flagged), Some("notes.bin"));
        let opaque = fm(&[("content", Value::String("scan.pdf".into()))]);
        assert_eq!(payload_of(&opaque), Some("scan.pdf"));
        // Separated prose is a page, not an attachment.
        let prose = fm(&[("content", Value::String("body.md".into()))]);
        assert_eq!(payload_of(&prose), None);
        assert_eq!(payload_of(&fm(&[])), None);
    }

    #[test]
    fn each_kind_embeds_as_itself() {
        let (img, md) = render("Loon Lake", "loon lake.jpg");
        assert!(
            img.contains(r#"<img src="loon lake.jpg" alt="Loon Lake">"#),
            "{img}"
        );
        assert!(
            !img.contains("download"),
            "a picture needs no download link"
        );
        assert_eq!(md, "![Loon Lake](loon lake.jpg)\n");

        let (pdf, md) = render("Scan", "scan.pdf");
        assert!(
            pdf.contains(r#"<iframe src="scan.pdf" title="Scan">"#),
            "{pdf}"
        );
        assert!(
            pdf.contains(r#"<a href="scan.pdf" download>Download scan.pdf</a>"#),
            "{pdf}"
        );
        assert_eq!(md, "[Scan](scan.pdf)\n");

        let (video, _) = render("Clip", "clip.mp4");
        assert!(
            video.contains(r#"<video controls src="clip.mp4">"#),
            "{video}"
        );
        let (audio, _) = render("Voice", "voice.m4a");
        assert!(
            audio.contains(r#"<audio controls src="voice.m4a">"#),
            "{audio}"
        );

        let (other, _) = render("Archive", "backup.zip");
        assert_eq!(
            other,
            r#"<p class="attachment-download"><a href="backup.zip" download>Download backup.zip</a></p>"#
        );
    }

    #[test]
    fn the_reference_is_escaped_for_an_attribute() {
        let (html, _) = render("A & B", r#"a"b.png"#);
        assert!(html.contains(r#"src="a&quot;b.png""#), "{html}");
        assert!(html.contains(r#"alt="A &amp; B""#), "{html}");
    }
}
