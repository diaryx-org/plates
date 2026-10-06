//! How a page says its body is set: the `font:` and `size:` in its own
//! frontmatter.
//!
//! ```yaml
//! font: serif   # serif | sans-serif | monospace | cursive | a family name
//! size: 12      # the body, in points
//! ```
//!
//! An editor that sets a document by these words sets a page; a site sets a
//! column in a stylesheet someone else chose, and the words are answered in
//! that idiom rather than copied:
//!
//! - **`size` is a ratio, not a size.** A 12-point manuscript is 12/16 of the
//!   default body, so it is set at three quarters of whatever the site's body
//!   is — the theme still decides the base, and a page that asked for a
//!   smaller face than its neighbours gets a smaller face than its neighbours,
//!   not a number that means one thing on paper and another on a phone. 16 is
//!   the size a page that names none is set at, in the editor and here.
//! - **`font` is a face, falling back to the site's.** A generic family is the
//!   browser's stack for it; a family by name is asked for first and the
//!   site's own face follows it, so a reader without that face reads the page
//!   in the face every other page is in rather than in the browser's default.
//!
//! Both reach the stylesheet as two custom properties on the page,
//! `--doc-font` and `--doc-scale`, which the built-in stylesheet reads on
//! `.content` and its headings. A caller's own stylesheet opts in by reading
//! the same two names. Nothing is written for a page that names neither, so a
//! site nobody set renders byte for byte as it did.
//!
//! `layout:` and `columns:` are not read here. `layout:` is already this
//! crate's word for which shell a page wears (`bare`, `verbatim`), and a sheet
//! of paper and a column count mean nothing on a page that scrolls; an unknown
//! `layout:` word is the site shell, as it always was.

use prov::{Mapping, Value};

use crate::appearance::FontFamily;

/// The frontmatter key a page names its face with.
pub const FONT_KEY: &str = "font";
/// The frontmatter key a page names its body size with, in points.
pub const SIZE_KEY: &str = "size";

/// The body size, in points, a page that names none is set at — what a
/// page's `size:` is a ratio *to*.
pub const DEFAULT_SIZE: f64 = 16.0;

/// What a page's frontmatter says about how its body is set, as CSS.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageSetting {
    /// A `font-family` value, when the page names a face this crate can write.
    pub font: Option<String>,
    /// The page's body size over [`DEFAULT_SIZE`], when it names one.
    pub scale: Option<f64>,
}

impl PageSetting {
    /// The setting `frontmatter` names. Each key is read generously — case,
    /// surrounding space and a trailing `pt` do not matter — and a value that
    /// is not one of these words is the same as no value at all.
    pub fn read(frontmatter: &Mapping) -> PageSetting {
        PageSetting {
            font: frontmatter
                .get(FONT_KEY)
                .and_then(|v| v.as_str())
                .and_then(font_family),
            scale: frontmatter
                .get(SIZE_KEY)
                .and_then(points)
                .map(|p| p / DEFAULT_SIZE),
        }
    }

    /// Whether the page names nothing — the site's setting throughout.
    pub fn is_empty(&self) -> bool {
        self.font.is_none() && self.scale.is_none()
    }

    /// The custom-property declarations, `--doc-font: …; --doc-scale: …;`, or
    /// an empty string for a page that names nothing.
    pub fn declarations(&self) -> String {
        let mut out = String::new();
        if let Some(font) = &self.font {
            out.push_str(&format!("--doc-font: {font}; "));
        }
        if let Some(scale) = self.scale {
            out.push_str(&format!("--doc-scale: {}; ", number(scale)));
        }
        out.trim_end().to_string()
    }

    /// A `<style>` setting [`declarations`](Self::declarations) on the
    /// document's root, for a page's `<head>`; `None` for a page that names
    /// nothing.
    pub fn style_tag(&self) -> Option<String> {
        (!self.is_empty()).then(|| format!("<style>:root {{ {} }}</style>", self.declarations()))
    }
}

/// A face as a `font-family` value: a generic's stack, or the named family
/// with the site's face after it. `None` for an empty word, and for a name
/// carrying anything but letters, digits, spaces, `-`, `_` and `.` — a face's
/// name never needs more, and a quote or a brace would let a page write CSS.
fn font_family(word: &str) -> Option<String> {
    let name = word.trim();
    if name.is_empty() {
        return None;
    }
    let generic = match name.to_lowercase().as_str() {
        "serif" => Some(FontFamily::Serif.to_css().to_string()),
        "sans-serif" => Some(FontFamily::System.to_css().to_string()),
        "monospace" => Some(FontFamily::Mono.to_css().to_string()),
        "cursive" => Some("cursive".to_string()),
        _ => None,
    };
    if generic.is_some() {
        return generic;
    }
    let safe = name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    safe.then(|| format!("\"{name}\", var(--font-family)"))
}

/// A body size in points: a positive number, written bare, as a decimal, or
/// as text with or without a trailing `pt`. leaf's bounds — up to 655.35 —
/// so a size an editor can carry is the only size read here.
fn points(value: &Value) -> Option<f64> {
    let n = match value {
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        Value::String(s) => {
            let s = s.trim();
            let s = s
                .strip_suffix("pt")
                .or_else(|| s.strip_suffix("PT"))
                .or_else(|| s.strip_suffix("Pt"))
                .unwrap_or(s)
                .trim_end();
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                return None;
            }
            s.parse().ok()?
        }
        _ => return None,
    };
    (n.is_finite() && (0.01..=655.35).contains(&n)).then_some(n)
}

/// A ratio as CSS writes a number: at most four places, no trailing zeros.
fn number(n: f64) -> String {
    let s = format!("{n:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::parse_or_empty;

    fn setting(meta: &str) -> PageSetting {
        PageSetting::read(
            &parse_or_empty(&format!("---\n{meta}---\nBody.\n"))
                .unwrap()
                .frontmatter,
        )
    }

    #[test]
    fn a_page_that_names_nothing_writes_nothing() {
        let s = setting("title: Hi\n");
        assert!(s.is_empty());
        assert_eq!(s.style_tag(), None);
    }

    #[test]
    fn a_size_is_a_ratio_to_the_default_body() {
        assert_eq!(setting("size: 12\n").scale, Some(0.75));
        assert_eq!(setting("size: 20.5\n").scale, Some(20.5 / 16.0));
        assert_eq!(setting("size: ' 12pt '\n").scale, Some(0.75));
        assert_eq!(setting("size: '18'\n").scale, Some(1.125));
        assert_eq!(setting("size: 16\n").declarations(), "--doc-scale: 1;");
        assert_eq!(setting("size: 14\n").declarations(), "--doc-scale: 0.875;");
    }

    #[test]
    fn a_size_that_is_not_points_is_no_size() {
        for meta in [
            "size: large\n",
            "size: 0\n",
            "size: -3\n",
            "size: 1000\n",
            "size: 12px\n",
        ] {
            assert_eq!(setting(meta).scale, None, "{meta}");
        }
    }

    #[test]
    fn a_generic_is_the_browsers_stack_for_it() {
        assert_eq!(
            setting("font: ' Serif '\n").font.as_deref(),
            Some(FontFamily::Serif.to_css())
        );
        assert_eq!(
            setting("font: monospace\n").font.as_deref(),
            Some(FontFamily::Mono.to_css())
        );
        assert_eq!(setting("font: cursive\n").font.as_deref(), Some("cursive"));
    }

    #[test]
    fn a_named_face_falls_back_to_the_sites() {
        assert_eq!(
            setting("font: EB Garamond\n").font.as_deref(),
            Some("\"EB Garamond\", var(--font-family)")
        );
    }

    #[test]
    fn a_name_that_could_write_css_is_no_face() {
        for meta in [
            "font: 'x\"; } body { color: red'\n",
            "font: 'a;b'\n",
            "font: '<b>'\n",
            "font: '   '\n",
        ] {
            assert_eq!(setting(meta).font, None, "{meta}");
        }
    }

    #[test]
    fn both_go_on_the_root_in_one_style() {
        assert_eq!(
            setting("font: serif\nsize: 12\n").style_tag().unwrap(),
            format!(
                "<style>:root {{ --doc-font: {}; --doc-scale: 0.75; }}</style>",
                FontFamily::Serif.to_css()
            )
        );
    }
}
